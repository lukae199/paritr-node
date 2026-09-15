use std::{
    collections::BTreeSet,
    sync::Arc,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use secp256k1::{Message as SecpMessage, PublicKey, Secp256k1, SecretKey};
use thiserror::Error;

use crate::{
    codec::{CodecError, ConsensusDecode, ConsensusEncode, Reader, Writer},
    consensus::{
        Block, BlockHeader, BlockTemplate, ChainSnapshot, Transaction, Workshare, CHAIN_ID,
        MAX_BLOCK_BYTES, MAX_TEMPLATE_BYTES, MAX_TRANSACTION_BYTES, PROTOCOL_VERSION,
    },
    crypto::{domain_hash, Hash32},
    node::Node,
};

pub const WIRE_MAGIC: [u8; 4] = *b"PRT9";
pub const MAX_WIRE_FRAME: usize = MAX_BLOCK_BYTES + 64 * 1024;
pub const MAX_PEER_URLS: usize = 256;
pub const MAX_HEADERS: usize = 2_000;
const RATE_WINDOW_SECONDS: u64 = 10;
const MAX_MESSAGES_PER_WINDOW: u32 = 1_000;
const MAX_HEAVY_MESSAGES_PER_WINDOW: u32 = 200;

struct MessageBudget {
    started: Instant,
    messages: u32,
    heavy: u32,
}

impl MessageBudget {
    fn new() -> Self {
        Self {
            started: Instant::now(),
            messages: 0,
            heavy: 0,
        }
    }

    fn allow(&mut self, message: &WireMessage) -> bool {
        if self.started.elapsed().as_secs() >= RATE_WINDOW_SECONDS {
            *self = Self::new();
        }
        self.messages = self.messages.saturating_add(1);
        if matches!(
            message,
            WireMessage::Block(_)
                | WireMessage::Transaction(_)
                | WireMessage::Workshare(_)
                | WireMessage::Template(_)
        ) {
            self.heavy = self.heavy.saturating_add(1);
        }
        self.messages <= MAX_MESSAGES_PER_WINDOW && self.heavy <= MAX_HEAVY_MESSAGES_PER_WINDOW
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ObjectKind {
    Block = 1,
    Transaction = 2,
    Workshare = 3,
    Template = 4,
}

impl TryFrom<u8> for ObjectKind {
    type Error = CodecError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Block),
            2 => Ok(Self::Transaction),
            3 => Ok(Self::Workshare),
            4 => Ok(Self::Template),
            tag => Err(CodecError::InvalidTag(tag)),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Hello {
    pub protocol: u16,
    pub chain_id: String,
    pub genesis: Hash32,
    pub node_public_key: Vec<u8>,
    pub timestamp: u64,
    pub challenge: Hash32,
    pub response_to: Hash32,
    pub height: u64,
    pub tip: Hash32,
    pub cumulative_work: [u8; 32],
    pub listen_url: String,
    pub signature: Vec<u8>,
}

impl Hello {
    pub fn signed(
        secret: &SecretKey,
        snapshot: &ChainSnapshot,
        listen_url: String,
        response_to: Hash32,
    ) -> Self {
        let public = PublicKey::from_secret_key(&Secp256k1::new(), secret);
        let mut challenge = [0_u8; 32];
        rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut challenge);
        let mut work = [0_u8; 32];
        if let Ok(bytes) = hex::decode(&snapshot.cumulative_work) {
            if bytes.len() == 32 {
                work.copy_from_slice(&bytes);
            }
        }
        let mut hello = Self {
            protocol: PROTOCOL_VERSION,
            chain_id: CHAIN_ID.to_owned(),
            genesis: Block::genesis().id(),
            node_public_key: public.serialize().to_vec(),
            timestamp: unix_time(),
            challenge: Hash32(challenge),
            response_to,
            height: snapshot.height,
            tip: snapshot.tip,
            cumulative_work: work,
            listen_url,
            signature: Vec::new(),
        };
        let digest = hello.signing_hash();
        hello.signature = Secp256k1::new()
            .sign_ecdsa(&SecpMessage::from_digest(digest.0), secret)
            .serialize_compact()
            .to_vec();
        hello
    }

    pub fn verify(&self, now: u64) -> bool {
        if self.protocol != PROTOCOL_VERSION
            || self.chain_id != CHAIN_ID
            || self.genesis != Block::genesis().id()
            || self.node_public_key.len() != 33
            || self.signature.len() != 64
            || self.timestamp.abs_diff(now) > 300
        {
            return false;
        }
        let Ok(public) = PublicKey::from_slice(&self.node_public_key) else {
            return false;
        };
        let Ok(signature) = secp256k1::ecdsa::Signature::from_compact(&self.signature) else {
            return false;
        };
        Secp256k1::verification_only()
            .verify_ecdsa(
                &SecpMessage::from_digest(self.signing_hash().0),
                &signature,
                &public,
            )
            .is_ok()
    }

    pub fn node_id(&self) -> Hash32 {
        domain_hash(b"PARITR-P9-NODE-ID-v1", &self.node_public_key)
    }

    fn signing_hash(&self) -> Hash32 {
        let mut clone = self.clone();
        clone.signature.clear();
        domain_hash(b"PARITR-P9-HELLO-v1", &clone.consensus_encode())
    }
}

impl ConsensusEncode for Hello {
    fn encode_to(&self, writer: &mut Writer) {
        writer.u16(self.protocol);
        writer.string(&self.chain_id);
        self.genesis.encode_to(writer);
        writer.bytes(&self.node_public_key);
        writer.u64(self.timestamp);
        self.challenge.encode_to(writer);
        self.response_to.encode_to(writer);
        writer.u64(self.height);
        self.tip.encode_to(writer);
        writer.fixed(&self.cumulative_work);
        writer.string(&self.listen_url);
        writer.bytes(&self.signature);
    }
}

impl ConsensusDecode for Hello {
    fn decode_from(reader: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            protocol: reader.u16()?,
            chain_id: reader.string(64)?.to_owned(),
            genesis: Hash32::decode_from(reader)?,
            node_public_key: reader.bytes(33)?.to_vec(),
            timestamp: reader.u64()?,
            challenge: Hash32::decode_from(reader)?,
            response_to: Hash32::decode_from(reader)?,
            height: reader.u64()?,
            tip: Hash32::decode_from(reader)?,
            cumulative_work: reader.fixed()?,
            listen_url: reader.string(512)?.to_owned(),
            signature: reader.bytes(64)?.to_vec(),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WireMessage {
    Hello(Hello),
    Inventory { kind: ObjectKind, id: Hash32 },
    GetData { kind: ObjectKind, id: Hash32 },
    Block(Block),
    Transaction(Transaction),
    Workshare(Workshare),
    Template(BlockTemplate),
    GetHeaders { locator: Vec<Hash32>, stop: Hash32 },
    Headers(Vec<BlockHeader>),
    Ping(u64),
    Pong(u64),
    Peers(Vec<String>),
}

impl WireMessage {
    pub fn encode_frame(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        writer.fixed(&WIRE_MAGIC);
        self.encode_to(&mut writer);
        writer.into_inner()
    }

    pub fn decode_frame(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() > MAX_WIRE_FRAME {
            return Err(CodecError::LimitExceeded {
                actual: bytes.len(),
                limit: MAX_WIRE_FRAME,
            });
        }
        let mut reader = Reader::new(bytes);
        if reader.fixed::<4>()? != WIRE_MAGIC {
            return Err(CodecError::NonCanonical("wrong P2P magic"));
        }
        let message = Self::decode_from(&mut reader)?;
        reader.finish()?;
        Ok(message)
    }
}

impl ConsensusEncode for WireMessage {
    fn encode_to(&self, writer: &mut Writer) {
        match self {
            Self::Hello(value) => {
                writer.u8(1);
                writer.bytes(&value.consensus_encode());
            }
            Self::Inventory { kind, id } => {
                writer.u8(2);
                writer.u8(*kind as u8);
                id.encode_to(writer);
            }
            Self::GetData { kind, id } => {
                writer.u8(3);
                writer.u8(*kind as u8);
                id.encode_to(writer);
            }
            Self::Block(value) => {
                writer.u8(4);
                writer.bytes(&value.consensus_encode());
            }
            Self::Transaction(value) => {
                writer.u8(5);
                writer.bytes(&value.consensus_encode());
            }
            Self::Workshare(value) => {
                writer.u8(6);
                writer.bytes(&value.consensus_encode());
            }
            Self::Template(value) => {
                writer.u8(7);
                writer.bytes(&value.consensus_encode());
            }
            Self::GetHeaders { locator, stop } => {
                writer.u8(8);
                writer.u32(u32::try_from(locator.len()).expect("locator count fits u32"));
                for hash in locator {
                    hash.encode_to(writer);
                }
                stop.encode_to(writer);
            }
            Self::Headers(headers) => {
                writer.u8(9);
                writer.u32(u32::try_from(headers.len()).expect("header count fits u32"));
                for header in headers {
                    header.encode_to(writer);
                }
            }
            Self::Ping(nonce) => {
                writer.u8(10);
                writer.u64(*nonce);
            }
            Self::Pong(nonce) => {
                writer.u8(11);
                writer.u64(*nonce);
            }
            Self::Peers(peers) => {
                writer.u8(12);
                writer.u32(u32::try_from(peers.len()).expect("peer count fits u32"));
                for peer in peers {
                    writer.string(peer);
                }
            }
        }
    }
}

impl ConsensusDecode for WireMessage {
    fn decode_from(reader: &mut Reader<'_>) -> Result<Self, CodecError> {
        match reader.u8()? {
            1 => Ok(Self::Hello(Hello::consensus_decode(reader.bytes(2_048)?)?)),
            2 => Ok(Self::Inventory {
                kind: reader.u8()?.try_into()?,
                id: Hash32::decode_from(reader)?,
            }),
            3 => Ok(Self::GetData {
                kind: reader.u8()?.try_into()?,
                id: Hash32::decode_from(reader)?,
            }),
            4 => Ok(Self::Block(Block::consensus_decode(
                reader.bytes(MAX_BLOCK_BYTES)?,
            )?)),
            5 => Ok(Self::Transaction(Transaction::consensus_decode(
                reader.bytes(MAX_TRANSACTION_BYTES)?,
            )?)),
            6 => Ok(Self::Workshare(Workshare::consensus_decode(
                reader.bytes(512)?,
            )?)),
            7 => Ok(Self::Template(BlockTemplate::consensus_decode(
                reader.bytes(MAX_TEMPLATE_BYTES)?,
            )?)),
            8 => {
                let count = reader.vector_len(64)?;
                let mut locator = Vec::with_capacity(count);
                for _ in 0..count {
                    locator.push(Hash32::decode_from(reader)?);
                }
                Ok(Self::GetHeaders {
                    locator,
                    stop: Hash32::decode_from(reader)?,
                })
            }
            9 => {
                let count = reader.vector_len(MAX_HEADERS)?;
                let mut headers = Vec::with_capacity(count);
                for _ in 0..count {
                    headers.push(BlockHeader::decode_from(reader)?);
                }
                Ok(Self::Headers(headers))
            }
            10 => Ok(Self::Ping(reader.u64()?)),
            11 => Ok(Self::Pong(reader.u64()?)),
            12 => {
                let count = reader.vector_len(MAX_PEER_URLS)?;
                let mut peers = Vec::with_capacity(count);
                let mut unique = BTreeSet::new();
                for _ in 0..count {
                    let peer = reader.string(512)?.to_owned();
                    if !unique.insert(peer.clone()) {
                        return Err(CodecError::NonCanonical("duplicate peer URL"));
                    }
                    peers.push(peer);
                }
                Ok(Self::Peers(peers))
            }
            tag => Err(CodecError::InvalidTag(tag)),
        }
    }
}

#[derive(Debug, Error)]
pub enum PeerError {
    #[error("P2P codec error: {0}")]
    Codec(#[from] CodecError),
    #[error("peer did not send a valid authenticated hello")]
    Handshake,
    #[error("websocket error: {0}")]
    WebSocket(String),
    #[error("peer message rate exceeded")]
    RateLimit,
}

async fn send_message<S, M>(socket: &mut S, message: M) -> Result<(), PeerError>
where
    S: futures_util::Sink<M> + Unpin,
    S::Error: std::fmt::Display,
{
    tokio::time::timeout(std::time::Duration::from_secs(10), socket.send(message))
        .await
        .map_err(|_| PeerError::WebSocket("write timeout".to_owned()))?
        .map_err(|error| PeerError::WebSocket(error.to_string()))
}

async fn process_message(
    node: Arc<Node>,
    peer: Hash32,
    request: WireMessage,
) -> Result<Option<WireMessage>, PeerError> {
    static VALIDATION_SLOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);
    let permit = VALIDATION_SLOTS
        .acquire()
        .await
        .map_err(|_| PeerError::Handshake)?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        node.handle_wire(peer, request)
    })
    .await
    .map_err(|error| PeerError::WebSocket(error.to_string()))
}

#[allow(clippy::too_many_lines)]
pub async fn serve_socket(mut socket: WebSocket, node: Arc<Node>) -> Result<(), PeerError> {
    if !node.is_enabled() {
        return Err(PeerError::Handshake);
    }
    let hello = node.local_hello(Hash32::ZERO);
    send_message(
        &mut socket,
        Message::Binary(WireMessage::Hello(hello.clone()).encode_frame().into()),
    )
    .await?;

    let first = tokio::time::timeout(std::time::Duration::from_secs(10), socket.next())
        .await
        .map_err(|_| PeerError::Handshake)?
        .ok_or(PeerError::Handshake)?
        .map_err(|error| PeerError::WebSocket(error.to_string()))?;
    let Message::Binary(bytes) = first else {
        return Err(PeerError::Handshake);
    };
    let WireMessage::Hello(remote) = WireMessage::decode_frame(&bytes)? else {
        return Err(PeerError::Handshake);
    };
    if !remote.verify(unix_time())
        || remote.response_to != hello.challenge
        || remote.node_id() == node.node_id()
    {
        return Err(PeerError::Handshake);
    }
    send_message(
        &mut socket,
        Message::Binary(
            WireMessage::Hello(node.local_hello(remote.challenge))
                .encode_frame()
                .into(),
        ),
    )
    .await?;
    let remote_id = remote.node_id();
    if !node.peer_connected(remote_id, true) {
        return Err(PeerError::Handshake);
    }
    node.learn_peer(&remote.listen_url);
    let mut events = node.subscribe();
    let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(30));
    let mut budget = MessageBudget::new();
    let mut last_received = Instant::now();
    let mut maintenance = tokio::time::interval(std::time::Duration::from_secs(1));
    let result = async {
        if remote.tip != node.chain_snapshot().tip {
            send_message(&mut socket, Message::Binary(
                    WireMessage::GetHeaders {
                        locator: node.block_locator(),
                        stop: Hash32::ZERO,
                    }
                    .encode_frame()
                    .into(),
                )).await?;
        }
        loop {
            tokio::select! {
                _ = maintenance.tick() => {
                    if !node.is_enabled() || last_received.elapsed().as_secs() > 90 { break; }
                }
                incoming = socket.next() => {
                    let Some(message) = incoming else { break };
                    last_received = Instant::now();
                    let message = message.map_err(|error| PeerError::WebSocket(error.to_string()))?;
                    match message {
                        Message::Binary(bytes) => {
                            let request = WireMessage::decode_frame(&bytes)?;
                            if !budget.allow(&request) {
                                return Err(PeerError::RateLimit);
                            }
                            if let Some(response) = process_message(Arc::clone(&node), remote_id, request).await? {
                                send_message(&mut socket, Message::Binary(response.encode_frame().into())).await?;
                            }
                        }
                        Message::Ping(payload) => send_message(&mut socket, Message::Pong(payload)).await?,
                        Message::Close(_) => break,
                        Message::Text(_) | Message::Pong(_) => {}
                    }
                }
                event = events.recv() => {
                    if let Ok(event) = event {
                        send_message(&mut socket, Message::Binary(event.encode_frame().into())).await?;
                    }
                }
                _ = heartbeat.tick() => {
                    send_message(&mut socket, Message::Binary(WireMessage::Peers(node.peer_addresses()).encode_frame().into())).await?;
                    send_message(&mut socket, Message::Binary(WireMessage::Ping(rand::random()).encode_frame().into())).await?;
                }
            }
        }
        Ok(())
    }
    .await;
    node.peer_disconnected(remote_id);
    result
}

pub fn spawn_outbound_manager(node: &Arc<Node>) -> Vec<tokio::task::JoinHandle<()>> {
    let node = Arc::clone(node);
    vec![tokio::spawn(async move {
        let mut tasks = tokio::task::JoinSet::new();
        let mut running = BTreeSet::new();
        let mut retry = std::collections::BTreeMap::new();
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(2));
        loop {
            tokio::select! {
                _ = tick.tick() => {
                    if !node.is_enabled() { continue; }
                    let own = node.mining_config().public_url.as_deref().and_then(normalize_peer_url);
                    let urls: BTreeSet<_> = node.config.seed_nodes.iter().chain(node.config.peers.iter())
                        .cloned().chain(node.peer_addresses()).filter_map(|url| normalize_peer_url(&url)).collect();
                    for url in urls {
                        if tasks.len() >= node.config.max_outbound_peers { break; }
                        if own.as_ref() == Some(&url) || running.contains(&url)
                            || retry.get(&url).is_some_and(|next| Instant::now() < *next) { continue; }
                        running.insert(url.clone());
                        let node = Arc::clone(&node);
                        tasks.spawn(async move {
                            if let Err(error) = outbound_once(node, &url).await {
                                tracing::debug!(peer = %url, %error, "outbound P2P connection ended");
                            }
                            url
                        });
                    }
                }
                result = tasks.join_next(), if !tasks.is_empty() => {
                    if let Some(Ok(url)) = result {
                        running.remove(&url);
                        retry.insert(url, Instant::now() + std::time::Duration::from_secs(30 + rand::random::<u64>() % 15));
                    }
                }
            }
        }
    })]
}

