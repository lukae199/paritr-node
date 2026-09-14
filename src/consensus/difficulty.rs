use primitive_types::{U256, U512};

use super::{
    initial_target, pow_limit, Block, ASERT_HALF_LIFE, TARGET_BLOCK_TIME,
    WORKSHARE_TARGET_MULTIPLIER,
};

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
    // Size 33 is the canonical compact form for high 255/256-bit values;
    // only a 16-bit mantissa can fit without overflowing U256.
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

/// Target for the block following `parent_height`.
///
/// The candidate's own timestamp never changes its target. This removes the
/// Protocol-8 emergency-rule incentive while retaining deterministic recovery
/// from large hashrate changes through ASERT.
pub fn required_target(parent_height: u64, parent_timestamp: u64, launch_timestamp: u64) -> U256 {
    if parent_height <= 1 {
        return initial_target();
    }
    // The first mined block anchors the schedule of its branch. Time spent
    // waiting to launch the network must never lower its starting difficulty.
    let ideal_elapsed = i128::from(parent_height - 1) * i128::from(TARGET_BLOCK_TIME);
    let actual_elapsed = i128::from(parent_timestamp) - i128::from(launch_timestamp);
    let drift = actual_elapsed - ideal_elapsed;
    let exponent = (drift * 65_536).div_euclid(i128::from(ASERT_HALF_LIFE));
    let shifts = exponent.div_euclid(65_536);
    let fraction = exponent.rem_euclid(65_536);

    // BCH's thoroughly reviewed cubic approximation of 2^(x/65536), evaluated
    // entirely with integers. Maximum relative error is far below one target bit.
    let factor = 65_536_i128
        + ((195_766_423_245_049_i128 * fraction
            + 971_821_376_i128 * fraction * fraction
            + 5_127_i128 * fraction * fraction * fraction
            + (1_i128 << 47))
            >> 48);

    let factor = u64::try_from(factor).expect("ASERT polynomial factor is positive and bounded");
    let mut target = U512::from(initial_target()) * U512::from(factor);
    target >>= 16;
    if shifts < 0 {
        target >>= usize::try_from(-shifts).unwrap_or(usize::MAX).min(511);
    } else {
        if shifts >= 256 {
            return pow_limit();
        }
        let shift = usize::try_from(shifts).expect("nonnegative shift");
        if target > (U512::from(pow_limit()) >> shift) {
            return pow_limit();
        }
        target <<= shift;
    }
    u512_to_u256_clamped(target, pow_limit()).max(U256::one())
}

pub fn required_bits(parent_height: u64, parent_timestamp: u64, launch_timestamp: u64) -> u32 {
    target_to_bits(required_target(
        parent_height,
        parent_timestamp,
        launch_timestamp,
    ))
}

/// The same branch-local anchor is used by miners, block validation and shares.
pub fn next_bits(history: &[Block]) -> u32 {
    let Some(parent) = history.last() else {
        return target_to_bits(initial_target());
    };
    let launch_timestamp = history
        .get(1)
        .map_or(parent.header.timestamp, |block| block.header.timestamp);
    required_bits(
        parent.header.height,
        parent.header.timestamp,
        launch_timestamp,
    )
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
        assert!(bits_to_target(0).is_none());
        assert!(bits_to_target(0x1d80_ffff).is_none());
    }

    #[test]
    fn asert_doubles_and_halves_near_half_life() {
        let launch = 1_000_000;
        let ideal = launch + 200 * TARGET_BLOCK_TIME;
        let baseline = required_target(201, ideal, launch);
        let slower = required_target(201, ideal + ASERT_HALF_LIFE as u64, launch);
        assert!(slower >= baseline * 2 - U256::from(2_u8));
        let faster = required_target(201, ideal - ASERT_HALF_LIFE as u64, launch);
        assert!(faster <= baseline / 2 + U256::from(2_u8));
    }

    #[test]
    fn launch_delay_does_not_change_difficulty_and_extreme_gaps_saturate() {
        assert_eq!(required_target(0, 0, 0), initial_target());
        assert_eq!(required_target(1, u64::MAX, u64::MAX), initial_target());
        assert_eq!(
            required_target(101, 10_000 + 100 * TARGET_BLOCK_TIME, 10_000),
            initial_target()
        );
        assert_eq!(
            required_target(101, 1_000_000 + 100 * TARGET_BLOCK_TIME, 1_000_000),
            initial_target()
        );
        assert_eq!(required_target(2, u64::MAX, 0), pow_limit());
        assert_eq!(required_target(u64::MAX, 0, 0), U256::one());
    }

    #[test]
    fn workshare_target_uses_consensus_multiplier() {
        let bits = target_to_bits(initial_target());
        let share = bits_to_target(workshare_bits(bits).unwrap()).unwrap();
        let block = bits_to_target(bits).unwrap();
        assert!(share >= block * WORKSHARE_TARGET_MULTIPLIER);
    }
}
