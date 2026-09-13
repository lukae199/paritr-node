use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use primitive_types::U256;
use rand::RngCore;
use tracing::{debug, info, warn};

use crate::{
    codec::ConsensusEncode,
    consensus::{Workshare, MAX_FUTURE_BLOCK_TIME},
    crypto::Hash32,
    node::Node,
};

#[derive(Default)]
pub struct MiningStats {
    hashes: AtomicU64,
    blocks: AtomicU64,
    workshares: AtomicU64,
    started_millis: AtomicU64,
}

impl MiningStats {
    pub fn hashes(&self) -> u64 {
        self.hashes.load(Ordering::Relaxed)
    }

    pub fn blocks(&self) -> u64 {
        self.blocks.load(Ordering::Relaxed)
    }

    pub fn workshares(&self) -> u64 {
        self.workshares.load(Ordering::Relaxed)
    }

    #[allow(clippy::cast_precision_loss)]
    pub fn hashrate(&self) -> f64 {
        let started = self.started_millis.load(Ordering::Relaxed);
        let elapsed = unix_millis().saturating_sub(started);
        if started == 0 || elapsed == 0 {
            0.0
        } else {
            self.hashes() as f64 * 1_000.0 / elapsed as f64
        }
    }
}

pub struct MiningController {
    stop: Arc<AtomicBool>,
    handles: Vec<JoinHandle<()>>,
    pub stats: Arc<MiningStats>,
}

impl MiningController {
    pub fn start(node: &Arc<Node>) -> Option<Self> {
        if !node.config.mining_enabled {
            return None;
        }
        let threads = if node.config.mining_threads == 0 {
            std::thread::available_parallelism().map_or(1, usize::from)
        } else {
            node.config.mining_threads
        }
        .max(1);
        let stop = Arc::new(AtomicBool::new(false));
        let stats = node.mining_stats();
        stats.started_millis.store(unix_millis(), Ordering::Relaxed);
        let mut handles = Vec::with_capacity(threads);
        for worker_id in 0..threads {
            let node = Arc::clone(node);
            let stop = Arc::clone(&stop);
            let stats = Arc::clone(&stats);
            handles.push(
                std::thread::Builder::new()
                    .name(format!("paritr-randomx-{worker_id}"))
                    .spawn(move || mine_loop(worker_id, &node, &stop, &stats))
                    .expect("mining worker can be spawned"),
            );
        }
        info!(threads, "RandomX mining started");
        Some(Self {
            stop,
            handles,
            stats,
        })
    }

    pub fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        for handle in self.handles.drain(..) {
            let _ = handle.join();
        }
    }
}

fn mine_loop(worker_id: usize, node: &Node, stop: &AtomicBool, stats: &MiningStats) {
    let intensity = node.config.mining_intensity;
    while !stop.load(Ordering::Relaxed) {
        if !node.is_enabled() {
            std::thread::sleep(Duration::from_millis(250));
            continue;
        }
        let extranonce = rand::rngs::OsRng.next_u64() ^ worker_id as u64;
        let candidate = match node.mining_candidate(extranonce) {
            Ok(candidate) => candidate,
            Err(error) => {
                warn!(worker_id, %error, "cannot construct mining candidate");
                std::thread::sleep(Duration::from_secs(2));
                continue;
            }
        };
        let previous_workshare = candidate
            .block
            .workshare_witness
            .workshares
            .last()
            .map_or(Hash32::ZERO, Workshare::id);
        let started = Instant::now();
        let mut header = candidate.block.header.clone();
        let mut nonce = rand::rngs::OsRng.next_u64();
        let mut burst_started = Instant::now();
        let mut burst_hashes = 0_u64;

        loop {
            if stop.load(Ordering::Relaxed)
                || !node.is_enabled()
                || node.generation() != candidate.generation
                || started.elapsed() > Duration::from_secs(MAX_FUTURE_BLOCK_TIME / 2)
            {
                break;
            }
            header.nonce = nonce;
            let hash = match node
                .pow()
                .calculate(&candidate.seed, &header.consensus_encode())
            {
                Ok(hash) => hash,
                Err(error) => {
                    warn!(worker_id, %error, "RandomX worker failed");
                    std::thread::sleep(Duration::from_secs(1));
                    break;
                }
            };
            stats.hashes.fetch_add(1, Ordering::Relaxed);
            burst_hashes += 1;
            let value = U256::from_little_endian(&hash);
            if value <= candidate.block_target {
                let mut block = candidate.block.clone();
                block.header = header.clone();
                match node.submit_block(&block) {
                    Ok(_) => {
                        stats.blocks.fetch_add(1, Ordering::Relaxed);
                        info!(worker_id, nonce, "full RandomX block found");
                    }
                    Err(error) => debug!(worker_id, %error, "locally mined block became stale"),
                }
                break;
            }
            if value <= candidate.workshare_target {
                let share = Workshare {
                    version: Workshare::VERSION,
                    previous_workshare,
                    template_id: candidate.template.id(),
                    miner: candidate
                        .block
                        .reward_claim
                        .as_ref()
                        .expect("candidate claim")
                        .finder,
                    extranonce,
                    candidate_header: header.clone(),
                };
                match node.submit_workshare(share) {
                    Ok(_) => {
                        stats.workshares.fetch_add(1, Ordering::Relaxed);
                        debug!(worker_id, nonce, "workshare found");
                    }
                    Err(error) => debug!(worker_id, %error, "locally mined workshare became stale"),
                }
                break;
            }
            nonce = nonce.wrapping_add(1);

            if burst_hashes >= 64 {
                if intensity < 100 {
                    let elapsed = burst_started.elapsed();
                    let pause = elapsed.mul_f64(f64::from(100 - intensity) / f64::from(intensity));
                    std::thread::sleep(pause.min(Duration::from_secs(2)));
                }
                burst_hashes = 0;
                burst_started = Instant::now();
            }
        }
    }
}

fn unix_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}
