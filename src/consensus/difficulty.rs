use primitive_types::{U256, U512};

use super::{initial_target, pow_limit, Block, TARGET_BLOCK_TIME, WORKSHARE_TARGET_MULTIPLIER};

pub const DAA_WINDOW: usize = 16;

pub fn target_to_bits(target: U256) -> u32 {
    if target.is_zero() {
        return 0;
    }
    let bytes = target.to_big_endian();
    let first = bytes.iter().position(|byte| *byte != 0).unwrap_or(31);
    let significant = &bytes[first..];
    let mut size = u32::try_from(significant.len()).expect("U256 length fits in u32");
    let word = if significant[0] & 0x80 != 0 {
        size += 1;
        (u32::from(significant[0]) << 8) | significant.get(1).map_or(0, |byte| u32::from(*byte))
    } else {
        (u32::from(significant[0]) << 16)
            | (significant.get(1).map_or(0, |byte| u32::from(*byte)) << 8)
            | significant.get(2).map_or(0, |byte| u32::from(*byte))
    };
    (size << 24) | word
}

pub fn bits_to_target(bits: u32) -> Option<U256> {
    let size = bits >> 24;
    let word = bits & 0x007f_ffff;
    if word == 0
        || bits & 0x0080_0000 != 0
        || size == 0
        || size > 33
        || (size == 33 && word > 0xffff)
    {
        return None;
    }
    let target = if size <= 3 {
        U256::from(word >> (8 * (3 - size)))
    } else {
        U256::from(word) << (8 * (size - 3))
    };
    (target_to_bits(target) == bits).then_some(target)
}

/// Responsive LWMA-16 algorithm: linearly weights recent solve times to eliminate
/// Poisson oscillations while reacting quickly to hashrate changes.
pub fn calculate_next_target(history: &[Block]) -> U256 {
    if history.len() < 2 {
        return initial_target();
    }
    let window_size = (history.len() - 1).min(DAA_WINDOW);
    let start_idx = history.len() - window_size;
    let slice = &history[start_idx - 1..];

    let mut weighted_times: u64 = 0;
    let mut weight_sum: u64 = 0;
    let mut avg_target = U512::zero();

    for i in 1..=window_size {
        let weight = i as u64;
        let solve_time = slice[i]
            .header
            .timestamp
            .saturating_sub(slice[i - 1].header.timestamp)
            .clamp(1, TARGET_BLOCK_TIME * 3);

        weighted_times += solve_time * weight;
        weight_sum += weight;
        let target = bits_to_target(slice[i].header.bits).unwrap_or_else(initial_target);
        avg_target += U512::from(target) * U512::from(weight);
    }

    let weighted_solve_time = (weighted_times + weight_sum / 2) / weight_sum;
    avg_target /= U512::from(weight_sum);

    let min_allowed = TARGET_BLOCK_TIME / 2; // 32s (-50% target cap)
    let max_allowed = TARGET_BLOCK_TIME + TARGET_BLOCK_TIME / 2; // 96s (+50% target cap)
    let clamped_time = weighted_solve_time.clamp(min_allowed, max_allowed);

    let next_target = (avg_target * U512::from(clamped_time)) / U512::from(TARGET_BLOCK_TIME);
    u512_to_u256_clamped(next_target, pow_limit()).max(U256::one())
}

pub fn next_bits(history: &[Block]) -> u32 {
    target_to_bits(calculate_next_target(history))
}

pub fn required_target(history: &[Block]) -> U256 {
    calculate_next_target(history)
}

pub fn required_bits(history: &[Block]) -> u32 {
    target_to_bits(calculate_next_target(history))
}

pub fn workshare_bits(block_bits: u32) -> Option<u32> {
    let block_target = bits_to_target(block_bits)?;
    let expanded = U512::from(block_target) * U512::from(WORKSHARE_TARGET_MULTIPLIER);
    let share_target = u512_to_u256_clamped(expanded, U256::MAX);
    Some(target_to_bits(share_target.max(U256::one())))
}

pub fn target_work(target: U256) -> U256 {
    if target == U256::MAX {
        return U256::one();
    }
    (!target / (target + U256::one())) + U256::one()
}

pub fn hash_meets_target(hash: [u8; 32], target: U256) -> bool {
    U256::from_little_endian(&hash) <= target
}

fn u512_to_u256_clamped(value: U512, maximum: U256) -> U256 {
    let bytes = value.to_big_endian();
    if bytes[..32].iter().any(|byte| *byte != 0) {
        return maximum;
    }
    U256::from_big_endian(&bytes[32..]).min(maximum)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_targets_are_canonical() {
        for target in [U256::one(), initial_target(), pow_limit(), U256::MAX] {
            let bits = target_to_bits(target);
            let decoded = bits_to_target(bits).unwrap();
            assert_eq!(target_to_bits(decoded), bits);
            assert!(decoded <= target);
        }
    }
}
