use std::{path::PathBuf, str::FromStr, sync::Arc};

use anyhow::Context;
use base64::Engine;
use clap::{Parser, Subcommand, ValueEnum};
use paritr::{
    codec::ConsensusEncode,
    config::{Config, MiningMode},
    consensus::{Block, NODE_VERSION, PROTOCOL_VERSION},
    crypto::Address,
    miner::MiningController,
    node::Node,
    pow::RandomX,
};

#[derive(Parser, Debug)]
#[command(name = "paritr-node", version = NODE_VERSION, about = "Paritr Protocol 9 full node")]
struct Cli {
    #[arg(long, default_value = "config.json", global = true)]
    config: PathBuf,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Run the full node (default command).
    Run,
    /// Create or update a local runtime configuration.
    Init {
        #[arg(long)]
        miner_address: Option<String>,
        #[arg(long)]
        enable_mining: bool,
        #[arg(long, default_value_t = 0)]
        mining_threads: usize,
        #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(u8).range(5..=100))]
        mining_intensity: u8,
        #[arg(long, value_enum, default_value_t = ModeArg::Light)]
        randomx_mode: ModeArg,
        #[arg(long)]
        public_bind: Option<String>,
        #[arg(long)]
        admin_bind: Option<String>,
        #[arg(long)]
        management_bind: Option<String>,
        #[arg(long)]
        public_url: Option<String>,
    },
    /// Pair the outbound-only portal agent.
    Pair {
        #[arg(long)]
        portal_url: String,
        #[arg(long)]
        code: String,
    },
    /// Remove portal credentials and notify the portal when reachable.
    Unpair,
    /// Validate configuration, `RandomX`, database, chain and chainstate.
    Check,
    /// Print configuration with all locally stored secrets redacted.
    ShowConfig,
    /// Print the local first-run URL and admin secret.
    AdminAccess,
    /// Print the immutable Protocol 9 genesis and consensus bytes.
    PrintGenesis,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ModeArg {
    Light,
    Fast,
}

#[tokio::main]
#[allow(clippy::too_many_lines)]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command.unwrap_or(Command::Run) {
        Command::Run => run(cli.config).await,
        Command::Init {
            miner_address,
            enable_mining,
            mining_threads,
            mining_intensity,
            randomx_mode,
            public_bind,
            admin_bind,
            management_bind,
            public_url,
        } => {
            let mut config = Config::load_or_create(&cli.config)?;
            if let Some(address) = miner_address {
                config.miner_address = Some(Address::from_str(&address)?);
            }
            config.mining_enabled = enable_mining;
            config.mining_threads = mining_threads;
            config.mining_intensity = mining_intensity;
            config.randomx_mode = match randomx_mode {
                ModeArg::Light => MiningMode::Light,
                ModeArg::Fast => MiningMode::Fast,
            };
            if let Some(bind) = public_bind {
                config.public_bind = bind;
            }
            if let Some(bind) = admin_bind {
                config.admin_bind = bind;
            }
            if let Some(bind) = management_bind {
                config.management_bind = bind;
            }
            if let Some(url) = public_url {
                config.public_url = (!url.is_empty()).then_some(url);
            }
            config.validate()?;
            config.save(&cli.config)?;
            println!("configuration written to {}", cli.config.display());
            Ok(())
        }
        Command::Pair { portal_url, code } => {
            let agent = paritr::portal::pair(&cli.config, &portal_url, &code).await?;
            println!("portal pairing active: {agent}");
            Ok(())
        }
        Command::Unpair => {
            paritr::portal::unpair(&cli.config).await?;
            println!("portal pairing removed");
            Ok(())
        }
        Command::Check => {
            let config = Config::load_or_create(&cli.config)?;
            let _logging = init_logging(&config.log_level, &config.data_dir);
            let randomx = Arc::new(RandomX::load(
                config.randomx_library.as_deref(),
                config.randomx_mode.into(),
            )?);
            let node = Node::open(config, cli.config.clone(), randomx)?;
            println!(
                "OK: Protocol {}, height {}, tip {}",
                PROTOCOL_VERSION,
                node.chain_snapshot().height,
                node.chain_snapshot().tip
            );
            Ok(())
        }
        Command::ShowConfig => {
            let config = Config::load_or_create(&cli.config)?;
            let mut value = serde_json::to_value(config)?;
            if let Some(object) = value.as_object_mut() {
                for field in ["admin_secret", "node_private_key", "portal_agent_token"] {
                    if object.get(field).is_some_and(|entry| !entry.is_null()) {
                        object.insert(field.to_owned(), serde_json::json!("<redacted>"));
                    }
                }
            }
            println!("{}", serde_json::to_string_pretty(&value)?);
            Ok(())
        }
        Command::AdminAccess => {
            let config = Config::load_or_create(&cli.config)?;
            let port = config
                .management_bind
                .parse::<std::net::SocketAddr>()?
                .port();
            println!("Management URL: http://{}.local:{port}", config.device_name);
            println!("Local fallback: http://127.0.0.1:{port}");
            println!("Admin secret: {}", config.admin_secret);
            println!("Keep this secret private; no wallet key or seed phrase is needed.");
            Ok(())
        }
        Command::PrintGenesis => {
            let genesis = Block::genesis();
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "id": genesis.id(),
                    "block": genesis,
                    "consensus_base64": base64::engine::general_purpose::STANDARD.encode(genesis.consensus_encode()),
                }))?
            );
            Ok(())
        }
    }
}

async fn run(config_path: PathBuf) -> anyhow::Result<()> {
    let config = Config::load_or_create(&config_path)?;
    let _logging = init_logging(&config.log_level, &config.data_dir);
    tracing::info!(
        version = NODE_VERSION,
        protocol = PROTOCOL_VERSION,
        "starting Paritr node"
    );
    let randomx = Arc::new(
        RandomX::load(
            config.randomx_library.as_deref(),
            config.randomx_mode.into(),
        )
        .context("RandomX v1.2.3 initialization failed")?,
    );
    let node = Node::open(config, config_path, randomx)?;
    let peers = paritr::p2p::spawn_outbound_manager(&node);
    let portal = paritr::portal::spawn(Arc::clone(&node));
    let miner = MiningController::start(&node);
    let _mdns = match paritr::mdns::register(&node.config) {
        Ok(service) => Some(service),
        Err(error) => {
            tracing::warn!(%error, "mDNS registration unavailable");
            None
        }
    };
    let management_port = node
        .config
        .management_bind
        .parse::<std::net::SocketAddr>()?
        .port();
    tracing::info!(
        url = %format!("http://{}.local:{management_port}", node.config.device_name),
        "local management page available"
    );

    tokio::select! {
        result = paritr::rpc::serve(node) => result?,
        result = tokio::signal::ctrl_c() => result?,
    }
    for task in peers {
        task.abort();
    }
    if let Some(task) = portal {
        task.abort();
    }
    if let Some(miner) = miner {
        miner.stop();
    }
    Ok(())
}

fn init_logging(
    level: &str,
    data_dir: &std::path::Path,
) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    use tracing_subscriber::fmt::writer::MakeWriterExt;

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(level));
    let _ = std::fs::create_dir_all(data_dir);
    let appender = tracing_appender::rolling::never(data_dir, "node.log");
    let (file_writer, guard) = tracing_appender::non_blocking(appender);
    let writer = std::io::stdout.and(file_writer);
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .with_ansi(false)
        .try_init()
        .ok()?;
    Some(guard)
}
