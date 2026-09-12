//! Local relay/mining policy. None of these values make a block invalid.

pub const DEFAULT_MIN_RELAY_FEE: u64 = 100_000;
pub const DEFAULT_DUST_LIMIT: u64 = 1_000;
pub const DEFAULT_MAX_MEMPOOL_BYTES: usize = 64 * 1024 * 1024;
pub const DEFAULT_MAX_NONCE_GAP: u64 = 64;
pub const DEFAULT_MEMPOOL_TTL_SECONDS: u64 = 24 * 60 * 60;
pub const RBF_PERCENT: u64 = 10;

#[derive(Clone, Debug)]
pub struct RelayPolicy {
    pub min_relay_fee: u64,
    pub dust_limit: u64,
    pub max_mempool_bytes: usize,
    pub max_nonce_gap: u64,
    pub ttl_seconds: u64,
}

impl Default for RelayPolicy {
    fn default() -> Self {
        Self {
            min_relay_fee: DEFAULT_MIN_RELAY_FEE,
            dust_limit: DEFAULT_DUST_LIMIT,
            max_mempool_bytes: DEFAULT_MAX_MEMPOOL_BYTES,
            max_nonce_gap: DEFAULT_MAX_NONCE_GAP,
            ttl_seconds: DEFAULT_MEMPOOL_TTL_SECONDS,
        }
    }
}
