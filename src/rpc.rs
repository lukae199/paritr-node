use std::{
    collections::HashMap,
    io::{Read, Seek, SeekFrom},
    net::SocketAddr,
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use axum::{
    body::{Body, Bytes},
    extract::{Path, Query, Request, State, WebSocketUpgrade},
    http::{header, HeaderValue, Method, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use base64::Engine;
use serde::{Deserialize, Serialize};
use tower_http::{cors::CorsLayer, limit::RequestBodyLimitLayer, trace::TraceLayer};

use crate::{
    codec::{ConsensusDecode, ConsensusEncode},
    config::{valid_device_name, Config, MiningMode},
    consensus::{self, Block, BlockTemplate, Transaction, Workshare},
    crypto::{domain_hash, Address, Hash32},
    node::Node,
    p2p,
    policy::{DEFAULT_DUST_LIMIT, DEFAULT_MIN_RELAY_FEE},
};

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(error: impl std::fmt::Display) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: error.to_string(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    fn unavailable(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(serde_json::json!({ "error": self.message })),
        )
            .into_response()
    }
}

#[derive(Serialize)]
struct Health {
    status: &'static str,
    chain_id: &'static str,
    protocol: u16,
    protocol_version: u16,
    node_version: &'static str,
}

#[derive(Serialize)]
struct ChainParameters {
    chain_id: &'static str,
    protocol_version: u16,
    header_version: u16,
    target_block_time: u64,
    daa: &'static str,
    asert_half_life: i64,
    initial_subsidy: u64,
    minimum_subsidy: u64,
    halving_interval: u64,
    finder_share_percent: u64,
    reward_window: u64,
    reward_maturity: u64,
    workshare_multiplier: u64,
    workshare_target_seconds: u64,
    randomx_version: &'static str,
    randomx_epoch_blocks: u64,
    randomx_seed_lag: u64,
    consensus_minimum_fee: u64,
    relay_minimum_fee: u64,
    relay_dust_limit: u64,
    maximum_reorg_depth: Option<u64>,
}

#[derive(Serialize)]
struct BlockResponse {
    id: Hash32,
    block: Block,
}

#[derive(Serialize)]
struct AddressResponse {
    address: Address,
    balance: u64,
    nonce: u64,
    pending_rewards: u64,
    confirmed: u64,
    pending_in: u64,
    pending_out: u64,
    spendable: u64,
    next_nonce: u64,
    immature: u64,
    reward_credit: u64,
    height: u64,
    coin: u64,
}

#[derive(Deserialize)]
struct MiningUpdate {
    miner_address: Option<String>,
    mining_enabled: Option<bool>,
    mining_processes: Option<usize>,
    mining_intensity: Option<u8>,
    randomx_mode: Option<MiningMode>,
}

#[derive(Deserialize)]
struct DeviceUpdate {
    device_name: Option<String>,
    public_url: Option<String>,
}

#[derive(Deserialize)]
struct PairRequest {
    portal_url: String,
    code: String,
}

#[derive(Deserialize)]
struct PeerUpdate {
    #[serde(default)]
    url: String,
    #[serde(default)]
    ident: String,
}

#[derive(Serialize)]
struct MiningTemplateResponse {
    block: Block,
    block_binary_base64: String,
    template_id: Hash32,
    randomx_seed_hex: String,
    block_target_hex: String,
    workshare_target_hex: String,
    generation: u64,
}

pub fn public_router(node: Arc<Node>) -> Router {
    let public = Router::new()
        .route("/", get(status))
        .route("/health", get(health))
        .route("/status", get(status))
        .route("/info", get(status))
        .route("/chain/params", get(chain_params))
        .route("/supply", get(supply))
        .route("/fee/estimate", get(fee_estimate))
        .route("/p2p/info", get(p2p_info))
        .route("/p2p/whoami", get(p2p_whoami))
        .route("/mining/info", get(status))
        .route("/mining/distribution", get(mining_distribution))
        .route("/mining/rewards/{address}", get(mining_rewards))
        .route("/block/{selector}", get(block))
        .route("/block/{selector}/raw", get(raw_block))
        .route("/address/{address}", get(address))
        .route("/address/{address}/transactions", get(address_transactions))
        .route("/transaction/{id}", get(transaction))
        .route("/transaction", post(submit_transaction))
        .route("/block", post(submit_block))
        .route("/block/raw", post(submit_raw_block))
        .route("/workshare", post(submit_workshare))
        .route("/workshare/template", post(submit_template))
        .route("/mining/template", get(mining_template))
        .route("/p2p/v9", get(p2p_socket))
        .layer(RequestBodyLimitLayer::new(consensus::MAX_BLOCK_BYTES))
        .layer(
            CorsLayer::new()
                .allow_origin(HeaderValue::from_static("*"))
                .allow_methods([Method::GET, Method::POST])
                .allow_headers([header::CONTENT_TYPE]),
        )
        .layer(TraceLayer::new_for_http())
        .with_state(Arc::clone(&node));

    // Directly registered HTTPS nodes use the same public base URL for the
    // secret-protected admin calls. The dedicated 5051 listener remains the
    // loopback path used by the outbound portal agent.
    public.merge(admin_router(node))
}

pub fn admin_router(node: Arc<Node>) -> Router {
    Router::new()
        .route("/admin/auth", get(admin_auth))
        .route("/admin/auth", post(admin_auth))
        .route("/admin/status", get(status))
        .route("/admin/config", get(admin_config).post(admin_config_update))
        .route("/admin/mining", post(admin_mining))
        .route("/admin/pair", post(admin_pair))
        .route("/admin/unpair", post(admin_unpair))
        .route("/admin/peers", get(admin_peers))
        .route("/admin/peers/add", post(admin_peer_add))
        .route("/admin/peers/remove", post(admin_peer_remove))
        .route("/admin/peers/unban", post(admin_peer_unban))
        .route("/admin/mempool", get(admin_mempool))
        .route("/admin/blocks", get(admin_blocks))
        .route("/admin/logs", get(admin_logs))
        .route("/admin/update", get(admin_update))
        .route("/state/snapshot", get(state_snapshot))
        .route("/admin/sync", post(admin_sync))
        .route("/admin/start", post(admin_start))
        .route("/admin/stop", post(admin_stop))
        .route("/admin/restart", post(admin_restart))
        .layer(middleware::from_fn_with_state(
            Arc::clone(&node),
            require_admin,
        ))
        .layer(RequestBodyLimitLayer::new(64 * 1024))
        .layer(TraceLayer::new_for_http())
        .with_state(node)
}

pub fn management_router(node: Arc<Node>) -> Router {
    Router::new()
        .route(
            "/",
            get(|| async { Html(include_str!("admin/index.html")) }),
        )
        .route(
            "/app.js",
            get(|| async {
                (
                    [(
                        header::CONTENT_TYPE,
                        "application/javascript; charset=utf-8",
                    )],
                    include_str!("admin/app.js"),
                )
            }),
        )
        .route(
            "/logo.svg",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "image/svg+xml")],
                    include_str!("admin/logo.svg"),
                )
            }),
        )
        .route(
            "/style.css",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
                    include_str!("admin/style.css"),
                )
            }),
        )
        .merge(admin_router(node))
}

