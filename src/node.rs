use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Context;
use base64::Engine;
use parking_lot::{Mutex, RwLock};
use primitive_types::U256;
use secp256k1::{PublicKey, Secp256k1, SecretKey};
use serde::Serialize;
use tokio::sync::broadcast;

use crate::{
    codec::ConsensusEncode,
    config::Config,
    consensus::{
        self, reward_allocation, Block, BlockHeader, BlockTemplate, Chain, ChainEvent,
        ConsensusError, RewardClaim, SystemTimeSource, Transaction, Workshare, WorkshareWitness,
        MAX_BLOCK_TRANSACTIONS, MAX_MONEY,
    },
    crypto::{domain_hash, Address, Hash32},
    mempool::{Mempool, MempoolError},
    miner::MiningStats,
    p2p::{Hello, ObjectKind, WireMessage},
    policy::RelayPolicy,
    pow::RandomX,
    storage::Storage,
};

#[derive(Clone, Debug, Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct NodeStatus {
    pub status: &'static str,
    pub node_version: &'static str,
    pub protocol_version: u16,
    pub chain_id: &'static str,
    pub genesis_hash: Hash32,
    pub height: u64,
    pub best_block_hash: Hash32,
    pub state_root: Hash32,
    pub cumulative_work: String,
    pub chain_work: String,
    pub difficulty_bits: String,
    pub bits: String,
    pub tip_bits: String,
    pub next_bits: String,
    pub target_block_time: u64,
    pub workshare_target_seconds: u64,
    pub reward_window: u64,
    pub finder_share_percent: u64,
    pub reward_maturity: u64,
    pub shares_in_window: usize,
    pub mempool_transactions: usize,
    pub mempool_size: usize,
    pub mempool_bytes: usize,
    pub live_workshares: usize,
    pub connected_peers: usize,
    pub peer_count: usize,
    pub network_nodes: usize,
    pub active_miners: usize,
    pub active_miners_scope: &'static str,
    pub node_enabled: bool,
    pub mining_enabled: bool,
    pub mining_active: bool,
    pub miner_address: Option<Address>,
    pub mining_processes: usize,
    pub mining_processes_config: usize,
    pub mining_intensity: u8,
    pub cpu_total: usize,
    pub randomx_mode: crate::config::MiningMode,
    pub randomx_fast_available: bool,
    pub hashrate: f64,
    pub network_hashrate: f64,
    pub estimated_hashrate: f64,
    pub node_shares_submitted: u64,
    pub blocks_found: u64,
    pub blocks_found_local: u64,
    pub difficulty: f64,
    pub tip_difficulty: f64,
    pub avg_block_time: f64,
    pub last_block_time: u64,
    pub circulating_supply: u64,
    pub next_block_reward: u64,
    pub min_subsidy: u64,
    pub halving_interval: u64,
    pub blocks_until_halving: u64,
    pub annual_inflation: f64,
    pub tail_emission: bool,
    pub coin: u64,
    pub share_multiplier: u64,
    pub platform: String,
    pub device_id: String,
    pub device_name: String,
    pub management_url: String,
    pub uptime_seconds: u64,
    pub emitted_supply: u64,
}

#[derive(Clone, Debug)]
pub struct MiningCandidate {
    pub block: Block,
    pub template: BlockTemplate,
    pub seed: Vec<u8>,
    pub block_target: U256,
    pub workshare_target: U256,
    pub generation: u64,
}

#[derive(Default)]
struct WorksharePool {
    anchor: Hash32,
    templates: BTreeMap<Hash32, BlockTemplate>,
    template_bytes: usize,
    shares: HashMap<Hash32, Workshare>,
    path_work: HashMap<Hash32, U256>,
    tip: Hash32,
}

impl WorksharePool {
    fn reset(&mut self, anchor: Hash32) {
        self.anchor = anchor;
        self.templates.clear();
        self.template_bytes = 0;
        self.shares.clear();
        self.path_work.clear();
        self.tip = Hash32::ZERO;
    }

    fn register_template(&mut self, template: BlockTemplate) -> Result<Hash32, ConsensusError> {
        if template.parent != self.anchor
            || template.version != BlockTemplate::VERSION
            || template.consensus_encode().len() > consensus::MAX_TEMPLATE_BYTES
        {
            return Err(ConsensusError::InvalidWorkshareChain);
        }
        let id = template.id();
        if self.templates.contains_key(&id) {
            return Ok(id);
        }
        let encoded_size = template.consensus_encode().len();
        if self.templates.len() >= consensus::MAX_TEMPLATES_PER_BLOCK
            || self.template_bytes.saturating_add(encoded_size) > 64 * 1024 * 1024
        {
            return Err(ConsensusError::SizeLimit);
        }
        self.template_bytes += encoded_size;
        self.templates.insert(id, template);
        Ok(id)
    }

    fn candidate_witness(&self, share: Workshare) -> Result<WorkshareWitness, ConsensusError> {
        if share.candidate_header.previous_block != self.anchor
            || !self.templates.contains_key(&share.template_id)
        {
            return Err(ConsensusError::InvalidWorkshareChain);
        }
        let mut reverse = vec![share];
        let mut seen = HashSet::new();
        while reverse
            .last()
            .expect("share path is nonempty")
            .previous_workshare
            != Hash32::ZERO
        {
            let previous = reverse
                .last()
                .expect("share path is nonempty")
                .previous_workshare;
            if !seen.insert(previous) || reverse.len() >= consensus::MAX_WORKSHARES_PER_BLOCK {
                return Err(ConsensusError::InvalidWorkshareChain);
            }
            reverse.push(
                self.shares
                    .get(&previous)
                    .cloned()
                    .ok_or(ConsensusError::InvalidWorkshareChain)?,
            );
        }
        reverse.reverse();
        let referenced: HashSet<_> = reverse.iter().map(|item| item.template_id).collect();
        let templates = self
            .templates
            .iter()
            .filter_map(|(id, template)| referenced.contains(id).then_some(template.clone()))
            .collect();
        Ok(WorkshareWitness {
            version: WorkshareWitness::VERSION,
            templates,
            workshares: reverse,
        })
    }

