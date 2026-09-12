use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{bail, Context};
use rand::RngCore;
use serde::{Deserialize, Serialize};

use crate::{consensus::CHAIN_ID, crypto::Address, pow::RandomXMode};

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MiningMode {
    Light,
    Fast,
}

impl From<MiningMode> for RandomXMode {
    fn from(value: MiningMode) -> Self {
        match value {
            MiningMode::Light => Self::Light,
            MiningMode::Fast => Self::Fast {
                initialization_threads: std::thread::available_parallelism().map_or(1, usize::from),
            },
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub network: String,
    pub public_bind: String,
    pub admin_bind: String,
    pub data_dir: PathBuf,
    pub randomx_library: Option<PathBuf>,
    pub randomx_mode: MiningMode,
    pub mining_enabled: bool,
    pub mining_threads: usize,
    pub mining_intensity: u8,
    pub miner_address: Option<Address>,
    pub seed_nodes: Vec<String>,
    pub peers: Vec<String>,
    pub max_inbound_peers: usize,
    pub max_outbound_peers: usize,
    pub public_url: Option<String>,
    pub admin_secret: String,
    pub node_private_key: String,
    pub portal_url: Option<String>,
    pub portal_agent_id: Option<String>,
    pub portal_agent_token: Option<String>,
    pub log_level: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            network: CHAIN_ID.to_owned(),
            public_bind: "0.0.0.0:5050".to_owned(),
            admin_bind: "127.0.0.1:5051".to_owned(),
            data_dir: PathBuf::from("data"),
            randomx_library: None,
            randomx_mode: MiningMode::Light,
            mining_enabled: false,
            mining_threads: 0,
            mining_intensity: 100,
            miner_address: None,
            seed_nodes: vec!["https://node0.oe-net.de".to_owned()],
            peers: Vec::new(),
            max_inbound_peers: 64,
            max_outbound_peers: 16,
            public_url: None,
            admin_secret: random_hex(24),
            node_private_key: valid_secret_hex(),
            portal_url: None,
            portal_agent_id: None,
            portal_agent_token: None,
            log_level: "info".to_owned(),
        }
    }
}

impl Config {
    pub fn load_or_create(path: &Path) -> anyhow::Result<Self> {
        let mut config = if path.exists() {
            let bytes =
                fs::read(path).with_context(|| format!("cannot read config {}", path.display()))?;
            serde_json::from_slice(&bytes)
                .with_context(|| format!("invalid config {}", path.display()))?
        } else {
            Self::default()
        };
        if config.data_dir.is_relative() {
            config.data_dir = path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(&config.data_dir);
        }
        config.validate()?;
        config.save(path)?;
        Ok(config)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if self.network != CHAIN_ID {
            bail!("config network must be {CHAIN_ID}, found {}", self.network);
        }
        self.public_bind
            .parse::<std::net::SocketAddr>()
            .context("public_bind must be an IP socket address")?;
        let admin = self
            .admin_bind
            .parse::<std::net::SocketAddr>()
            .context("admin_bind must be an IP socket address")?;
        if !admin.ip().is_loopback() {
            bail!("admin_bind must remain on loopback; use the outbound portal agent for remote control");
        }
        if !(5..=100).contains(&self.mining_intensity) {
            bail!("mining_intensity must be between 5 and 100");
        }
        if self.mining_threads > 1_024 {
            bail!("mining_threads must be 0 (automatic) or at most 1024");
        }
        if self.max_inbound_peers > 1_024 || self.max_outbound_peers > 256 {
            bail!("peer limits exceed the supported safety bounds");
        }
        if self.seed_nodes.len().saturating_add(self.peers.len()) > 4_096 {
            bail!("too many configured peer URLs");
        }
        if self.mining_enabled && self.miner_address.is_none() {
            bail!("miner_address is required when mining_enabled is true");
        }
        validate_https_url(self.public_url.as_deref(), "public_url")?;
        validate_https_url(self.portal_url.as_deref(), "portal_url")?;
        if hex::decode(&self.admin_secret).map_or(true, |bytes| bytes.len() < 24) {
            bail!("admin_secret must contain at least 24 random bytes as hexadecimal");
        }
        let secret = hex::decode(&self.node_private_key).context("node_private_key must be hex")?;
        secp256k1::SecretKey::from_slice(&secret).context("node_private_key is invalid")?;
        Ok(())
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temporary = path.with_extension("json.tmp");
        let mut file = fs::File::create(&temporary)?;
        file.write_all(&serde_json::to_vec_pretty(self)?)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        set_private_permissions(&temporary)?;
        fs::rename(&temporary, path)?;
        set_private_permissions(path)?;
        Ok(())
    }
}

fn validate_https_url(value: Option<&str>, field: &str) -> anyhow::Result<()> {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    let url = url::Url::parse(value).with_context(|| format!("{field} is not a valid URL"))?;
    let loopback = url
        .host_str()
        .is_some_and(|host| matches!(host, "localhost" | "127.0.0.1" | "::1"));
    if url.scheme() != "https" && !(url.scheme() == "http" && loopback) {
        bail!("{field} must use HTTPS (HTTP is allowed only on loopback)");
    }
    if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
        bail!("{field} must not contain credentials or a fragment");
    }
    Ok(())
}

fn random_hex(bytes: usize) -> String {
    let mut value = vec![0_u8; bytes];
    rand::rngs::OsRng.fill_bytes(&mut value);
    hex::encode(value)
}

fn valid_secret_hex() -> String {
    loop {
        let candidate = random_hex(32);
        if hex::decode(&candidate)
            .ok()
            .and_then(|bytes| secp256k1::SecretKey::from_slice(&bytes).ok())
            .is_some()
        {
            return candidate;
        }
    }
}

#[cfg(unix)]
fn set_private_permissions(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
#[allow(clippy::unnecessary_wraps)]
fn set_private_permissions(_path: &Path) -> std::io::Result<()> {
    // The Windows installer applies a user-only ACL to the installation folder.
    Ok(())
}