pub async fn serve(node: Arc<Node>) -> anyhow::Result<()> {
    let public_address: SocketAddr = node.config.public_bind.parse()?;
    let admin_address: SocketAddr = node.config.admin_bind.parse()?;
    let management_address: SocketAddr = node.config.management_bind.parse()?;
    if public_address.port() == admin_address.port()
        || public_address.port() == management_address.port()
    {
        anyhow::bail!("public and management ports must be different");
    }
    let public_listener = tokio::net::TcpListener::bind(public_address).await?;
    let management_listener = tokio::net::TcpListener::bind(management_address).await?;
    tracing::info!(%public_address, "public RPC/P2P listening");
    tracing::info!(%management_address, "management UI and authenticated admin API listening");
    if admin_address.port() == management_address.port() {
        anyhow::ensure!(
            admin_address.is_ipv4() == management_address.is_ipv4()
                && (management_address.ip().is_unspecified()
                    || management_address.ip() == admin_address.ip()),
            "shared management listener must also accept the loopback admin address"
        );
        tokio::try_join!(
            axum::serve(public_listener, public_router(Arc::clone(&node))),
            axum::serve(management_listener, management_router(node)),
        )?;
        return Ok(());
    }
    // Keep explicitly configured legacy installations with a separate port working.
    let admin_listener = tokio::net::TcpListener::bind(admin_address).await?;
    tokio::try_join!(
        axum::serve(public_listener, public_router(Arc::clone(&node))),
        axum::serve(admin_listener, admin_router(Arc::clone(&node))),
        axum::serve(management_listener, management_router(node)),
    )?;
    Ok(())
}