#[allow(clippy::too_many_lines)]
async fn outbound_once(node: Arc<Node>, url: &str) -> Result<(), PeerError> {
    use tokio_tungstenite::tungstenite::Message as TungsteniteMessage;

    let (mut socket, _) = tokio::time::timeout(std::time::Duration::from_secs(15), async {
        // Connect to the checked DNS result, not a second potentially rebound lookup.
        let addresses = ensure_public_peer(url).await?;
        let stream = tokio::net::TcpStream::connect(addresses.as_slice())
            .await
            .map_err(|error| PeerError::WebSocket(error.to_string()))?;
        let limits = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
            .max_message_size(Some(MAX_WIRE_FRAME))
            .max_frame_size(Some(MAX_WIRE_FRAME));
        tokio_tungstenite::client_async_tls_with_config(url, stream, Some(limits), None)
            .await
            .map_err(|error| PeerError::WebSocket(error.to_string()))
    })
    .await
    .map_err(|_| PeerError::Handshake)??;
    let first = tokio::time::timeout(std::time::Duration::from_secs(10), socket.next())
        .await
        .map_err(|_| PeerError::Handshake)?
        .ok_or(PeerError::Handshake)?
        .map_err(|error| PeerError::WebSocket(error.to_string()))?;
    let TungsteniteMessage::Binary(bytes) = first else {
        return Err(PeerError::Handshake);
    };
    let WireMessage::Hello(challenge) = WireMessage::decode_frame(&bytes)? else {
        return Err(PeerError::Handshake);
    };
    if !challenge.verify(unix_time())
        || challenge.response_to != Hash32::ZERO
        || challenge.node_id() == node.node_id()
    {
        return Err(PeerError::Handshake);
    }
    let response = node.local_hello(challenge.challenge);
    send_message(
        &mut socket,
        TungsteniteMessage::Binary(WireMessage::Hello(response.clone()).encode_frame().into()),
    )
    .await?;
    let acknowledgement = tokio::time::timeout(std::time::Duration::from_secs(10), socket.next())
        .await
        .map_err(|_| PeerError::Handshake)?
        .ok_or(PeerError::Handshake)?
        .map_err(|error| PeerError::WebSocket(error.to_string()))?;
    let TungsteniteMessage::Binary(bytes) = acknowledgement else {
        return Err(PeerError::Handshake);
    };
    let WireMessage::Hello(remote) = WireMessage::decode_frame(&bytes)? else {
        return Err(PeerError::Handshake);
    };
    if !remote.verify(unix_time())
        || remote.node_id() != challenge.node_id()
        || remote.response_to != response.challenge
    {
        return Err(PeerError::Handshake);
    }
    let remote_id = remote.node_id();
    if !node.peer_connected(remote_id, false) {
        return Err(PeerError::Handshake);
    }
    node.learn_peer(&remote.listen_url);
    let mut events = node.subscribe();
    let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(30));
    let mut budget = MessageBudget::new();
    let mut last_received = Instant::now();
    let mut maintenance = tokio::time::interval(std::time::Duration::from_secs(1));
    let result = async {
        if remote.tip != node.chain_snapshot().tip {
            send_message(&mut socket, TungsteniteMessage::Binary(
                    WireMessage::GetHeaders {
                        locator: node.block_locator(),
                        stop: Hash32::ZERO,
                    }
                    .encode_frame()
                    .into(),
                )).await?;
        }
        loop {
            tokio::select! {
                _ = maintenance.tick() => {
                    if !node.is_enabled() || last_received.elapsed().as_secs() > 90 { break; }
                }
                incoming = socket.next() => {
                    let Some(message) = incoming else { break };
                    last_received = Instant::now();
                    let message = message.map_err(|error| PeerError::WebSocket(error.to_string()))?;
                    match message {
                        TungsteniteMessage::Binary(bytes) => {
                            let request = WireMessage::decode_frame(&bytes)?;
                            if !budget.allow(&request) {
                                return Err(PeerError::RateLimit);
                            }
                            if let Some(response) = process_message(Arc::clone(&node), remote_id, request).await? {
                                send_message(&mut socket, TungsteniteMessage::Binary(response.encode_frame().into())).await?;
                            }
                        }
                        TungsteniteMessage::Ping(payload) => send_message(&mut socket, TungsteniteMessage::Pong(payload)).await?,
                        TungsteniteMessage::Close(_) => break,
                        TungsteniteMessage::Text(_) | TungsteniteMessage::Pong(_) | TungsteniteMessage::Frame(_) => {}
                    }
                }
                event = events.recv() => {
                    if let Ok(event) = event {
                        send_message(&mut socket, TungsteniteMessage::Binary(event.encode_frame().into())).await?;
                    }
                }
                _ = heartbeat.tick() => {
                    send_message(&mut socket, TungsteniteMessage::Binary(WireMessage::Peers(node.peer_addresses()).encode_frame().into())).await?;
                    send_message(&mut socket, TungsteniteMessage::Binary(WireMessage::Ping(rand::random()).encode_frame().into())).await?;
                }
            }
        }
        Ok(())
    }.await;
    node.peer_disconnected(remote_id);
    result
}

