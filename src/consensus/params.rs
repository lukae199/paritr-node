use primitive_types::U256;

pub const NODE_VERSION: &str = "4.0.1-rc.2";
pub const PROTOCOL_VERSION: u16 = 9;
pub const HEADER_VERSION: u16 = 3;
pub const CHAIN_ID: &str = "paritr-mainnet";

pub const COIN: u64 = 100_000_000;
pub const INITIAL_SUBSIDY: u64 = 10 * COIN;
pub const HALVING_INTERVAL: u64 = 1_600_000;
pub const MIN_SUBSIDY: u64 = COIN / 2;
pub const FINDER_SHARE_PERCENT: u64 = 5;
pub const REWARD_WINDOW: u64 = 1_440;
pub const REWARD_MATURITY: u64 = 100;

pub const TARGET_BLOCK_TIME: u64 = 64;
pub const ASERT_HALF_LIFE: i64 = 2 * 60 * 60;
pub const MEDIAN_TIME_SPAN: usize = 11;
pub const MAX_FUTURE_BLOCK_TIME: u64 = 300;

// Two-second expected cadence. This smooths rewards while remaining bounded by
// the witness count/size limits and does not alter the 64-second block target.
pub const WORKSHARE_TARGET_MULTIPLIER: u64 = 32;
pub const MAX_WORKSHARES_PER_BLOCK: usize = 512;
pub const MAX_TEMPLATES_PER_BLOCK: usize = 512;
pub const MAX_WORKSHARE_WITNESS_BYTES: usize = 4 * 1024 * 1024;

pub const MAX_BLOCK_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_BLOCK_TRANSACTIONS: usize = 4_096;
pub const MAX_TRANSACTION_BYTES: usize = 1_024;
pub const MAX_TEMPLATE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_NOTE_BYTES: usize = 256;
pub const MAX_MONEY: u64 = (1_u64 << 62) - 1;

pub const RANDOMX_EPOCH_BLOCKS: u64 = 2_048;
pub const RANDOMX_SEED_LAG: u64 = 64;
pub const RANDOMX_VERSION_LINE: &str = "1.2.3";
pub const RANDOMX_BOOTSTRAP_SEED: &[u8] = b"Paritr-Protocol-9-RandomX-Bootstrap-v1";

pub const GENESIS_TIMESTAMP: u64 = 1_788_984_000;
pub const GENESIS_MESSAGE: &str =
    "Paritr Mainnet Protocol 9 - sustainable workshare PoW - 2026-09-09";

pub fn pow_limit() -> U256 {
    (U256::one() << 248) - U256::one()
}

pub fn initial_target() -> U256 {
    (U256::one() << 242) - U256::one()
}

pub fn block_subsidy(height: u64) -> u64 {
    if height == 0 {
        return 0;
    }
    let halvings = (height - 1) / HALVING_INTERVAL;
    if halvings >= 64 {
        return MIN_SUBSIDY;
    }
    (INITIAL_SUBSIDY >> halvings).max(MIN_SUBSIDY)
}

pub fn randomx_seed_reference_height(height: u64) -> u64 {
    if height < RANDOMX_EPOCH_BLOCKS {
        return 0;
    }
    let epoch_start = (height / RANDOMX_EPOCH_BLOCKS) * RANDOMX_EPOCH_BLOCKS;
    epoch_start.saturating_sub(RANDOMX_SEED_LAG)
}