    fn accept(&mut self, share: Workshare) -> Result<bool, ConsensusError> {
        let id = share.id();
        if self.shares.contains_key(&id) {
            return Ok(false);
        }
        if self.shares.len() >= 8_192 {
            return Err(ConsensusError::SizeLimit);
        }
        let parent_work = if share.previous_workshare == Hash32::ZERO {
            U256::zero()
        } else {
            *self
                .path_work
                .get(&share.previous_workshare)
                .ok_or(ConsensusError::InvalidWorkshareChain)?
        };
        let weight = consensus::rewards::workshare_weight(share.candidate_header.bits)?;
        let cumulative = parent_work
            .checked_add(weight)
            .ok_or(ConsensusError::ArithmeticOverflow)?;
        self.shares.insert(id, share);
        self.path_work.insert(id, cumulative);
        let tip_work = self.path_work.get(&self.tip).copied().unwrap_or_default();
        if cumulative > tip_work || (cumulative == tip_work && id < self.tip) {
            self.tip = id;
            return Ok(true);
        }
        Ok(false)
    }

    fn witness(&self) -> WorkshareWitness {
        if self.tip == Hash32::ZERO {
            WorkshareWitness::empty()
        } else {
            self.candidate_witness(self.shares[&self.tip].clone())
                .expect("accepted workshare path remains available")
        }
    }
}

pub struct Node {
    pub config: Config,
    runtime_mining: RwLock<Config>,
    config_path: PathBuf,
    storage: Arc<Storage>,
    pow: Arc<RandomX>,
    chain: RwLock<Chain>,
    block_processing: Mutex<()>,
    discovered_peers: Mutex<BTreeSet<String>>,
    mempool: Mutex<Mempool>,
    workshares: Mutex<WorksharePool>,
    identity_secret: SecretKey,
    identity_id: Hash32,
    connected_peers: Mutex<HashMap<Hash32, bool>>,
    sync_queues: Mutex<HashMap<Hash32, VecDeque<Hash32>>>,
    pending_workshares: Mutex<HashMap<Hash32, (Hash32, Workshare)>>,
    events: broadcast::Sender<WireMessage>,
    generation: AtomicU64,
    mining_stats: Arc<MiningStats>,
    #[allow(clippy::struct_field_names)]
    node_enabled: AtomicBool,
    started_at: u64,
}

impl Node {
    pub fn open(
        config: Config,
        config_path: PathBuf,
        pow: Arc<RandomX>,
    ) -> anyhow::Result<Arc<Self>> {
        std::fs::create_dir_all(&config.data_dir)?;
        let database = config
            .data_dir
            .join(format!("chain-{}.redb", Block::genesis().id()));
        let storage = Arc::new(Storage::open(&database)?);
        let stored = storage.load_active_blocks()?;
        let chain = if stored.is_empty() {
            let chain = Chain::genesis();
            storage.save_chain(&chain)?;
            chain
        } else {
            Chain::from_blocks(stored, pow.as_ref(), &SystemTimeSource)
                .context("stored active chain failed full validation")?
        };
        let secret_bytes = hex::decode(&config.node_private_key)?;
        let identity_secret = SecretKey::from_slice(&secret_bytes)?;
        let public = PublicKey::from_secret_key(&Secp256k1::new(), &identity_secret);
        let identity_id = domain_hash(b"PARITR-P9-NODE-ID-v1", &public.serialize());
        let (events, _) = broadcast::channel(2_048);
        let mut pool = WorksharePool::default();
        pool.reset(chain.tip().id());
        let node_enabled = config.node_enabled;
        Ok(Arc::new(Self {
            runtime_mining: RwLock::new(config.clone()),
            config,
            config_path,
            storage,
            pow,
            chain: RwLock::new(chain),
            block_processing: Mutex::new(()),
            discovered_peers: Mutex::new(BTreeSet::new()),
            mempool: Mutex::new(Mempool::new(RelayPolicy::default())),
            workshares: Mutex::new(pool),
            identity_secret,
            identity_id,
            connected_peers: Mutex::new(HashMap::new()),
            sync_queues: Mutex::new(HashMap::new()),
            pending_workshares: Mutex::new(HashMap::new()),
            events,
            generation: AtomicU64::new(0),
            node_enabled: AtomicBool::new(node_enabled),
            mining_stats: Arc::new(MiningStats::default()),
            started_at: unix_time(),
        }))
    }

    pub fn node_id(&self) -> Hash32 {
        self.identity_id
    }

    pub fn mining_config(&self) -> Config {
        self.runtime_mining.read().clone()
    }

