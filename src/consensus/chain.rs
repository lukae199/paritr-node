use primitive_types::U256;
use serde::Serialize;

use crate::crypto::Hash32;

use super::{
    validation::{cumulative_work, validate_block},
    Block, ConsensusError, LedgerState, PowVerifier, TimeSource,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChainEvent {
    Extended {
        block: Hash32,
        height: u64,
    },
    Reorganized {
        old_tip: Hash32,
        new_tip: Hash32,
        fork_height: u64,
    },
    Known,
    SideChain,
}

#[derive(Clone, Debug, Serialize)]
pub struct ChainSnapshot {
    pub height: u64,
    pub tip: Hash32,
    pub cumulative_work: String,
    pub state_root: Hash32,
}

#[derive(Clone, Debug)]
pub struct Chain {
    blocks: Vec<Block>,
    state: LedgerState,
    cumulative_work: U256,
}

impl Chain {
    pub fn genesis() -> Self {
        let genesis = Block::genesis();
        let cumulative_work =
            cumulative_work(std::slice::from_ref(&genesis)).expect("fixed genesis target is valid");
        Self {
            blocks: vec![genesis],
            state: LedgerState::default(),
            cumulative_work,
        }
    }

    pub fn from_blocks(
        blocks: Vec<Block>,
        pow: &dyn PowVerifier,
        time: &dyn TimeSource,
    ) -> Result<Self, ConsensusError> {
        if blocks.first() != Some(&Block::genesis()) {
            return Err(ConsensusError::DifferentGenesis);
        }
        let mut state = LedgerState::default();
        for index in 1..blocks.len() {
            state = validate_block(&blocks[index], &blocks[..index], &state, pow, time)?;
        }
        let cumulative_work = cumulative_work(&blocks)?;
        Ok(Self {
            blocks,
            state,
            cumulative_work,
        })
    }

    pub fn height(&self) -> u64 {
        self.tip().header.height
    }

    pub fn tip(&self) -> &Block {
        self.blocks.last().expect("chain always contains genesis")
    }

    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    pub fn state(&self) -> &LedgerState {
        &self.state
    }

    pub fn cumulative_work(&self) -> U256 {
        self.cumulative_work
    }

    pub fn block_at(&self, height: u64) -> Option<&Block> {
        self.blocks.get(usize::try_from(height).ok()?)
    }

    pub fn block_by_hash(&self, hash: Hash32) -> Option<&Block> {
        self.blocks.iter().find(|block| block.id() == hash)
    }

    pub fn append(
        &mut self,
        block: Block,
        pow: &dyn PowVerifier,
        time: &dyn TimeSource,
    ) -> Result<ChainEvent, ConsensusError> {
        if self.block_by_hash(block.id()).is_some() {
            return Ok(ChainEvent::Known);
        }
        if block.header.previous_block != self.tip().id() {
            return Err(ConsensusError::WrongParent);
        }
        let next_state = validate_block(&block, &self.blocks, &self.state, pow, time)?;
        let target =
            super::bits_to_target(block.header.bits).ok_or(ConsensusError::InvalidTarget)?;
        self.cumulative_work = self
            .cumulative_work
            .checked_add(super::target_work(target))
            .ok_or(ConsensusError::ArithmeticOverflow)?;
        self.state = next_state;
        let id = block.id();
        let height = block.header.height;
        self.blocks.push(block);
        Ok(ChainEvent::Extended { block: id, height })
    }

    /// Validate a complete candidate before changing live state. There is no
    /// consensus reorg-depth cap: the valid chain with the greatest real L1
    /// work wins. Equal-work ties use the lower tip id only as a deterministic
    /// convergence rule; Workshares are never part of this comparison.
    pub fn consider_chain(
        &mut self,
        candidate: Vec<Block>,
        pow: &dyn PowVerifier,
        time: &dyn TimeSource,
    ) -> Result<ChainEvent, ConsensusError> {
        if candidate.first() != self.blocks.first() {
            return Err(ConsensusError::DifferentGenesis);
        }
        // Side branches are validated even when they are not yet preferred. This
        // prevents poisoned branch objects from entering persistent storage and
        // makes later branch extension independent of arrival order.
        let validated = Self::from_blocks(candidate, pow, time)?;
        let candidate_tip = validated.tip().id();
        let preferred = validated.cumulative_work > self.cumulative_work
            || (validated.cumulative_work == self.cumulative_work
                && candidate_tip < self.tip().id());
        if !preferred {
            return Ok(ChainEvent::SideChain);
        }
        let fork_index = self
            .blocks
            .iter()
            .zip(validated.blocks.iter())
            .take_while(|(left, right)| left.id() == right.id())
            .count()
            .saturating_sub(1);
        let old_tip = self.tip().id();
        let new_tip = validated.tip().id();
        *self = validated;
        Ok(ChainEvent::Reorganized {
            old_tip,
            new_tip,
            fork_height: u64::try_from(fork_index).expect("height fits u64"),
        })
    }

    pub fn snapshot(&self) -> ChainSnapshot {
        ChainSnapshot {
            height: self.height(),
            tip: self.tip().id(),
            cumulative_work: format!("{:064x}", self.cumulative_work),
            state_root: self.state.root(),
        }
    }
}
