use std::{path::Path, sync::Arc, time::Duration};

use anyhow::{bail, Context};
use reqwest::{Client, Method, Url};
use serde::{Deserialize, Serialize};

use crate::{config::Config, consensus, node::Node};

const MAX_COMMANDS: usize = 20;
const MAX_RESULT_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Deserialize)]
struct PairResponse {
    agent_id: String,
    token: String,
}

#[derive(Debug, Deserialize)]
struct SyncResponse {
    #[serde(default)]
    commands: Vec<PortalCommand>,
}

#[derive(Debug, Deserialize)]
struct PortalCommand {
    id: u64,
    method: String,
    endpoint: String,
    #[serde(default)]
    payload: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct CommandResult {
    id: u64,
    http_status: u16,
    response: serde_json::Value,
}

pub async fn pair(config_path: &Path, portal: &str, code: &str) -> anyhow::Result<String> {
    let mut config = Config::load_or_create(config_path)?;
    let base = validate_portal(portal)?;
    let client = Client::builder()
        .timeout(Duration::from_secs(20))
        .https_only(base.scheme() == "https")
        .build()?;
    let payload = serde_json::json!({
        "code": code.trim().to_uppercase(),
        "device_id": config.device_id,
        "chain_id": consensus::CHAIN_ID,
        "genesis_hash": consensus::Block::genesis().id(),
        "protocol_version": consensus::PROTOCOL_VERSION,
        "node_version": consensus::NODE_VERSION,
        "public_url": config.public_url,
        "platform": std::env::consts::OS,
        "architecture": std::env::consts::ARCH,
    });
    let response = client
        .post(action_url(&base, "node_agent_pair")?)
        .json(&payload)
        .send()
        .await?;
    if !response.status().is_success() {
        let status = response.status();
        let detail = response.text().await.unwrap_or_default();
        bail!(
            "portal pairing failed with HTTP {status}: {}",
            detail.chars().take(512).collect::<String>()
        );
    }
    let paired: PairResponse = response.json().await.context("invalid portal response")?;
    if paired.agent_id.is_empty() || paired.token.len() < 24 {
        bail!("portal returned invalid agent credentials");
    }
    config.portal_url = Some(base.to_string().trim_end_matches('/').to_owned());
    config.portal_agent_id = Some(paired.agent_id.clone());
    config.portal_agent_token = Some(paired.token);
    config.save(config_path)?;
    Ok(paired.agent_id)
}

pub async fn unpair(config_path: &Path) -> anyhow::Result<()> {
    let mut config = Config::load_or_create(config_path)?;
    if let (Some(portal), Some(agent), Some(token)) = (
        config.portal_url.as_deref(),
        config.portal_agent_id.as_deref(),
        config.portal_agent_token.as_deref(),
    ) {
        if let Ok(base) = validate_portal(portal) {
            let client = Client::builder().timeout(Duration::from_secs(10)).build()?;
            if let Ok(url) = action_url(&base, "node_agent_unpair") {
                let _ = client
                    .post(url)
                    .bearer_auth(token)
                    .header("X-Paritr-Agent-ID", agent)
                    .json(&serde_json::json!({ "agent_id": agent }))
                    .send()
                    .await;
            }
        }
    }
    config.portal_agent_id = None;
    config.portal_agent_token = None;
    config.save(config_path)
}

pub fn spawn(node: Arc<Node>) -> Option<tokio::task::JoinHandle<()>> {
    let portal = node.config.portal_url.clone()?;
    let agent = node.config.portal_agent_id.clone()?;
    let token = node.config.portal_agent_token.clone()?;
    Some(tokio::spawn(async move {
        let client = match Client::builder().timeout(Duration::from_secs(20)).build() {
            Ok(client) => client,
            Err(error) => {
                tracing::error!(%error, "cannot start portal agent");
                return;
            }
        };
        let Ok(base) = validate_portal(&portal) else {
            tracing::error!("invalid configured portal URL");
            return;
        };
        let mut results = Vec::new();
        let mut backoff = 2_u64;
        loop {
            match sync_once(&client, &node, &base, &agent, &token, &results).await {
                Ok(commands) => {
                    results.clear();
                    for command in commands.into_iter().take(MAX_COMMANDS) {
                        results.push(execute_command(&client, &node, command).await);
                    }
                    backoff = 2;
                    if results.is_empty() {
                        tokio::time::sleep(Duration::from_secs(2)).await;
                    }
                }
                Err(error) => {
                    tracing::debug!(%error, "portal sync failed");
                    tokio::time::sleep(Duration::from_secs(backoff)).await;
                    backoff = (backoff * 2).min(60);
                }
            }
        }
    }))
}

async fn sync_once(
    client: &Client,
    node: &Node,
    base: &Url,
    agent: &str,
    token: &str,
    results: &[CommandResult],
) -> anyhow::Result<Vec<PortalCommand>> {
    let response = client
        .post(action_url(base, "node_agent_sync")?)
        .bearer_auth(token)
        .header("X-Paritr-Agent-ID", agent)
        .json(&serde_json::json!({
            "agent_id": agent,
            "chain_id": consensus::CHAIN_ID,
            "genesis_hash": consensus::Block::genesis().id(),
            "protocol_version": consensus::PROTOCOL_VERSION,
            "node_version": consensus::NODE_VERSION,
            "public_url": node.config.public_url,
            "status": node.status(),
            "results": results,
        }))
        .send()
        .await?;
    if !response.status().is_success() {
        let status = response.status();
        let detail = response.text().await.unwrap_or_default();
        bail!(
            "portal returned HTTP {status}: {}",
            detail.chars().take(512).collect::<String>()
        );
    }
    Ok(response.json::<SyncResponse>().await?.commands)
}

async fn execute_command(client: &Client, node: &Node, command: PortalCommand) -> CommandResult {
    let method = command.method.to_uppercase();
    if !allowed_endpoint(&method, &command.endpoint) {
        return CommandResult {
            id: command.id,
            http_status: 400,
            response: serde_json::json!({ "error": "relay endpoint is not allowed" }),
        };
    }
    let admin = command.endpoint.starts_with("/admin/") || command.endpoint.starts_with("/state/");
    let bind = if admin {
        &node.config.admin_bind
    } else {
        &node.config.public_bind
    };
    let port = bind
        .parse::<std::net::SocketAddr>()
        .map_or(if admin { 5051 } else { 5050 }, |address| address.port());
    let url = format!("http://127.0.0.1:{port}{}", command.endpoint);
    let request = client.request(
        Method::from_bytes(method.as_bytes()).unwrap_or(Method::GET),
        url,
    );
    let request = if admin {
        request.bearer_auth(&node.config.admin_secret)
    } else {
        request
    };
    let request = if method == "POST" {
        request.json(&command.payload)
    } else {
        request
    };
    match request.send().await {
        Ok(response) => response_to_result(command.id, response).await,
        Err(error) => CommandResult {
            id: command.id,
            http_status: 502,
            response: serde_json::json!({ "error": error.to_string() }),
        },
    }
}

async fn response_to_result(id: u64, response: reqwest::Response) -> CommandResult {
    let status = response.status();
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESULT_BYTES as u64)
    {
        return CommandResult {
            id,
            http_status: 502,
            response: serde_json::json!({ "error": "node response exceeds relay limit" }),
        };
    }
    let bytes = response.bytes().await.unwrap_or_default();
    let value = if bytes.len() > MAX_RESULT_BYTES {
        serde_json::json!({ "error": "node response exceeds relay limit" })
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| serde_json::json!({ "error": "node returned non-JSON data" }))
    };
    CommandResult {
        id,
        http_status: status.as_u16(),
        response: value,
    }
}