    pub fn apply_mining_config(&self, config: Config) {
        *self.runtime_mining.write() = config;
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    pub fn chain_snapshot(&self) -> consensus::ChainSnapshot {
        self.chain.read().snapshot()
    }

    #[allow(clippy::cast_precision_loss, clippy::too_many_lines)]
    pub fn status(&self) -> NodeStatus {
        let config = self.mining_config();
        let live_workshares = self.workshares.lock().witness().workshares;
        let chain = self.chain.read();
        let mempool = self.mempool.lock();
        let tip = chain.tip();
        let peer_count = self.connected_peers.lock().len();
        let cpu_total = std::thread::available_parallelism().map_or(1, usize::from);
        let mining_processes = if config.mining_threads == 0 {
            cpu_total
        } else {
            config.mining_threads.min(cpu_total)
        };
        let hashrate = self.mining_stats.hashrate();
        let window_start = chain
            .blocks()
            .len()
            .saturating_sub(usize::try_from(consensus::REWARD_WINDOW).expect("window fits usize"));
        let settled_workshares = chain.blocks()[window_start..]
            .iter()
            .flat_map(|block| block.workshare_witness.workshares.iter());
        let active_miners = peer_count + usize::from(self.is_enabled());
        let shares_in_window = settled_workshares.count() + live_workshares.len();

        let blocks = chain.blocks();
        let mined_blocks: Vec<_> = blocks.iter().skip(1).rev().take(16).cloned().collect();
        let avg_block_time = if mined_blocks.len() > 1 {
            let newest = mined_blocks.first().expect("nonempty").header.timestamp;
            let oldest = mined_blocks.last().expect("nonempty").header.timestamp;
            let span = newest.saturating_sub(oldest);
            (span as f64) / ((mined_blocks.len() - 1) as f64).max(1.0)
        } else {
            consensus::TARGET_BLOCK_TIME as f64
        };

        let estimated_hashrate = if mined_blocks.len() > 1 {
            let newest = mined_blocks.first().expect("nonempty").header.timestamp;
            let oldest = mined_blocks.last().expect("nonempty").header.timestamp;
            let span = newest.saturating_sub(oldest);
            if span == 0 {
                0.0
            } else {
                mined_blocks
                    .iter()
                    .filter_map(|b| consensus::bits_to_target(b.header.bits))
                    .map(consensus::target_work)
                    .map(work_as_f64)
                    .sum::<f64>()
                    / (span as f64)
            }
        } else {
            0.0
        };

        let implied_hashrate = consensus::bits_to_target(tip.header.bits).map_or(0.0, |target| {
            work_as_f64(consensus::target_work(target)) / consensus::TARGET_BLOCK_TIME as f64
        });

        let network_hashrate = if estimated_hashrate > 0.0 {
            estimated_hashrate.max(hashrate)
        } else {
            implied_hashrate.max(hashrate)
        };

        let emitted_supply = scheduled_supply(chain.height());
        let circulating_supply = chain
            .state()
            .accounts()
            .values()
            .map(|account| account.balance)
            .fold(0_u64, u64::saturating_add);
        let next_height = chain.height().saturating_add(1);
        let next_block_reward = consensus::block_subsidy(next_height);
        let blocks_until_halving = consensus::HALVING_INTERVAL
            - next_height.saturating_sub(1) % consensus::HALVING_INTERVAL;
        let annual_inflation = if circulating_supply == 0 {
            0.0
        } else {
            let blocks_per_year = 365 * 24 * 60 * 60 / consensus::TARGET_BLOCK_TIME;
            blocks_per_year as f64 * next_block_reward as f64 / circulating_supply as f64 * 100.0
        };
        let blocks_found = config.miner_address.map_or(0, |address| {
            u64::try_from(
                chain
                    .blocks()
                    .iter()
                    .filter_map(|block| block.reward_claim.as_ref())
                    .filter(|claim| claim.finder == address)
                    .count(),
            )
            .unwrap_or(u64::MAX)
        });
        let cumulative_work = format!("{:064x}", chain.cumulative_work());
        let next_bits = consensus::next_bits(chain.blocks());
        let difficulty = displayed_difficulty(next_bits);
        let tip_difficulty = displayed_difficulty(tip.header.bits);
        NodeStatus {
            status: if self.is_enabled() {
                "online"
            } else {
                "stopped"
            },
            node_version: consensus::NODE_VERSION,
            protocol_version: consensus::PROTOCOL_VERSION,
            chain_id: consensus::CHAIN_ID,
            genesis_hash: Block::genesis().id(),
            height: chain.height(),
            best_block_hash: tip.id(),
            state_root: chain.state().root(),
            cumulative_work: cumulative_work.clone(),
            chain_work: cumulative_work,
            difficulty_bits: format!("{:08x}", tip.header.bits),
            bits: format!("{next_bits:08x}"),
            tip_bits: format!("{:08x}", tip.header.bits),
            next_bits: format!("{next_bits:08x}"),
            target_block_time: consensus::TARGET_BLOCK_TIME,
            workshare_target_seconds: consensus::TARGET_BLOCK_TIME
                / consensus::WORKSHARE_TARGET_MULTIPLIER,
            reward_window: consensus::REWARD_WINDOW,
            finder_share_percent: consensus::FINDER_SHARE_PERCENT,
            reward_maturity: consensus::REWARD_MATURITY,
            shares_in_window,
            mempool_transactions: mempool.len(),
            mempool_size: mempool.len(),
            mempool_bytes: mempool.bytes(),
            live_workshares: live_workshares.len(),
            connected_peers: peer_count,
            peer_count,
            network_nodes: peer_count.saturating_add(1),
            active_miners,
            active_miners_scope: "connected_node_identities_including_self",
            node_enabled: self.is_enabled(),
            mining_enabled: config.mining_enabled,
            mining_active: self.is_enabled() && config.mining_enabled && hashrate > 0.0,
            miner_address: config.miner_address,
            mining_processes,
            mining_processes_config: config.mining_threads,
            mining_intensity: config.mining_intensity,
            cpu_total,
            randomx_mode: config.randomx_mode,
            randomx_fast_available: randomx_fast_available(),
            hashrate,
            network_hashrate,
            estimated_hashrate,
            node_shares_submitted: self.mining_stats.workshares(),
            blocks_found,
            blocks_found_local: self.mining_stats.blocks(),
            difficulty,
            tip_difficulty,
            avg_block_time,
            last_block_time: tip.header.timestamp,
            circulating_supply,
            next_block_reward,
            min_subsidy: consensus::MIN_SUBSIDY,
            halving_interval: consensus::HALVING_INTERVAL,
            blocks_until_halving,
            annual_inflation,
            tail_emission: consensus::MIN_SUBSIDY > 0,
            coin: consensus::COIN,
            share_multiplier: consensus::WORKSHARE_TARGET_MULTIPLIER,
            platform: format!("{}/{}", std::env::consts::OS, std::env::consts::ARCH),
            device_id: self.config.device_id.clone(),
            device_name: self.config.device_name.clone(),
            management_url: format!(
                "http://{}.local:{}",
                self.config.device_name,
                self.config
                    .management_bind
                    .parse::<std::net::SocketAddr>()
                    .map_or(5051, |address| address.port())
            ),
            uptime_seconds: unix_time().saturating_sub(self.started_at),
            emitted_supply,
        }
    }

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    pub fn is_enabled(&self) -> bool {
        self.node_enabled.load(Ordering::Acquire)
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.node_enabled.store(enabled, Ordering::Release);
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    pub fn mining_stats(&self) -> Arc<MiningStats> {
        Arc::clone(&self.mining_stats)
    }

    pub fn recent_blocks(&self, limit: usize) -> Vec<Block> {
        let chain = self.chain.read();
        chain
            .blocks()
            .iter()
            .rev()
            .take(limit.min(100))
            .cloned()
            .collect()
    }

    pub fn mempool_transactions(&self) -> Vec<Transaction> {
        let state = self.chain.read().state().clone();
        self.mempool
            .lock()
            .select(&state, consensus::MAX_BLOCK_TRANSACTIONS)
    }

    pub fn transaction(&self, id: Hash32) -> Option<Transaction> {
        let pending = self.mempool.lock().get(id);
        pending.or_else(|| {
            self.chain
                .read()
                .blocks()
                .iter()
                .flat_map(|block| block.transactions.iter())
                .find(|transaction| transaction.id() == id)
                .cloned()
        })
    }

    pub fn address_history(
        &self,
        address: Address,
        limit: usize,
        offset: usize,
    ) -> Vec<serde_json::Value> {
        let chain = self.chain.read();
        let height = chain.height();
        let mut entries = Vec::new();
        for transaction in self
            .mempool
            .lock()
            .select(chain.state(), consensus::MAX_BLOCK_TRANSACTIONS)
            .into_iter()
            .rev()
        {
            let (direction, counterparty) = if transaction.sender == address {
                ("out", transaction.recipient)
            } else if transaction.recipient == address {
                ("in", transaction.sender)
            } else {
                continue;
            };
            entries.push(serde_json::json!({
                "txid": transaction.id(),
                "direction": direction,
                "counterparty": counterparty,
                "amount": transaction.amount,
                "fee": if direction == "out" { transaction.fee } else { 0 },
                "timestamp": unix_time(),
                "height": null,
                "confirmations": 0,
                "status": "pending",
                "matured": true,
            }));
        }
        let share_blocks: Vec<_> = chain
            .blocks()
            .iter()
            .enumerate()
            .filter_map(|(index, block)| {
                block
                    .workshare_witness
                    .workshares
                    .iter()
                    .any(|share| share.miner == address)
                    .then_some(index)
            })
            .collect();
        for (index, block) in chain.blocks().iter().enumerate().rev() {
            if entries.len() >= offset.saturating_add(limit.min(100)) {
                break;
            }
            if let Some(reward) = historical_reward(chain.blocks(), index, address, &share_blocks) {
                entries.push(reward);
            }
            for transaction in block.transactions.iter().rev() {
                let (direction, counterparty) = if transaction.sender == address {
                    ("out", transaction.recipient)
                } else if transaction.recipient == address {
                    ("in", transaction.sender)
                } else {
                    continue;
                };
                entries.push(serde_json::json!({
                    "txid": transaction.id(),
                    "direction": direction,
                    "counterparty": counterparty,
                    "amount": transaction.amount,
                    "fee": if direction == "out" { transaction.fee } else { 0 },
                    "timestamp": block.header.timestamp,
                    "height": block.header.height,
                    "confirmations": height.saturating_sub(block.header.height).saturating_add(1),
                    "status": "confirmed",
                    "matured": true,
                }));
            }
        }
        entries
            .into_iter()
            .skip(offset)
            .take(limit.min(100))
            .collect()
    }

    pub fn connected_peer_ids(&self) -> Vec<String> {
        self.connected_peers
            .lock()
            .keys()
            .map(ToString::to_string)
            .collect()
    }

    pub fn supply_state(&self) -> (u64, u64, usize, usize) {
        let chain = self.chain.read();
        let materialized = chain
            .state()
            .accounts()
            .values()
            .map(|account| account.balance)
            .fold(0_u64, u64::saturating_add);
        let outstanding = chain
            .state()
            .pending_rewards()
            .values()
            .copied()
            .fold(0_u64, u64::saturating_add);
        (
            materialized,
            outstanding,
            chain.state().accounts().len(),
            chain.state().pending_rewards().len(),
        )
    }

    pub fn state_snapshot(&self) -> serde_json::Value {
        let chain = self.chain.read();
        let bytes = chain.state().consensus_encode();
        serde_json::json!({
            "format": "paritr-ledger-state-v1",
            "snapshot_version": consensus::LedgerState::SNAPSHOT_VERSION,
            "height": chain.height(),
            "block_hash": chain.tip().id(),
            "state_root": chain.state().root(),
            "consensus_base64": base64::engine::general_purpose::STANDARD.encode(bytes),
        })
    }

    pub fn mining_distribution(&self, address: Option<Address>) -> (usize, usize, usize) {
        let chain = self.chain.read();
        let start = chain
            .blocks()
            .len()
            .saturating_sub(usize::try_from(consensus::REWARD_WINDOW).expect("window fits usize"));
        let live = self.workshares.lock().witness();
        let shares = chain.blocks()[start..]
            .iter()
            .flat_map(|block| block.workshare_witness.workshares.iter())
            .chain(live.workshares.iter());
        let miners = self.connected_peers.lock().len() + usize::from(self.is_enabled());
        let total = shares.clone().count();
        let own = address.map_or(0, |wanted| {
            shares.filter(|share| share.miner == wanted).count()
        });
        (total, own, miners)
    }

    pub fn next_workshare_entitlement(&self, address: Address) -> u64 {
        let chain = self.chain.read();
        reward_allocation(chain.height().saturating_add(1), address, 0, chain.blocks())
            .ok()
            .and_then(|allocation| {
                allocation
                    .workshare_rewards
                    .into_iter()
                    .find_map(|(candidate, amount)| (candidate == address).then_some(amount))
            })
            .unwrap_or(0)
    }

    pub fn mining_rewards(&self, address: Address) -> (u64, u64) {
        let chain = self.chain.read();
        let pending = chain.state().pending_for(address);
        let mut credited = u128::from(chain.state().account(address).balance) + u128::from(pending);
        let mut received = 0_u128;
        for transaction in chain.blocks().iter().flat_map(|block| &block.transactions) {
            if transaction.sender == address {
                credited += u128::from(transaction.amount) + u128::from(transaction.fee);
            }
            if transaction.recipient == address {
                received += u128::from(transaction.amount);
            }
        }
        (
            u64::try_from(credited.saturating_sub(received)).unwrap_or(u64::MAX),
            pending,
        )
    }

    pub fn account(&self, address: Address) -> (consensus::Account, u64) {
        let chain = self.chain.read();
        (
            chain.state().account(address),
            chain.state().pending_for(address),
        )
    }

    #[allow(clippy::type_complexity)]
    pub fn account_view(&self, address: Address) -> (consensus::Account, u64, u64, u64, u64) {
        let chain = self.chain.read();
        let account = chain.state().account(address);
        let pending_rewards = chain.state().pending_for(address);
        let selected = self
            .mempool
            .lock()
            .select(chain.state(), consensus::MAX_BLOCK_TRANSACTIONS);
        let mut pending_in = 0_u64;
        let mut pending_out = 0_u64;
        let mut next_nonce = account.nonce;
        for transaction in selected {
            if transaction.recipient == address {
                pending_in = pending_in.saturating_add(transaction.amount);
            }
            if transaction.sender == address {
                pending_out = pending_out
                    .saturating_add(transaction.amount)
                    .saturating_add(transaction.fee);
                next_nonce = next_nonce.max(transaction.nonce.saturating_add(1));
            }
        }
        (
            account,
            pending_rewards,
            pending_in,
            pending_out,
            next_nonce,
        )
    }

    pub fn block_at(&self, height: u64) -> Option<Block> {
        self.chain.read().block_at(height).cloned()
    }

    pub fn block_by_hash(&self, hash: Hash32) -> Option<Block> {
        self.chain
            .read()
            .block_by_hash(hash)
            .cloned()
            .or_else(|| self.storage.block(hash).ok().flatten())
    }

    pub fn submit_transaction(&self, transaction: Transaction) -> Result<Hash32, MempoolError> {
        let state = self.chain.read().state().clone();
        let id = self
            .mempool
            .lock()
            .accept(transaction, &state, unix_time())?;
        let _ = self.events.send(WireMessage::Inventory {
            kind: ObjectKind::Transaction,
            id,
        });
        Ok(id)
    }

    pub fn submit_block(&self, block: &Block) -> Result<ChainEvent, ConsensusError> {
        let _processing = self.block_processing.lock();
        let mut candidate = self.chain.read().clone();
        if candidate.block_by_hash(block.id()).is_some() {
            return Ok(ChainEvent::Known);
        }
        let old_tip = candidate.tip().id();
        let event = if block.header.previous_block == old_tip {
            candidate.append(block.clone(), self.pow.as_ref(), &SystemTimeSource)?
        } else {
            let candidate_blocks = self.assemble_candidate(&candidate, block.clone())?;
            candidate.consider_chain(candidate_blocks, self.pow.as_ref(), &SystemTimeSource)?
        };
        self.storage
            .store_side_block(block)
            .map_err(|error| ConsensusError::StorageInvariant(error.to_string()))?;
        if matches!(
            event,
            ChainEvent::Extended { .. } | ChainEvent::Reorganized { .. }
        ) {
            let (confirmed, detached) = match &event {
                ChainEvent::Extended { .. } => (block.transactions.clone(), Vec::new()),
                ChainEvent::Reorganized { fork_height, .. } => {
                    let first = usize::try_from(fork_height.saturating_add(1))
                        .map_err(|_| ConsensusError::WrongHeight)?;
                    let confirmed = candidate.blocks()[first..]
                        .iter()
                        .flat_map(|active_block| active_block.transactions.iter().cloned())
                        .collect::<Vec<_>>();
                    let detached = self.chain.read().blocks()[first..]
                        .iter()
                        .flat_map(|old_block| old_block.transactions.iter().cloned())
                        .collect::<Vec<_>>();
                    (confirmed, detached)
                }
                _ => unreachable!("guard only admits active-chain changes"),
            };
            self.storage
                .save_chain(&candidate)
                .map_err(|error| ConsensusError::StorageInvariant(error.to_string()))?;
            let confirmed_ids = confirmed
                .iter()
                .map(Transaction::id)
                .collect::<HashSet<_>>();
            let mut active = self.chain.write();
            let mut mempool = self.mempool.lock();
            mempool.remove_confirmed(&confirmed);
            for transaction in detached {
                if !confirmed_ids.contains(&transaction.id()) {
                    let _ = mempool.accept(transaction, candidate.state(), unix_time());
                }
            }
            drop(mempool);
            let new_tip = candidate.tip().id();
            *active = candidate;
            self.workshares.lock().reset(new_tip);
            self.pending_workshares.lock().clear();
            self.generation.fetch_add(1, Ordering::SeqCst);
            let _ = self.events.send(WireMessage::Inventory {
                kind: ObjectKind::Block,
                id: new_tip,
            });
        }
        Ok(event)
    }

    fn assemble_candidate(
        &self,
        active: &Chain,
        block: Block,
    ) -> Result<Vec<Block>, ConsensusError> {
        let mut reverse = vec![block];
        let mut seen = HashSet::new();
        let fork_height = loop {
            let parent_hash = reverse
                .last()
                .expect("branch contains submitted block")
                .header
                .previous_block;
            if let Some(parent) = active.block_by_hash(parent_hash) {
                break parent.header.height;
            }
            if !seen.insert(parent_hash) || reverse.len() > 100_000 {
                return Err(ConsensusError::WrongParent);
            }
            reverse.push(
                self.storage
                    .block(parent_hash)
                    .map_err(|error| ConsensusError::StorageInvariant(error.to_string()))?
                    .ok_or(ConsensusError::WrongParent)?,
            );
        };
        reverse.reverse();
        let mut candidate = active.blocks()
            [..=usize::try_from(fork_height).map_err(|_| ConsensusError::WrongHeight)?]
            .to_vec();
        candidate.extend(reverse);
        Ok(candidate)
    }

    pub fn register_template(&self, template: BlockTemplate) -> Result<Hash32, ConsensusError> {
        let id = template.id();
        if template.version != BlockTemplate::VERSION
            || template.consensus_encode().len() > consensus::MAX_TEMPLATE_BYTES
        {
            return Err(ConsensusError::SizeLimit);
        }
        let chain = self.chain.read();
        if template.height != chain.height().saturating_add(1)
            || template.parent != chain.tip().id()
        {
            return Err(ConsensusError::InvalidWorkshareChain);
        }
        if self.workshares.lock().templates.contains_key(&id) {
            return Ok(id);
        }
        consensus::validation::validate_template_transactions(
            &template.transactions,
            chain.state(),
            template.height,
        )?;
        let mut pool = self.workshares.lock();
        pool.register_template(template)?;
        drop(pool);
        let _ = self.events.send(WireMessage::Inventory {
            kind: ObjectKind::Template,
            id,
        });
        Ok(id)
    }

    pub fn submit_workshare(&self, share: Workshare) -> Result<bool, ConsensusError> {
        let id = share.id();
        let chain = self.chain.read();
        let witness = self.workshares.lock().candidate_witness(share.clone())?;
        consensus::validation::validate_workshares(
            &witness,
            chain.blocks(),
            chain.state(),
            self.pow.as_ref(),
            unix_time().saturating_add(consensus::MAX_FUTURE_BLOCK_TIME),
        )?;
        let changed = self.workshares.lock().accept(share)?;
        drop(chain);
        if changed {
            self.generation.fetch_add(1, Ordering::SeqCst);
        }
        let _ = self.events.send(WireMessage::Inventory {
            kind: ObjectKind::Workshare,
            id,
        });
        Ok(changed)
    }

    pub fn mining_candidate(&self, extranonce: u64) -> Result<MiningCandidate, ConsensusError> {
        let chain = self.chain.read();
        let parent = chain.tip();
        let height = parent.header.height + 1;
        let transactions = self
            .mempool
            .lock()
            .select(chain.state(), MAX_BLOCK_TRANSACTIONS);
        let fees = transactions.iter().try_fold(0_u64, |total, transaction| {
            total
                .checked_add(transaction.fee)
                .filter(|fees| *fees <= MAX_MONEY)
                .ok_or(ConsensusError::MoneyRange)
        })?;
        let finder = self
            .mining_config()
            .miner_address
            .ok_or(ConsensusError::RewardMismatch)?;
        let allocation = reward_allocation(height, finder, fees, chain.blocks())?;
        let claim = RewardClaim {
            version: RewardClaim::VERSION,
            height,
            finder,
            amount: allocation.finder_amount,
            extranonce,
        };
        let (post_state, _, _) = consensus::validation::transition_for_candidate(
            height,
            &claim,
            &transactions,
            chain.blocks(),
            chain.state(),
        )?;
        let witness = self.workshares.lock().witness();
        let mut leaves = Vec::with_capacity(transactions.len() + 1);
        leaves.push(claim.id());
        leaves.extend(transactions.iter().map(Transaction::id));
        let timestamp = unix_time()
            .max(parent.header.timestamp + 1)
            .max(consensus::validation::median_time_past(chain.blocks()) + 1);
        let bits = consensus::next_bits(chain.blocks());
        let header = BlockHeader {
            version: consensus::HEADER_VERSION,
            protocol: consensus::PROTOCOL_VERSION,
            height,
            previous_block: parent.id(),
            transactions_root: consensus::merkle_root(&leaves),
            state_root: post_state.root(),
            workshare_root: witness.root(),
            timestamp,
            bits,
            nonce: 0,
        };
        let template = BlockTemplate {
            version: BlockTemplate::VERSION,
            height,
            parent: parent.id(),
            transactions: transactions.clone(),
        };
        let template_id = template.id();
        let mut pool = self.workshares.lock();
        let template_is_new = !pool.templates.contains_key(&template_id);
        pool.register_template(template.clone())?;
        drop(pool);
        if template_is_new {
            let _ = self.events.send(WireMessage::Inventory {
                kind: ObjectKind::Template,
                id: template_id,
            });
        }
        let seed = consensus::validation::randomx_seed(chain.blocks(), height)?;
        let block_target = consensus::bits_to_target(bits).ok_or(ConsensusError::InvalidTarget)?;
        let workshare_target = consensus::bits_to_target(
            consensus::workshare_bits(bits).ok_or(ConsensusError::InvalidTarget)?,
        )
        .ok_or(ConsensusError::InvalidTarget)?;
        Ok(MiningCandidate {
            block: Block {
                header,
                reward_claim: Some(claim),
                transactions,
                workshare_witness: witness,
            },
            template,
            seed,
            block_target,
            workshare_target,
            generation: self.generation.load(Ordering::SeqCst),
        })
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    pub fn local_hello(&self, response_to: Hash32) -> Hello {
        let listen_url = self
            .mining_config()
            .public_url
            .as_deref()
            .map(|url| format!("{}/p2p/v10", url.trim_end_matches('/')))
            .unwrap_or_default();
        Hello::signed(
            &self.identity_secret,
            &self.chain_snapshot(),
            listen_url,
            response_to,
        )
    }

    pub fn learn_peer(&self, value: &str) {
        if value.len() > 512 {
            return;
        }
        let Some(url) = crate::p2p::normalize_peer_url(value) else {
            return;
        };
        if !url.starts_with("wss://") {
            return;
        }
        let mut peers = self.discovered_peers.lock();
        if peers.len() < crate::p2p::MAX_PEER_URLS {
            peers.insert(url);
        }
    }

    pub fn peer_addresses(&self) -> Vec<String> {
        let mut peers = self.discovered_peers.lock().clone();
        if let Some(url) = self.mining_config().public_url {
            if let Some(url) = crate::p2p::normalize_peer_url(&url) {
                peers.insert(url);
            }
        }
        peers.into_iter().take(crate::p2p::MAX_PEER_URLS).collect()
    }

    pub fn peer_connected(&self, id: Hash32, inbound: bool) -> bool {
        let mut peers = self.connected_peers.lock();
        if peers.contains_key(&id) {
            return false;
        }
        let direction_count = peers
            .values()
            .filter(|direction| **direction == inbound)
            .count();
        let limit = if inbound {
            self.config.max_inbound_peers
        } else {
            self.config.max_outbound_peers
        };
        if direction_count >= limit {
            return false;
        }
        peers.insert(id, inbound);
        true
    }

    pub fn peer_disconnected(&self, id: Hash32) {
        self.connected_peers.lock().remove(&id);
        self.sync_queues.lock().remove(&id);
        self.pending_workshares
            .lock()
            .retain(|_, (peer, _)| *peer != id);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<WireMessage> {
        self.events.subscribe()
    }

    pub fn request_sync(&self) {
        let _ = self.events.send(WireMessage::GetHeaders {
            locator: self.block_locator(),
            stop: Hash32::ZERO,
        });
    }

    pub fn block_locator(&self) -> Vec<Hash32> {
        let chain = self.chain.read();
        let mut locator = Vec::new();
        let mut height = chain.height();
        let mut step = 1_u64;
        loop {
            locator.push(
                chain
                    .block_at(height)
                    .expect("locator height is active")
                    .id(),
            );
            if height == 0 {
                break;
            }
            height = height.saturating_sub(step);
            if locator.len() >= 10 {
                step = step.saturating_mul(2);
            }
        }
        locator
    }

    #[allow(clippy::too_many_lines)]
    pub fn handle_wire(&self, peer: Hash32, message: WireMessage) -> Option<WireMessage> {
        match message {
            WireMessage::Hello(_) | WireMessage::Pong(_) => None,
            WireMessage::Peers(peers) => {
                for url in peers {
                    self.learn_peer(&url);
                }
                None
            }
            WireMessage::Ping(nonce) => Some(WireMessage::Pong(nonce)),
            WireMessage::Inventory { kind, id } => match kind {
                ObjectKind::Block if self.block_by_hash(id).is_none() => {
                    Some(WireMessage::GetData { kind, id })
                }
                ObjectKind::Transaction if !self.mempool.lock().contains(id) => {
                    Some(WireMessage::GetData { kind, id })
                }
                ObjectKind::Template if !self.workshares.lock().templates.contains_key(&id) => {
                    Some(WireMessage::GetData { kind, id })
                }
                ObjectKind::Workshare
                    if !self.workshares.lock().shares.contains_key(&id)
                        && !self.pending_workshares.lock().contains_key(&id) =>
                {
                    Some(WireMessage::GetData { kind, id })
                }
                _ => None,
            },
            WireMessage::GetData { kind, id } => match kind {
                ObjectKind::Block => self.block_by_hash(id).map(WireMessage::Block),
                ObjectKind::Transaction => {
                    self.mempool.lock().get(id).map(WireMessage::Transaction)
                }
                ObjectKind::Template => self
                    .workshares
                    .lock()
                    .templates
                    .get(&id)
                    .cloned()
                    .map(WireMessage::Template),
                ObjectKind::Workshare => self
                    .workshares
                    .lock()
                    .shares
                    .get(&id)
                    .cloned()
                    .map(WireMessage::Workshare),
            },
            WireMessage::Block(block) => {
                let id = block.id();
                if let Err(error) = self.submit_block(&block) {
                    tracing::debug!(%peer, %error, "peer supplied invalid or orphan block");
                    self.sync_queues.lock().remove(&peer);
                    return None;
                }
                let mut queues = self.sync_queues.lock();
                let queue = queues.get_mut(&peer)?;
                if queue.front().copied() != Some(id) {
                    queues.remove(&peer);
                    return None;
                }
                queue.pop_front();
                if let Some(next) = queue.front().copied() {
                    return Some(WireMessage::GetData {
                        kind: ObjectKind::Block,
                        id: next,
                    });
                }
                queues.remove(&peer);
                drop(queues);
                Some(WireMessage::GetHeaders {
                    locator: self.block_locator(),
                    stop: Hash32::ZERO,
                })
            }
            WireMessage::Transaction(transaction) => {
                let _ = self.submit_transaction(transaction);
                None
            }
            WireMessage::Template(template) => {
                if self.register_template(template).is_ok() {
                    self.process_pending_workshares();
                }
                self.pending_request_for(peer)
            }
            WireMessage::Workshare(share) => {
                let id = share.id();
                let pool = self.workshares.lock();
                let missing_template = !pool.templates.contains_key(&share.template_id);
                let missing_previous = share.previous_workshare != Hash32::ZERO
                    && !pool.shares.contains_key(&share.previous_workshare);
                drop(pool);
                if missing_template || missing_previous {
                    let mut pending = self.pending_workshares.lock();
                    if pending.len() < 2_048 {
                        pending.entry(id).or_insert((peer, share));
                    }
                    drop(pending);
                    return self.pending_request_for(peer);
                }
                let _ = self.submit_workshare(share);
                self.process_pending_workshares();
                self.pending_request_for(peer)
            }
            WireMessage::GetHeaders { locator, stop } => {
                let chain = self.chain.read();
                let start = locator
                    .iter()
                    .find_map(|hash| {
                        chain
                            .block_by_hash(*hash)
                            .map(|block| block.header.height + 1)
                    })
                    .unwrap_or(0);
                let headers = chain
                    .blocks()
                    .iter()
                    .skip(usize::try_from(start).unwrap_or(usize::MAX))
                    .take(crate::p2p::MAX_HEADERS)
                    .take_while(|block| stop == Hash32::ZERO || block.id() != stop)
                    .map(|block| block.header.clone())
                    .collect();
                Some(WireMessage::Headers(headers))
            }
            WireMessage::Headers(headers) => {
                if headers.is_empty() || headers.len() > crate::p2p::MAX_HEADERS {
                    self.sync_queues.lock().remove(&peer);
                    return None;
                }
                let mut expected_parent = None;
                let mut expected_height = None;
                let mut queue = VecDeque::new();
                for header in headers {
                    if header.version != consensus::HEADER_VERSION
                        || header.protocol != consensus::PROTOCOL_VERSION
                    {
                        self.sync_queues.lock().remove(&peer);
                        return None;
                    }
                    if let (Some(parent), Some(height)) = (expected_parent, expected_height) {
                        if header.previous_block != parent || header.height != height {
                            self.sync_queues.lock().remove(&peer);
                            return None;
                        }
                    } else {
                        let Some(parent) = self.block_by_hash(header.previous_block) else {
                            self.sync_queues.lock().remove(&peer);
                            return None;
                        };
                        if header.height != parent.header.height.saturating_add(1) {
                            self.sync_queues.lock().remove(&peer);
                            return None;
                        }
                    }
                    let id = header.id();
                    if self.block_by_hash(id).is_none() {
                        queue.push_back(id);
                    }
                    expected_parent = Some(id);
                    expected_height = Some(header.height.saturating_add(1));
                }
                let next = queue.front().copied()?;
                self.sync_queues.lock().insert(peer, queue);
                Some(WireMessage::GetData {
                    kind: ObjectKind::Block,
                    id: next,
                })
            }
        }
    }

    pub fn pow(&self) -> &Arc<RandomX> {
        &self.pow
    }

    fn process_pending_workshares(&self) {
        loop {
            let ready = {
                let pool = self.workshares.lock();
                self.pending_workshares
                    .lock()
                    .iter()
                    .find_map(|(id, (_, share))| {
                        let template_ready = pool.templates.contains_key(&share.template_id);
                        let predecessor_ready = share.previous_workshare == Hash32::ZERO
                            || pool.shares.contains_key(&share.previous_workshare);
                        (template_ready && predecessor_ready).then_some(*id)
                    })
            };
            let Some(id) = ready else { break };
            let Some((_, share)) = self.pending_workshares.lock().remove(&id) else {
                continue;
            };
            let _ = self.submit_workshare(share);
        }
    }

    fn pending_request_for(&self, peer: Hash32) -> Option<WireMessage> {
        let pool = self.workshares.lock();
        self.pending_workshares
            .lock()
            .values()
            .find_map(|(origin, share)| {
                if *origin != peer {
                    return None;
                }
                if !pool.templates.contains_key(&share.template_id) {
                    return Some(WireMessage::GetData {
                        kind: ObjectKind::Template,
                        id: share.template_id,
                    });
                }
                if share.previous_workshare != Hash32::ZERO
                    && !pool.shares.contains_key(&share.previous_workshare)
                {
                    return Some(WireMessage::GetData {
                        kind: ObjectKind::Workshare,
                        id: share.previous_workshare,
                    });
                }
                None
            })
    }
}

fn scheduled_supply(height: u64) -> u64 {
    let mut remaining = height;
    let mut subsidy = consensus::INITIAL_SUBSIDY;
    let mut total = 0_u64;
    while remaining > 0 {
        let span = remaining.min(consensus::HALVING_INTERVAL);
        total = total.saturating_add(span.saturating_mul(subsidy));
        remaining -= span;
        subsidy = (subsidy / 2).max(consensus::MIN_SUBSIDY);
    }
    total
}

fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn historical_reward(
    history: &[Block],
    index: usize,
    address: Address,
    share_blocks: &[usize],
) -> Option<serde_json::Value> {
    let block = &history[index];
    let claim = block.reward_claim.as_ref()?;
    let finder_amount = if claim.finder == address {
        claim.amount
    } else {
        0
    };
    let window = usize::try_from(consensus::REWARD_WINDOW).expect("window fits usize");
    let eligible = share_blocks
        .partition_point(|position| *position < index)
        .checked_sub(1)
        .is_some_and(|position| share_blocks[position] >= index.saturating_sub(window));
    let share_amount = if eligible {
        reward_allocation(block.header.height, claim.finder, 0, &history[..index])
            .ok()?
            .workshare_rewards
            .into_iter()
            .find_map(|(recipient, amount)| (recipient == address).then_some(amount))
            .unwrap_or(0)
    } else {
        0
    };
    let amount = finder_amount + share_amount;
    if amount == 0 {
        return None;
    }
    let mut identity = block.id().consensus_encode();
    identity.extend(address.consensus_encode());
    let height = history.last()?.header.height;
    Some(serde_json::json!({
        "txid": domain_hash(b"PARITR-REWARD-HISTORY-v1", &identity),
        "direction": "reward",
        "amount": amount,
        "finder_amount": finder_amount,
        "workshare_amount": share_amount,
        "fee": 0,
        "timestamp": block.header.timestamp,
        "height": block.header.height,
        "confirmations": height.saturating_sub(block.header.height).saturating_add(1),
        "status": "confirmed",
        "matured": height >= block.header.height.saturating_add(consensus::REWARD_MATURITY),
        "maturity_height": block.header.height.saturating_add(consensus::REWARD_MATURITY),
    }))
}

pub fn randomx_fast_available() -> bool {
    if usize::BITS < 64 {
        return false;
    }
    #[cfg(target_os = "linux")]
    {
        const MINIMUM_KIB: u64 = 2_621_440;
        for path in [
            "/sys/fs/cgroup/memory.max",
            "/sys/fs/cgroup/memory/memory.limit_in_bytes",
        ] {
            if let Ok(limit) = std::fs::read_to_string(path) {
                if let Ok(bytes) = limit.trim().parse::<u64>() {
                    if bytes / 1024 < MINIMUM_KIB {
                        return false;
                    }
                }
            }
        }
        if let Ok(meminfo) = std::fs::read_to_string("/proc/meminfo") {
            if let Some(total) = meminfo.lines().find_map(|line| {
                line.strip_prefix("MemTotal:")?
                    .split_whitespace()
                    .next()?
                    .parse::<u64>()
                    .ok()
            }) {
                return total >= MINIMUM_KIB;
            }
        }
    }
    true
}

pub(crate) fn work_as_f64(work: U256) -> f64 {
    work.to_string().parse().unwrap_or(f64::MAX)
}

pub(crate) fn displayed_difficulty(bits: u32) -> f64 {
    consensus::bits_to_target(bits).map_or(0.0, |target| {
        work_as_f64(consensus::pow_limit()) / work_as_f64(target)
    })
}

#[allow(dead_code)]
fn is_database(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension == "redb")
}