async fn health() -> Json<Health> {
    Json(Health {
        status: "ok",
        chain_id: consensus::CHAIN_ID,
        protocol: consensus::PROTOCOL_VERSION,
        protocol_version: consensus::PROTOCOL_VERSION,
        node_version: consensus::NODE_VERSION,
    })
}

async fn status(State(node): State<Arc<Node>>) -> Json<crate::node::NodeStatus> {
    Json(node.status())
}

async fn chain_params() -> Json<ChainParameters> {
    Json(ChainParameters {
        chain_id: consensus::CHAIN_ID,
        protocol_version: consensus::PROTOCOL_VERSION,
        header_version: consensus::HEADER_VERSION,
        target_block_time: consensus::TARGET_BLOCK_TIME,
        daa: "ASERT integer fixed-point",
        asert_half_life: consensus::ASERT_HALF_LIFE,
        initial_subsidy: consensus::INITIAL_SUBSIDY,
        minimum_subsidy: consensus::MIN_SUBSIDY,
        halving_interval: consensus::HALVING_INTERVAL,
        finder_share_percent: consensus::FINDER_SHARE_PERCENT,
        reward_window: consensus::REWARD_WINDOW,
        reward_maturity: consensus::REWARD_MATURITY,
        workshare_multiplier: consensus::WORKSHARE_TARGET_MULTIPLIER,
        workshare_target_seconds: consensus::TARGET_BLOCK_TIME
            / consensus::WORKSHARE_TARGET_MULTIPLIER,
        randomx_version: consensus::RANDOMX_VERSION_LINE,
        randomx_epoch_blocks: consensus::RANDOMX_EPOCH_BLOCKS,
        randomx_seed_lag: consensus::RANDOMX_SEED_LAG,
        consensus_minimum_fee: 0,
        relay_minimum_fee: DEFAULT_MIN_RELAY_FEE,
        relay_dust_limit: DEFAULT_DUST_LIMIT,
        maximum_reorg_depth: None,
    })
}

async fn supply(State(node): State<Arc<Node>>) -> Json<serde_json::Value> {
    let status = node.status();
    let (materialized, outstanding, accounts, reward_entries) = node.supply_state();
    let allocated = materialized.saturating_add(outstanding);
    Json(serde_json::json!({
        "circulating": materialized,
        "circulating_supply": materialized,
        "liquid_supply": materialized,
        "emitted_supply": status.emitted_supply,
        "scheduled_supply": status.emitted_supply,
        "materialized_supply": materialized,
        "allocated_supply": allocated,
        "unissued_subsidy": status.emitted_supply.saturating_sub(allocated),
        "reward_credits_outstanding": outstanding,
        "reward_credit_accounts": reward_entries,
        "accounts": accounts,
        "max_supply": null,
        "coin": consensus::COIN,
    }))
}

async fn fee_estimate() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "min_fee": DEFAULT_MIN_RELAY_FEE,
        "recommended_fee": DEFAULT_MIN_RELAY_FEE,
        "dust_limit": DEFAULT_DUST_LIMIT,
        "mempool_pressure": 0.0,
    }))
}

async fn p2p_info(State(node): State<Arc<Node>>) -> Json<serde_json::Value> {
    let snapshot = node.chain_snapshot();
    let status = node.status();
    Json(serde_json::json!({
        "chain_id": consensus::CHAIN_ID,
        "protocol": consensus::PROTOCOL_VERSION,
        "protocol_version": consensus::PROTOCOL_VERSION,
        "node_version": consensus::NODE_VERSION,
        "node_id": node.node_id(),
        "height": snapshot.height,
        "best_hash": snapshot.tip,
        "chain_work": snapshot.cumulative_work,
        "hashrate": status.hashrate,
        "peer_count": status.peer_count,
        "p2p_wire_path": "/p2p/v9",
        "node_url": node.config.public_url,
    }))
}