pub(crate) fn normalize_peer_url(value: &str) -> Option<String> {
    if value.len() > 512 {
        return None;
    }
    let raw = value.trim();
    let mut url = url::Url::parse(raw).ok()?;
    match url.scheme() {
        "https" => url.set_scheme("wss").ok()?,
        "http" => url.set_scheme("ws").ok()?,
        "wss" | "ws" => {}
        _ => return None,
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    if url.path().is_empty() || url.path() == "/" {
        url.set_path("/p2p/v9");
    }
    if url.path() != "/p2p/v9" {
        return None;
    }
    Some(url.to_string())
}

async fn ensure_public_peer(value: &str) -> Result<Vec<std::net::SocketAddr>, PeerError> {
    let url = url::Url::parse(value).map_err(|_| PeerError::Handshake)?;
    let host = url.host_str().ok_or(PeerError::Handshake)?;
    let port = url.port_or_known_default().ok_or(PeerError::Handshake)?;
    let addresses = tokio::net::lookup_host((host, port))
        .await
        .map_err(|error| PeerError::WebSocket(error.to_string()))?;
    let mut checked = Vec::new();
    for address in addresses {
        let ip = address.ip();
        let private = ip.is_loopback()
            || ip.is_unspecified()
            || match ip {
                std::net::IpAddr::V4(ip) => {
                    ip.is_private()
                        || ip.is_link_local()
                        || ip.is_multicast()
                        || ip.is_broadcast()
                        || ip.is_documentation()
                        || ip.octets()[0] == 0
                        || ip.octets()[0] >= 240
                        || (ip.octets()[0] == 100 && (64..=127).contains(&ip.octets()[1]))
                }
                std::net::IpAddr::V6(ip) => {
                    ip.is_unique_local()
                        || ip.is_unicast_link_local()
                        || ip.is_multicast()
                        || ip.to_ipv4_mapped().is_some()
                }
            };
        if private && !(url.scheme() == "ws" && ip.is_loopback()) {
            return Err(PeerError::Handshake);
        }
        if checked.len() < 32 {
            checked.push(address);
        }
    }
    if checked.is_empty() {
        Err(PeerError::Handshake)
    } else {
        Ok(checked)
    }
}

fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn discovery_keeps_private_services_out_of_public_peer_dials() {
        assert!(ensure_public_peer("wss://127.0.0.1:5051/p2p/v9")
            .await
            .is_err());
        assert!(ensure_public_peer("wss://10.0.0.1:5050/p2p/v9")
            .await
            .is_err());
        assert!(normalize_peer_url("https://example.org/admin/config").is_none());
        assert_eq!(
            normalize_peer_url("https://example.org").as_deref(),
            Some("wss://example.org/p2p/v9")
        );
    }

    #[test]
    fn frame_round_trip_and_magic_are_canonical() {
        let message = WireMessage::GetHeaders {
            locator: vec![Block::genesis().id(), Hash32([7; 32])],
            stop: Hash32::ZERO,
        };
        let encoded = message.encode_frame();
        assert_eq!(WireMessage::decode_frame(&encoded).unwrap(), message);

        let mut wrong_magic = encoded.clone();
        wrong_magic[0] ^= 1;
        assert!(matches!(
            WireMessage::decode_frame(&wrong_magic),
            Err(CodecError::NonCanonical("wrong P2P magic"))
        ));

        let mut trailing = encoded;
        trailing.push(0);
        assert!(matches!(
            WireMessage::decode_frame(&trailing),
            Err(CodecError::TrailingBytes(1))
        ));
    }

    #[test]
    fn hello_authentication_binds_chain_and_payload() {
        let secret = SecretKey::from_slice(&[11; 32]).unwrap();
        let snapshot = crate::consensus::Chain::genesis().snapshot();
        let hello = Hello::signed(
            &secret,
            &snapshot,
            "https://node.example/p2p/v9".to_owned(),
            Hash32([3; 32]),
        );
        assert!(hello.verify(hello.timestamp));
        assert_eq!(hello.response_to, Hash32([3; 32]));

        let mut tampered = hello;
        tampered.height += 1;
        assert!(!tampered.verify(tampered.timestamp));
    }

    #[test]
    fn duplicate_peer_urls_are_rejected() {
        let message = WireMessage::Peers(vec![
            "wss://node.example/p2p/v9".to_owned(),
            "wss://node.example/p2p/v9".to_owned(),
        ]);
        assert!(matches!(
            WireMessage::decode_frame(&message.encode_frame()),
            Err(CodecError::NonCanonical("duplicate peer URL"))
        ));
    }
}
