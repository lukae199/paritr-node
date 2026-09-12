use std::collections::BTreeMap;

use primitive_types::{U256, U512};

use crate::crypto::Address;

use super::{
    bits_to_target, block_subsidy, target_work, workshare_bits, Block, ConsensusError,
    FINDER_SHARE_PERCENT, MAX_MONEY, REWARD_WINDOW,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RewardAllocation {
    pub finder_amount: u64,
    pub workshare_rewards: Vec<(Address, u64)>,
}

impl RewardAllocation {
    pub fn total(&self) -> u64 {
        self.workshare_rewards
            .iter()
            .fold(self.finder_amount, |total, (_, amount)| total + amount)
    }
}

pub fn reward_allocation(
    height: u64,
    finder: Address,
    fees: u64,
    parent_history: &[Block],
) -> Result<RewardAllocation, ConsensusError> {
    let subsidy = block_subsidy(height);
    if fees > MAX_MONEY || subsidy.checked_add(fees).is_none() {
        return Err(ConsensusError::MoneyRange);
    }
    let finder_base = subsidy * FINDER_SHARE_PERCENT / 100;
    let pool = subsidy - finder_base;

    let start = parent_history
        .len()
        .saturating_sub(usize::try_from(REWARD_WINDOW).expect("window fits usize"));
    let mut weights: BTreeMap<Address, U512> = BTreeMap::new();
    for block in &parent_history[start..] {
        for share in &block.workshare_witness.workshares {
            let share_bits =
                workshare_bits(share.candidate_header.bits).ok_or(ConsensusError::InvalidTarget)?;
            let target = bits_to_target(share_bits).ok_or(ConsensusError::InvalidTarget)?;
            let weight = U512::from(target_work(target));
            let entry = weights.entry(share.miner).or_default();
            *entry = entry.saturating_add(weight);
        }
    }

    let total_weight = weights
        .values()
        .copied()
        .fold(U512::zero(), U512::saturating_add);
    if pool == 0 || total_weight.is_zero() {
        return Ok(RewardAllocation {
            finder_amount: finder_base
                .checked_add(pool)
                .and_then(|value| value.checked_add(fees))
                .ok_or(ConsensusError::MoneyRange)?,
            workshare_rewards: Vec::new(),
        });
    }

    let mut allocations = Vec::with_capacity(weights.len());
    let mut distributed = 0_u64;
    for (address, weight) in weights {
        let numerator = U512::from(pool) * weight;
        let amount = (numerator / total_weight).low_u64();
        let remainder = numerator % total_weight;
        distributed = distributed
            .checked_add(amount)
            .ok_or(ConsensusError::MoneyRange)?;
        allocations.push((address, amount, remainder));
    }
    let leftover = pool
        .checked_sub(distributed)
        .ok_or(ConsensusError::MoneyRange)?;
    allocations.sort_by(|left, right| right.2.cmp(&left.2).then_with(|| left.0.cmp(&right.0)));
    for allocation in allocations
        .iter_mut()
        .take(usize::try_from(leftover).expect("pool remainder fits usize"))
    {
        allocation.1 += 1;
    }
    allocations.sort_by_key(|allocation| allocation.0);
    let workshare_rewards = allocations
        .into_iter()
        .filter_map(|(address, amount, _)| (amount > 0).then_some((address, amount)))
        .collect::<Vec<_>>();

    let result = RewardAllocation {
        finder_amount: finder_base
            .checked_add(fees)
            .ok_or(ConsensusError::MoneyRange)?,
        workshare_rewards,
    };
    if result.total() != subsidy + fees {
        return Err(ConsensusError::RewardMismatch);
    }
    let _ = finder; // Finder identity matters to the caller's state transition.
    Ok(result)
}

pub fn workshare_weight(block_bits: u32) -> Result<U256, ConsensusError> {
    let bits = workshare_bits(block_bits).ok_or(ConsensusError::InvalidTarget)?;
    Ok(target_work(
        bits_to_target(bits).ok_or(ConsensusError::InvalidTarget)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consensus::{
        BlockHeader, BlockTemplate, Workshare, WorkshareWitness, HEADER_VERSION, PROTOCOL_VERSION,
    };
    use crate::crypto::Hash32;

    fn address(byte: u8) -> Address {
        Address::from_public_key_hash([byte; 20])
    }

    fn share(miner: Address, nonce: u64, bits: u32) -> Workshare {
        Workshare {
            version: Workshare::VERSION,
            previous_workshare: Hash32::ZERO,
            template_id: Hash32([u8::try_from(nonce).expect("test nonce fits in u8"); 32]),
            miner,
            extranonce: nonce,
            candidate_header: BlockHeader {
                version: HEADER_VERSION,
                protocol: PROTOCOL_VERSION,
                height: 1,
                previous_block: Block::genesis().id(),
                transactions_root: Hash32::ZERO,
                state_root: Hash32::ZERO,
                workshare_root: Hash32::ZERO,
                timestamp: 1,
                bits,
                nonce,
            },
        }
    }

    #[test]
    fn bootstrap_pool_goes_to_finder() {
        let allocation = reward_allocation(1, address(1), 7, &[Block::genesis()]).unwrap();
        assert!(allocation.workshare_rewards.is_empty());
        assert_eq!(allocation.finder_amount, super::super::INITIAL_SUBSIDY + 7);
    }

    #[test]
    fn equal_work_is_split_exactly_and_address_sorted() {
        let bits = super::super::target_to_bits(super::super::initial_target());
        let left = address(2);
        let right = address(3);
        let shares = vec![share(right, 1, bits), share(left, 2, bits)];
        let templates = shares
            .iter()
            .map(|_| BlockTemplate {
                version: BlockTemplate::VERSION,
                height: 1,
                parent: Block::genesis().id(),
                transactions: Vec::new(),
            })
            .collect();
        let mut settled = Block::genesis();
        settled.workshare_witness = WorkshareWitness {
            version: WorkshareWitness::VERSION,
            templates,
            workshares: shares,
        };
        let allocation = reward_allocation(2, address(9), 11, &[settled]).unwrap();
        assert_eq!(allocation.total(), super::super::INITIAL_SUBSIDY + 11);
        assert_eq!(allocation.workshare_rewards.len(), 2);
        assert_eq!(allocation.workshare_rewards[0].0, left);
        assert_eq!(allocation.workshare_rewards[1].0, right);
        assert_eq!(
            allocation.workshare_rewards[0].1,
            allocation.workshare_rewards[1].1
        );
    }
}