async fn p2p_whoami() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "chain_id": consensus::CHAIN_ID }))
}

#[allow(clippy::cast_precision_loss)]
async fn mining_distribution(
    State(node): State<Arc<Node>>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let address = query
        .get("address")
        .filter(|value| !value.is_empty())
        .map(|value| Address::from_str(value).map_err(ApiError::bad_request))
        .transpose()?;
    let (total, own, miners) = node.mining_distribution(address);
    let percent = if total == 0 {
        0.0
    } else {
        own as f64 * 100.0 / total as f64
    };
    let pending = address.map_or(0, |value| node.account(value).1);
    let entitlement = address.map_or(0, |value| node.next_workshare_entitlement(value));
    Ok(Json(serde_json::json!({
        "height": node.chain_snapshot().height.saturating_add(1),
        "window": consensus::REWARD_WINDOW,
        "finder_share_percent": consensus::FINDER_SHARE_PERCENT,
        "share_multiplier": consensus::WORKSHARE_TARGET_MULTIPLIER,
        "total_shares": total,
        "active_miners": miners,
        "address_shares": own,
        "address_share_percent": percent,
        "address_next_entitlement": entitlement,
        "address_reward_credit": 0,
        "address_immature_rewards": pending,
        "address_estimated_payout": entitlement,
        "coin": consensus::COIN,
    })))
}

async fn mining_rewards(
    State(node): State<Arc<Node>>,
    Path(address): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let address = Address::from_str(&address).map_err(ApiError::bad_request)?;
    let (total, pending) = node.mining_rewards(address);
    Ok(Json(serde_json::json!({
        "address": address,
        "total_mined": total,
        "immature": pending,
        "pending": pending,
        "coin": consensus::COIN,
    })))
}

async fn block(
    State(node): State<Arc<Node>>,
    Path(selector): Path<String>,
) -> Result<Json<BlockResponse>, ApiError> {
    let block = resolve_block(&node, &selector)?;
    Ok(Json(BlockResponse {
        id: block.id(),
        block,
    }))
}

async fn raw_block(
    State(node): State<Arc<Node>>,
    Path(selector): Path<String>,
) -> Result<Response, ApiError> {
    let bytes = resolve_block(&node, &selector)?.consensus_encode();
    Ok((
        [(header::CONTENT_TYPE, "application/x-paritr-block-v9")],
        bytes,
    )
        .into_response())
}

fn resolve_block(node: &Node, selector: &str) -> Result<Block, ApiError> {
    if let Ok(height) = selector.parse::<u64>() {
        node.block_at(height)
            .ok_or_else(|| ApiError::not_found("block height not found"))
    } else {
        let hash = Hash32::from_str(selector).map_err(ApiError::bad_request)?;
        node.block_by_hash(hash)
            .ok_or_else(|| ApiError::not_found("block hash not found"))
    }
}

async fn address(
    State(node): State<Arc<Node>>,
    Path(address): Path<String>,
) -> Result<Json<AddressResponse>, ApiError> {
    let address = Address::from_str(&address).map_err(ApiError::bad_request)?;
    let (account, pending_rewards, pending_in, pending_out, next_nonce) =
        node.account_view(address);
    Ok(Json(AddressResponse {
        address,
        balance: account.balance,
        nonce: account.nonce,
        pending_rewards,
        confirmed: account.balance,
        pending_in,
        pending_out,
        spendable: account.balance.saturating_sub(pending_out),
        next_nonce,
        immature: pending_rewards,
        reward_credit: 0,
        height: node.chain_snapshot().height,
        coin: consensus::COIN,
    }))
}

