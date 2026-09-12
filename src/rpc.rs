use std::{net::SocketAddr, str::FromStr, sync::Arc};

use axum::{
    body::{Body, Bytes},
    extract::{Path, Request, State, WebSocketUpgrade},
    http::{header, HeaderValue, Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use base64::Engine;
use serde::Serialize;
use tower_http::{cors::CorsLayer, limit::RequestBodyLimitLayer, trace::TraceLayer};

use crate::{
    codec::{ConsensusDecode, ConsensusEncode},
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
    protocol: u16,
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
    Router::new()
        .route("/health", get(health))
        .route("/status", get(status))
        .route("/chain/params", get(chain_params))
        .route("/block/{selector}", get(block))
        .route("/block/{selector}/raw", get(raw_block))
        .route("/address/{address}", get(address))
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
        .with_state(node)
}

pub fn admin_router(node: Arc<Node>) -> Router {
    Router::new()
        .route("/admin/auth", get(admin_auth))
        .route("/admin/status", get(status))
        .route("/admin/sync", post(admin_sync))
        .layer(middleware::from_fn_with_state(
            Arc::clone(&node),
            require_admin,
        ))
        .layer(RequestBodyLimitLayer::new(64 * 1024))
        .layer(TraceLayer::new_for_http())
        .with_state(node)
}

pub async fn serve(node: Arc<Node>) -> anyhow::Result<()> {
    let public_address: SocketAddr = node.config.public_bind.parse()?;
    let admin_address: SocketAddr = node.config.admin_bind.parse()?;
    if public_address == admin_address {
        anyhow::bail!("public and admin listeners must be different");
    }
    let public_listener = tokio::net::TcpListener::bind(public_address).await?;
    let admin_listener = tokio::net::TcpListener::bind(admin_address).await?;
    tracing::info!(%public_address, "public RPC/P2P listening");
    tracing::info!(%admin_address, "loopback admin API listening");
    tokio::try_join!(
        axum::serve(public_listener, public_router(Arc::clone(&node))),
        axum::serve(admin_listener, admin_router(node)),
    )?;
    Ok(())
}

async fn health() -> Json<Health> {
    Json(Health {
        status: "ok",
        protocol: consensus::PROTOCOL_VERSION,
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
    let (account, pending_rewards) = node.account(address);
    Ok(Json(AddressResponse {
        address,
        balance: account.balance,
        nonce: account.nonce,
        pending_rewards,
    }))
}

async fn submit_transaction(
    State(node): State<Arc<Node>>,
    Json(transaction): Json<Transaction>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
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
    upgrade
        .max_message_size(p2p::MAX_WIRE_FRAME)
        .max_frame_size(p2p::MAX_WIRE_FRAME)
        .on_upgrade(move |socket| async move {
            if let Err(error) = p2p::serve_socket(socket, node).await {
                tracing::debug!(%error, "P2P connection closed");
            }
        })
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

async fn admin_auth() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "authenticated": true }))
}

async fn admin_sync(State(node): State<Arc<Node>>) -> Json<serde_json::Value> {
    node.request_sync();
    Json(serde_json::json!({
        "requested": true,
        "height": node.chain_snapshot().height,
    }))
}