fn allowed_endpoint(method: &str, endpoint: &str) -> bool {
    if endpoint.len() > 2_048
        || !endpoint.starts_with('/')
        || endpoint.contains("..")
        || endpoint.contains("://")
        || endpoint.contains('\0')
    {
        return false;
    }
    let path = endpoint.split('?').next().unwrap_or(endpoint);
    match method {
        "GET" => {
            matches!(
                path,
                "/" | "/status"
                    | "/info"
                    | "/health"
                    | "/chain/params"
                    | "/supply"
                    | "/fee/estimate"
                    | "/mining/info"
                    | "/mining/distribution"
                    | "/p2p/info"
                    | "/p2p/whoami"
                    | "/state/snapshot"
                    | "/admin/auth"
                    | "/admin/status"
                    | "/admin/peers"
                    | "/admin/mempool"
                    | "/admin/blocks"
            ) || path.starts_with("/address/")
                || path.starts_with("/block/")
                || path.starts_with("/transaction/")
                || path.starts_with("/mining/rewards/")
        }
        "POST" => {
            matches!(
                path,
                "/transaction"
                    | "/admin/auth"
                    | "/admin/mining"
                    | "/admin/peers/unban"
                    | "/admin/peers/add"
                    | "/admin/peers/remove"
                    | "/admin/sync"
                    | "/admin/start"
                    | "/admin/stop"
                    | "/admin/restart"
            ) && !endpoint.contains('?')
        }
        _ => false,
    }
}

fn validate_portal(value: &str) -> anyhow::Result<Url> {
    let mut url = Url::parse(value)?;
    let loopback = url
        .host_str()
        .is_some_and(|host| matches!(host, "localhost" | "127.0.0.1" | "::1"));
    if url.scheme() != "https" && !(url.scheme() == "http" && loopback) {
        bail!("portal must use HTTPS (HTTP is allowed only on loopback)");
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        bail!("portal URL must not contain credentials, query, or fragment");
    }
    if url.path().ends_with("/api.php") {
        let path = url.path().trim_end_matches("/api.php").to_owned();
        url.set_path(&path);
    }
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    Ok(url)
}

fn action_url(base: &Url, action: &str) -> anyhow::Result<Url> {
    let mut url = base.join("api.php")?;
    url.query_pairs_mut().append_pair("action", action);
    Ok(url)
}