async fn address_transactions(
    State(node): State<Arc<Node>>,
    Path(address): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let address = Address::from_str(&address).map_err(ApiError::bad_request)?;
    let limit = query
        .get("limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(25)
        .clamp(1, 100);
    let offset = query
        .get("offset")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    Ok(Json(serde_json::json!({
        "transactions": node.address_history(address, limit, offset),
        "limit": limit,
        "offset": offset,
    })))
}

async fn transaction(
    State(node): State<Arc<Node>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let id = Hash32::from_str(&id).map_err(ApiError::bad_request)?;
    let transaction = node
        .transaction(id)
        .ok_or_else(|| ApiError::not_found("transaction not found"))?;
    Ok(Json(
        serde_json::json!({ "id": id, "transaction": transaction }),
    ))
}

async fn submit_transaction(
    State(node): State<Arc<Node>>,
    Json(transaction): Json<Transaction>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    ensure_node_running(&node)?;
    let id = node
        .submit_transaction(transaction)
        .map_err(ApiError::bad_request)?;
    Ok((
        StatusCode::ACCEPTED,
        Json(serde_json::json!({ "txid": id })),
    ))
}

async fn submit_block(
    State(node): State<Arc<Node>>,
    Json(block): Json<Block>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    ensure_node_running(&node)?;
    let id = block.id();
    let event = node.submit_block(&block).map_err(ApiError::bad_request)?;
    Ok((
        StatusCode::ACCEPTED,
        Json(serde_json::json!({ "block_id": id, "result": format!("{event:?}") })),
    ))
}

async fn submit_raw_block(
    State(node): State<Arc<Node>>,
    bytes: Bytes,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let block = Block::consensus_decode(&bytes).map_err(ApiError::bad_request)?;
    submit_block(State(node), Json(block)).await
}

async fn submit_workshare(
    State(node): State<Arc<Node>>,
    Json(workshare): Json<Workshare>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    ensure_node_running(&node)?;
    let id = workshare.id();
    let preferred = node
        .submit_workshare(workshare)
        .map_err(ApiError::bad_request)?;
    Ok((
        StatusCode::ACCEPTED,
        Json(serde_json::json!({ "workshare_id": id, "preferred_tip": preferred })),
    ))
}

async fn submit_template(
    State(node): State<Arc<Node>>,
    Json(template): Json<BlockTemplate>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    ensure_node_running(&node)?;
    let id = node
        .register_template(template)
        .map_err(ApiError::bad_request)?;
    Ok((
        StatusCode::ACCEPTED,
        Json(serde_json::json!({ "template_id": id })),
    ))
}

async fn mining_template(
    State(node): State<Arc<Node>>,
) -> Result<Json<MiningTemplateResponse>, ApiError> {
    ensure_node_running(&node)?;
    let candidate = node
        .mining_candidate(rand::random())
        .map_err(ApiError::bad_request)?;
    Ok(Json(MiningTemplateResponse {
        block_binary_base64: base64::engine::general_purpose::STANDARD
            .encode(candidate.block.consensus_encode()),
        template_id: candidate.template.id(),
        randomx_seed_hex: hex::encode(candidate.seed),
        block_target_hex: format!("{:064x}", candidate.block_target),
        workshare_target_hex: format!("{:064x}", candidate.workshare_target),
        generation: candidate.generation,
        block: candidate.block,
    }))
}

async fn p2p_socket(State(node): State<Arc<Node>>, upgrade: WebSocketUpgrade) -> Response {
    if !node.is_enabled() {
        return (StatusCode::SERVICE_UNAVAILABLE, "node operation is stopped").into_response();
    }
    upgrade
        .max_message_size(p2p::MAX_WIRE_FRAME)
        .max_frame_size(p2p::MAX_WIRE_FRAME)
        .on_upgrade(move |socket| async move {
            if let Err(error) = p2p::serve_socket(socket, node).await {
                tracing::debug!(%error, "P2P connection closed");
            }
        })
}

fn ensure_node_running(node: &Node) -> Result<(), ApiError> {
    if node.is_enabled() {
        Ok(())
    } else {
        Err(ApiError::unavailable("node operation is stopped"))
    }
}

async fn require_admin(
    State(node): State<Arc<Node>>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    let expected = domain_hash(
        b"PARITR-P9-ADMIN-AUTH-v1",
        node.config.admin_secret.as_bytes(),
    );
    let supplied = token.map(|value| domain_hash(b"PARITR-P9-ADMIN-AUTH-v1", value.as_bytes()));
    if supplied != Some(expected) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(next.run(request).await)
}

async fn admin_auth(State(node): State<Arc<Node>>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "authenticated": true,
        "node_version": consensus::NODE_VERSION,
        "miner_address": node.config.miner_address,
        "mining_enabled": node.config.mining_enabled,
        "node_enabled": node.is_enabled(),
        "public_url": node.config.public_url,
    }))
}

async fn admin_sync(State(node): State<Arc<Node>>) -> Result<Json<serde_json::Value>, ApiError> {
    ensure_node_running(&node)?;
    node.request_sync();
    Ok(Json(serde_json::json!({
        "requested": true,
        "replaced": false,
        "height": node.chain_snapshot().height,
    })))
}

async fn admin_config(State(node): State<Arc<Node>>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "network": node.config.network,
        "public_bind": node.config.public_bind,
        "admin_bind": node.config.admin_bind,
        "management_bind": node.config.management_bind,
        "randomx_mode": node.config.randomx_mode,
        "node_enabled": node.is_enabled(),
        "mining_enabled": node.config.mining_enabled,
        "mining_threads": node.config.mining_threads,
        "mining_intensity": node.config.mining_intensity,
        "miner_address": node.config.miner_address,
        "public_url": node.config.public_url,
        "portal_url": node.config.portal_url,
        "portal_paired": node.config.portal_agent_id.is_some(),
        "device_id": node.config.device_id,
        "device_name": node.config.device_name,
        "restart_policy": "configuration changes restart the supervised node",
    }))
}

async fn admin_config_update(
    State(node): State<Arc<Node>>,
    Json(update): Json<DeviceUpdate>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut config = Config::load_or_create(node.config_path()).map_err(ApiError::bad_request)?;
    if let Some(name) = update.device_name {
        let name = name.trim().to_ascii_lowercase();
        if !valid_device_name(&name) {
            return Err(ApiError::bad_request(
                "device_name must use lowercase letters, digits and interior hyphens",
            ));
        }
        if name != config.device_name
            && !crate::mdns::name_available(&name, &config.device_id)
                .await
                .map_err(ApiError::bad_request)?
        {
            return Err(ApiError::bad_request(
                "device_name is already used by another Paritr node on this LAN",
            ));
        }
        config.device_name = name;
    }
    if let Some(public_url) = update.public_url {
        config.public_url = (!public_url.trim().is_empty()).then(|| public_url.trim().to_owned());
    }
    config.validate().map_err(ApiError::bad_request)?;
    config
        .save(node.config_path())
        .map_err(ApiError::bad_request)?;
    schedule_restart();
    Ok(Json(serde_json::json!({
        "status": "success",
        "device_name": config.device_name,
        "public_url": config.public_url,
        "restart_scheduled": true,
    })))
}

async fn admin_mining(
    State(node): State<Arc<Node>>,
    Json(update): Json<MiningUpdate>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut config = Config::load_or_create(node.config_path()).map_err(ApiError::bad_request)?;
    if let Some(value) = update.miner_address {
        config.miner_address = if value.trim().is_empty() {
            None
        } else {
            Some(Address::from_str(value.trim()).map_err(ApiError::bad_request)?)
        };
    }
    if let Some(value) = update.mining_enabled {
        config.mining_enabled = value;
    }
    if let Some(value) = update.mining_processes {
        config.mining_threads = value;
    }
    if let Some(value) = update.mining_intensity {
        config.mining_intensity = value;
    }
    if let Some(value) = update.randomx_mode {
        if value == MiningMode::Fast && !crate::node::randomx_fast_available() {
            return Err(ApiError::bad_request(
                "RandomX fast mode requires a 64-bit operating system",
            ));
        }
        config.randomx_mode = value;
    }
    config.validate().map_err(ApiError::bad_request)?;
    config
        .save(node.config_path())
        .map_err(ApiError::bad_request)?;
    schedule_restart();
    Ok(Json(serde_json::json!({
        "status": "success",
        "miner_address": config.miner_address,
        "mining_enabled": config.mining_enabled,
        "mining_processes": config.mining_threads,
        "mining_processes_config": config.mining_threads,
        "mining_intensity": config.mining_intensity,
        "randomx_mode": config.randomx_mode,
        "randomx_fast_available": crate::node::randomx_fast_available(),
        "cpu_total": std::thread::available_parallelism().map_or(1, usize::from),
        "restart_scheduled": true,
    })))
}

async fn admin_pair(
    State(node): State<Arc<Node>>,
    Json(request): Json<PairRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let agent_id = crate::portal::pair(node.config_path(), &request.portal_url, &request.code)
        .await
        .map_err(ApiError::bad_request)?;
    schedule_restart();
    Ok(Json(serde_json::json!({
        "status": "success",
        "agent_id": agent_id,
        "restart_scheduled": true,
    })))
}

async fn admin_unpair(State(node): State<Arc<Node>>) -> Result<Json<serde_json::Value>, ApiError> {
    crate::portal::unpair(node.config_path())
        .await
        .map_err(ApiError::bad_request)?;
    schedule_restart();
    Ok(Json(serde_json::json!({
        "status": "success",
        "restart_scheduled": true,
    })))
}

async fn admin_peers(State(node): State<Arc<Node>>) -> Json<serde_json::Value> {
    let status = node.status();
    let mut urls = node.config.seed_nodes.clone();
    for url in &node.config.peers {
        if !urls.contains(url) {
            urls.push(url.clone());
        }
    }
    let peers = urls
        .into_iter()
        .map(|url| {
            serde_json::json!({
                "url": url,
                "failures": 0,
                "last_seen": 0,
                "height": null,
                "hashrate": 0,
            })
        })
        .collect::<Vec<_>>();
    Json(serde_json::json!({
        "peers": peers,
        "connected_peer_ids": node.connected_peer_ids(),
        "active": node.config.peers,
        "seeds": node.config.seed_nodes,
        "node_id": node.node_id(),
        "inbound_count": status.connected_peers,
        "connected_nodes": status.network_nodes,
    }))
}

async fn admin_peer_add(
    State(node): State<Arc<Node>>,
    Json(update): Json<PeerUpdate>,
) -> Result<Json<serde_json::Value>, ApiError> {
    validate_peer_url(&update.url)?;
    let mut config = Config::load_or_create(node.config_path()).map_err(ApiError::bad_request)?;
    if !config.peers.contains(&update.url) {
        config.peers.push(update.url);
    }
    config.validate().map_err(ApiError::bad_request)?;
    config
        .save(node.config_path())
        .map_err(ApiError::bad_request)?;
    schedule_restart();
    Ok(Json(serde_json::json!({
        "status": "success",
        "peers": config.peers,
        "restart_scheduled": true,
    })))
}

async fn admin_peer_remove(
    State(node): State<Arc<Node>>,
    Json(update): Json<PeerUpdate>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut config = Config::load_or_create(node.config_path()).map_err(ApiError::bad_request)?;
    config.peers.retain(|value| value != update.url.trim());
    config.seed_nodes.retain(|value| value != update.url.trim());
    config
        .save(node.config_path())
        .map_err(ApiError::bad_request)?;
    schedule_restart();
    Ok(Json(serde_json::json!({
        "status": "success",
        "peers": config.peers,
        "restart_scheduled": true,
    })))
}

async fn admin_peer_unban(Json(update): Json<PeerUpdate>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "success",
        "unbanned": update.ident,
    }))
}

fn validate_peer_url(value: &str) -> Result<(), ApiError> {
    let url = url::Url::parse(value.trim()).map_err(ApiError::bad_request)?;
    if !matches!(url.scheme(), "https" | "wss" | "http" | "ws")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(ApiError::bad_request("invalid peer URL"));
    }
    Ok(())
}

async fn admin_mempool(State(node): State<Arc<Node>>) -> Json<serde_json::Value> {
    let transactions = node.mempool_transactions();
    Json(serde_json::json!({
        "count": transactions.len(),
        "transactions": transactions,
    }))
}

async fn admin_blocks(
    State(node): State<Arc<Node>>,
    Query(query): Query<HashMap<String, String>>,
) -> Json<serde_json::Value> {
    let limit = query
        .get("limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(25)
        .clamp(1, 100);
    let blocks = node
        .recent_blocks(limit)
        .into_iter()
        .map(|block| {
            let difficulty = consensus::bits_to_target(block.header.bits)
                .map(consensus::target_work)
                .map_or(0.0, crate::node::work_as_f64);
            serde_json::json!({
                "height": block.header.height,
                "hash": block.id(),
                "timestamp": block.header.timestamp,
                "tx_count": block.transactions.len(),
                "difficulty": difficulty,
                "reward": consensus::block_subsidy(block.header.height),
                "block": block,
            })
        })
        .collect::<Vec<_>>();
    Json(serde_json::json!({
        "height": node.chain_snapshot().height,
        "blocks": blocks,
    }))
}

async fn admin_logs(
    State(node): State<Arc<Node>>,
    Query(query): Query<HashMap<String, String>>,
) -> Json<serde_json::Value> {
    let limit = query
        .get("limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(200)
        .clamp(1, 1_000);
    let path = node.config.data_dir.join("node.log");
    let text = tail_log(&path).unwrap_or_default();
    let lines: Vec<_> = text.lines().rev().take(limit).collect();
    Json(serde_json::json!({
        "lines": lines.into_iter().rev().collect::<Vec<_>>(),
    }))
}

async fn state_snapshot(State(node): State<Arc<Node>>) -> Json<serde_json::Value> {
    Json(node.state_snapshot())
}

async fn admin_update() -> Result<Json<serde_json::Value>, ApiError> {
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .https_only(true)
        .build()
        .map_err(ApiError::bad_request)?
        .get("https://paritr.highactive.de/downloads/STABLE")
        .send()
        .await
        .map_err(ApiError::bad_request)?
        .error_for_status()
        .map_err(ApiError::bad_request)?;
    let latest = response.text().await.map_err(ApiError::bad_request)?;
    let latest = latest.trim();
    if latest.is_empty()
        || latest.len() > 64
        || !latest
            .bytes()
            .all(|value| value.is_ascii_alphanumeric() || matches!(value, b'.' | b'-'))
    {
        return Err(ApiError::bad_request(
            "download server returned an invalid stable version",
        ));
    }
    let command = if cfg!(windows) {
        ".\\manage.ps1 update"
    } else {
        "./manage.sh update"
    };
    Ok(Json(serde_json::json!({
        "current": consensus::NODE_VERSION,
        "latest": latest,
        "update_available": latest != consensus::NODE_VERSION,
        "install_command": command,
        "source": "https://paritr.highactive.de/downloads",
    })))
}

fn tail_log(path: &std::path::Path) -> std::io::Result<String> {
    const MAX_BYTES: u64 = 512 * 1024;
    let mut file = std::fs::File::open(path)?;
    let length = file.metadata()?.len();
    file.seek(SeekFrom::Start(length.saturating_sub(MAX_BYTES)))?;
    let mut bytes = Vec::with_capacity(usize::try_from(length.min(MAX_BYTES)).unwrap_or(0));
    file.read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

async fn admin_restart() -> Json<serde_json::Value> {
    schedule_restart();
    Json(serde_json::json!({
        "status": "success",
        "restart_scheduled": true,
    }))
}

async fn admin_start(State(node): State<Arc<Node>>) -> Result<Json<serde_json::Value>, ApiError> {
    set_node_enabled(&node, true)?;
    Ok(Json(serde_json::json!({
        "status": "success",
        "node_enabled": true,
        "restart_scheduled": false,
    })))
}

async fn admin_stop(State(node): State<Arc<Node>>) -> Result<Json<serde_json::Value>, ApiError> {
    set_node_enabled(&node, false)?;
    Ok(Json(serde_json::json!({
        "status": "success",
        "node_enabled": false,
        "restart_scheduled": false,
    })))
}

fn set_node_enabled(node: &Node, enabled: bool) -> Result<(), ApiError> {
    let mut config = Config::load_or_create(node.config_path()).map_err(ApiError::bad_request)?;
    config.node_enabled = enabled;
    config.validate().map_err(ApiError::bad_request)?;
    config
        .save(node.config_path())
        .map_err(ApiError::bad_request)?;
    node.set_enabled(enabled);
    Ok(())
}

fn schedule_restart() {
    tokio::spawn(async {
        // Leave enough time for the HTTP response and an outbound portal result
        // acknowledgement before the service supervisor restarts the process.
        tokio::time::sleep(Duration::from_secs(3)).await;
        std::process::exit(75);
    });
}
