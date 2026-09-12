use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};

use primitive_types::U256;
use thiserror::Error;

use crate::{
    codec::{CodecError, ConsensusEncode},
    crypto::{domain_hash, parse_and_validate_signature, CryptoError, Hash32},
};

use super::{
    bits_to_target, pow_limit, randomx_seed_reference_height, required_bits, reward_allocation,
    target_work, workshare_bits, Block, BlockHeader, BlockTemplate, LedgerState, RewardAllocation,
    RewardClaim, Transaction, WorkshareWitness, CHAIN_ID, HEADER_VERSION, MAX_BLOCK_BYTES,
    MAX_BLOCK_TRANSACTIONS, MAX_FUTURE_BLOCK_TIME, MAX_MONEY, MAX_TEMPLATE_BYTES,
    MAX_TRANSACTION_BYTES, MAX_WORKSHARE_WITNESS_BYTES, MEDIAN_TIME_SPAN, PROTOCOL_VERSION,
    RANDOMX_BOOTSTRAP_SEED, REWARD_MATURITY,
};

#[derive(Debug, Error)]
pub enum ConsensusError {
    #[error("codec error: {0}")]
    Codec(#[from] CodecError),
    #[error("cryptographic validation failed: {0}")]
    Crypto(#[from] CryptoError),
    #[error("block is not the fixed Protocol 9 genesis")]
    InvalidGenesis,
    #[error("unsupported protocol or object version")]
    UnsupportedVersion,
    #[error("block does not extend the supplied parent")]
    WrongParent,
    #[error("block height is not parent height + 1")]
    WrongHeight,
    #[error("timestamp violates median-time-past or monotonicity")]
    InvalidTimestamp,
    #[error("timestamp is too far in the future")]
    FutureTimestamp,
    #[error("target is invalid, non-canonical, or unexpected")]
    InvalidTarget,
    #[error("proof of work does not meet the target")]
    InvalidProofOfWork,
    #[error("RandomX validation is unavailable: {0}")]
    PowUnavailable(String),
    #[error("serialized object exceeds a consensus size limit")]
    SizeLimit,
    #[error("transaction root mismatch")]
    TransactionRootMismatch,
    #[error("state root mismatch")]
    StateRootMismatch,
    #[error("workshare witness commitment mismatch")]
    WorkshareRootMismatch,
    #[error("workshare witness is not canonical")]
    NonCanonicalWitness,
    #[error("workshare chain or template reference is invalid")]
    InvalidWorkshareChain,
    #[error("duplicate transaction")]
    DuplicateTransaction,
    #[error("transaction has an invalid amount, fee, or self-transfer")]
    InvalidTransaction,
    #[error("transaction nonce does not match account state")]
    InvalidNonce,
    #[error("insufficient balance")]
    InsufficientBalance,
    #[error("monetary value is out of range")]
    MoneyRange,
    #[error("reward claim or allocation does not match consensus")]
    RewardMismatch,
    #[error("integer arithmetic overflow")]
    ArithmeticOverflow,
    #[error("candidate chain has no common genesis")]
    DifferentGenesis,
    #[error("candidate chain is not preferred by cumulative L1 work")]
    InsufficientWork,
    #[error("stored chain data is inconsistent: {0}")]
    StorageInvariant(String),
}

pub trait PowVerifier: Send + Sync {
    fn hash(&self, seed: &[u8], input: &[u8]) -> Result<[u8; 32], String>;
}

pub trait TimeSource: Send + Sync {
    fn unix_time(&self) -> u64;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemTimeSource;

impl TimeSource for SystemTimeSource {
    fn unix_time(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}

pub fn validate_genesis(block: &Block) -> Result<(), ConsensusError> {
    if block == &Block::genesis() {
        Ok(())
    } else {
        Err(ConsensusError::InvalidGenesis)
    }
}

pub fn validate_block(
    block: &Block,
    parent_history: &[Block],
    parent_state: &LedgerState,
    pow: &dyn PowVerifier,
    time: &dyn TimeSource,
) -> Result<LedgerState, ConsensusError> {
    if block.header.height == 0 {
        validate_genesis(block)?;
        return Ok(LedgerState::default());
    }
    let parent = parent_history.last().ok_or(ConsensusError::WrongParent)?;
    validate_header_context(&block.header, parent, parent_history, time)?;
    if block.consensus_encode().len() > MAX_BLOCK_BYTES
        || block.transactions.len() > MAX_BLOCK_TRANSACTIONS
    {
        return Err(ConsensusError::SizeLimit);
    }
    if block.transaction_root() != block.header.transactions_root {
        return Err(ConsensusError::TransactionRootMismatch);
    }
    validate_witness_shape(&block.workshare_witness)?;
    if block.workshare_witness.root() != block.header.workshare_root {
        return Err(ConsensusError::WorkshareRootMismatch);
    }

    let seed = randomx_seed(parent_history, block.header.height)?;
    let target = bits_to_target(block.header.bits).ok_or(ConsensusError::InvalidTarget)?;
    verify_pow(pow, &seed, &block.header, target)?;

    let claim = block
        .reward_claim
        .as_ref()
        .ok_or(ConsensusError::RewardMismatch)?;
    if claim.version != RewardClaim::VERSION || claim.height != block.header.height {
        return Err(ConsensusError::RewardMismatch);
    }
    let (post_state, allocation, _) = transition_for_candidate(
        block.header.height,
        claim,
        &block.transactions,
        parent_history,
        parent_state,
    )?;
    if claim.amount != allocation.finder_amount {
        return Err(ConsensusError::RewardMismatch);
    }
    if post_state.root() != block.header.state_root {
        return Err(ConsensusError::StateRootMismatch);
    }

    validate_workshares(
        &block.workshare_witness,
        parent_history,
        parent_state,
        pow,
        block.header.timestamp,
    )?;
    Ok(post_state)
}

pub(crate) fn transition_for_candidate(
    height: u64,
    claim: &RewardClaim,
    transactions: &[Transaction],
    parent_history: &[Block],
    parent_state: &LedgerState,
) -> Result<(LedgerState, RewardAllocation, u64), ConsensusError> {
    let mut state = parent_state.clone();
    state.mature(height)?;
    let fees = apply_transactions(&mut state, transactions)?;
    let allocation = reward_allocation(height, claim.finder, fees, parent_history)?;
    if claim.amount != allocation.finder_amount {
        return Err(ConsensusError::RewardMismatch);
    }
    let maturity = height
        .checked_add(REWARD_MATURITY)
        .ok_or(ConsensusError::ArithmeticOverflow)?;
    state.schedule_reward(maturity, claim.finder, allocation.finder_amount)?;
    for (recipient, amount) in &allocation.workshare_rewards {
        state.schedule_reward(maturity, *recipient, *amount)?;
    }
    Ok((state, allocation, fees))
}

fn apply_transactions(
    state: &mut LedgerState,
    transactions: &[Transaction],
) -> Result<u64, ConsensusError> {
    let mut seen = std::collections::BTreeSet::new();
    let mut total_fees = 0_u64;
    for transaction in transactions {
        validate_transaction_structure(transaction)?;
        if !seen.insert(transaction.id()) {
            return Err(ConsensusError::DuplicateTransaction);
        }
        let account = state.account(transaction.sender);
        if transaction.nonce != account.nonce {
            return Err(ConsensusError::InvalidNonce);
        }
        let debit = transaction
            .amount
            .checked_add(transaction.fee)
            .ok_or(ConsensusError::MoneyRange)?;
        if debit > account.balance {
            return Err(ConsensusError::InsufficientBalance);
        }
        let signature: [u8; 64] = transaction
            .signature
            .as_slice()
            .try_into()
            .map_err(|_| ConsensusError::InvalidTransaction)?;
        parse_and_validate_signature(
            transaction.sender,
            &transaction.public_key,
            &signature,
            transaction.signature_hash(),
        )?;
        state.debit(transaction.sender, debit)?;
        state.credit(transaction.recipient, transaction.amount)?;
        state.set_nonce(
            transaction.sender,
            transaction
                .nonce
                .checked_add(1)
                .ok_or(ConsensusError::ArithmeticOverflow)?,
        );
        total_fees = total_fees
            .checked_add(transaction.fee)
            .filter(|fees| *fees <= MAX_MONEY)
            .ok_or(ConsensusError::MoneyRange)?;
    }
    Ok(total_fees)
}

pub(crate) fn validate_template_transactions(
    transactions: &[Transaction],
    parent_state: &LedgerState,
    height: u64,
) -> Result<(), ConsensusError> {
    if transactions.len() > MAX_BLOCK_TRANSACTIONS {
        return Err(ConsensusError::SizeLimit);
    }
    let mut state = parent_state.clone();
    state.mature(height)?;
    apply_transactions(&mut state, transactions)?;
    Ok(())
}

fn validate_transaction_structure(transaction: &Transaction) -> Result<(), ConsensusError> {
    if transaction.version != Transaction::VERSION
        || transaction.amount == 0
        || transaction.amount > MAX_MONEY
        || transaction.fee > MAX_MONEY
        || transaction.sender == transaction.recipient
        || !matches!(transaction.public_key.len(), 33 | 65)
        || transaction.signature.len() != 64
        || transaction.consensus_encode().len() > MAX_TRANSACTION_BYTES
    {
        return Err(ConsensusError::InvalidTransaction);
    }
    Ok(())
}

fn validate_header_context(
    header: &BlockHeader,
    parent: &Block,
    parent_history: &[Block],
    time: &dyn TimeSource,
) -> Result<(), ConsensusError> {
    if header.version != HEADER_VERSION || header.protocol != PROTOCOL_VERSION {
        return Err(ConsensusError::UnsupportedVersion);
    }
    if header.height != parent.header.height + 1 {
        return Err(ConsensusError::WrongHeight);
    }
    if header.previous_block != parent.id() {
        return Err(ConsensusError::WrongParent);
    }
    if header.bits != required_bits(parent.header.height, parent.header.timestamp)
        || bits_to_target(header.bits).is_none_or(|target| target > pow_limit())
    {
        return Err(ConsensusError::InvalidTarget);
    }
    if header.timestamp <= parent.header.timestamp
        || header.timestamp <= median_time_past(parent_history)
    {
        return Err(ConsensusError::InvalidTimestamp);
    }
    if header.timestamp > time.unix_time().saturating_add(MAX_FUTURE_BLOCK_TIME) {
        return Err(ConsensusError::FutureTimestamp);
    }
    Ok(())
}

fn validate_witness_shape(witness: &WorkshareWitness) -> Result<(), ConsensusError> {
    if witness.version != WorkshareWitness::VERSION
        || witness.consensus_encode().len() > MAX_WORKSHARE_WITNESS_BYTES
        || !witness.is_canonical()
    {
        return Err(ConsensusError::NonCanonicalWitness);
    }
    if witness.templates.iter().any(|template| {
        template.version != BlockTemplate::VERSION
            || template.consensus_encode().len() > MAX_TEMPLATE_BYTES
    }) {
        return Err(ConsensusError::NonCanonicalWitness);
    }
    Ok(())
}

pub(crate) fn validate_workshares(
    witness: &WorkshareWitness,
    parent_history: &[Block],
    parent_state: &LedgerState,
    pow: &dyn PowVerifier,
    settling_timestamp: u64,
) -> Result<(), ConsensusError> {
    if witness.workshares.is_empty() {
        return Ok(());
    }
    let parent = parent_history.last().ok_or(ConsensusError::WrongParent)?;
    let templates: BTreeMap<_, _> = witness
        .templates
        .iter()
        .map(|template| (template.id(), template))
        .collect();
    let seed = randomx_seed(parent_history, parent.header.height + 1)?;
    let expected_bits = required_bits(parent.header.height, parent.header.timestamp);
    let block_target = bits_to_target(expected_bits).ok_or(ConsensusError::InvalidTarget)?;
    let share_bits = workshare_bits(expected_bits).ok_or(ConsensusError::InvalidTarget)?;
    let share_target = bits_to_target(share_bits).ok_or(ConsensusError::InvalidTarget)?;
    let mtp = median_time_past(parent_history);
    let mut previous = Hash32::ZERO;
    let mut prefix = WorkshareWitness::empty();

    for share in &witness.workshares {
        if share.version != super::Workshare::VERSION
            || share.previous_workshare != previous
            || share.candidate_header.version != HEADER_VERSION
            || share.candidate_header.protocol != PROTOCOL_VERSION
            || share.candidate_header.height != parent.header.height + 1
            || share.candidate_header.previous_block != parent.id()
            || share.candidate_header.bits != expected_bits
            || share.candidate_header.timestamp <= parent.header.timestamp
            || share.candidate_header.timestamp <= mtp
            || share.candidate_header.timestamp > settling_timestamp
        {
            return Err(ConsensusError::InvalidWorkshareChain);
        }
        let template = templates
            .get(&share.template_id)
            .ok_or(ConsensusError::InvalidWorkshareChain)?;
        if template.version != BlockTemplate::VERSION
            || template.height != parent.header.height + 1
            || template.parent != parent.id()
        {
            return Err(ConsensusError::InvalidWorkshareChain);
        }
        let fees = template
            .transactions
            .iter()
            .try_fold(0_u64, |sum, transaction| {
                sum.checked_add(transaction.fee)
                    .ok_or(ConsensusError::MoneyRange)
            })?;
        let allocation = reward_allocation(template.height, share.miner, fees, parent_history)?;
        let claim = RewardClaim {
            version: RewardClaim::VERSION,
            height: template.height,
            finder: share.miner,
            amount: allocation.finder_amount,
            extranonce: share.extranonce,
        };
        let (state, _, _) = transition_for_candidate(
            template.height,
            &claim,
            &template.transactions,
            parent_history,
            parent_state,
        )?;
        let mut txids = Vec::with_capacity(template.transactions.len() + 1);
        txids.push(claim.id());
        txids.extend(template.transactions.iter().map(Transaction::id));
        if share.candidate_header.transactions_root != super::merkle_root(&txids)
            || share.candidate_header.state_root != state.root()
            || share.candidate_header.workshare_root != prefix.root()
        {
            return Err(ConsensusError::InvalidWorkshareChain);
        }
        let pow_hash = pow
            .hash(&seed, &share.candidate_header.consensus_encode())
            .map_err(ConsensusError::PowUnavailable)?;
        // A full-target solution is a block, not a reward-bearing Workshare.
        // Enforcing the open lower bound avoids claiming the same work twice.
        if !super::difficulty::hash_meets_target(pow_hash, share_target)
            || super::difficulty::hash_meets_target(pow_hash, block_target)
        {
            return Err(ConsensusError::InvalidProofOfWork);
        }

        previous = share.id();
        prefix.workshares.push(share.clone());
        if !prefix
            .templates
            .iter()
            .any(|existing| existing.id() == template.id())
        {
            prefix.templates.push((*template).clone());
            prefix.templates.sort_by_key(BlockTemplate::id);
        }
    }
    Ok(())
}

fn verify_pow(
    pow: &dyn PowVerifier,
    seed: &[u8],
    header: &BlockHeader,
    target: U256,
) -> Result<(), ConsensusError> {
    let hash = pow
        .hash(seed, &header.consensus_encode())
        .map_err(ConsensusError::PowUnavailable)?;
    if super::difficulty::hash_meets_target(hash, target) {
        Ok(())
    } else {
        Err(ConsensusError::InvalidProofOfWork)
    }
}

pub fn randomx_seed(parent_history: &[Block], height: u64) -> Result<Vec<u8>, ConsensusError> {
    if height < super::RANDOMX_EPOCH_BLOCKS {
        return Ok(RANDOMX_BOOTSTRAP_SEED.to_vec());
    }
    let reference_height = randomx_seed_reference_height(height);
    let reference = parent_history
        .iter()
        .find(|block| block.header.height == reference_height)
        .ok_or_else(|| {
            ConsensusError::StorageInvariant(format!(
                "RandomX reference block {reference_height} unavailable"
            ))
        })?;
    let mut material = crate::codec::Writer::new();
    material.bytes(CHAIN_ID.as_bytes());
    material.u64(height / super::RANDOMX_EPOCH_BLOCKS);
    reference.id().encode_to(&mut material);
    Ok(
        domain_hash(b"PARITR-P9-RANDOMX-EPOCH-v1", &material.into_inner())
            .0
            .to_vec(),
    )
}

pub fn median_time_past(history: &[Block]) -> u64 {
    let start = history.len().saturating_sub(MEDIAN_TIME_SPAN);
    let mut timestamps: Vec<_> = history[start..]
        .iter()
        .map(|block| block.header.timestamp)
        .collect();
    timestamps.sort_unstable();
    timestamps
        .get(timestamps.len() / 2)
        .copied()
        .unwrap_or_default()
}

pub fn cumulative_work(blocks: &[Block]) -> Result<U256, ConsensusError> {
    blocks.iter().try_fold(U256::zero(), |total, block| {
        let target = bits_to_target(block.header.bits).ok_or(ConsensusError::InvalidTarget)?;
        total
            .checked_add(target_work(target))
            .ok_or(ConsensusError::ArithmeticOverflow)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        codec::ConsensusEncode,
        consensus::{
            merkle_root, Chain, ChainEvent, Workshare, WorkshareWitness, GENESIS_TIMESTAMP,
            INITIAL_SUBSIDY, TARGET_BLOCK_TIME,
        },
        crypto::Address,
    };
    use primitive_types::U256;
    use secp256k1::{PublicKey, Secp256k1, SecretKey};

    struct FixedPow([u8; 32]);

    impl PowVerifier for FixedPow {
        fn hash(&self, _seed: &[u8], _input: &[u8]) -> Result<[u8; 32], String> {
            Ok(self.0)
        }
    }

    struct FixedTime;

    impl TimeSource for FixedTime {
        fn unix_time(&self) -> u64 {
            u64::MAX / 2
        }
    }

    fn address(byte: u8) -> Address {
        let secret = SecretKey::from_slice(&[byte; 32]).expect("valid test secret");
        Address::from_public_key(&PublicKey::from_secret_key(&Secp256k1::new(), &secret))
    }

    fn next_block(
        history: &[Block],
        state: &LedgerState,
        finder: Address,
        nonce: u64,
    ) -> (Block, LedgerState) {
        let parent = history.last().expect("history contains genesis");
        let height = parent.header.height + 1;
        let allocation = reward_allocation(height, finder, 0, history).expect("test allocation");
        let claim = RewardClaim {
            version: RewardClaim::VERSION,
            height,
            finder,
            amount: allocation.finder_amount,
            extranonce: height,
        };
        let (post_state, _, _) =
            transition_for_candidate(height, &claim, &[], history, state).expect("test transition");
        let witness = WorkshareWitness::empty();
        let block = Block {
            header: BlockHeader {
                version: HEADER_VERSION,
                protocol: PROTOCOL_VERSION,
                height,
                previous_block: parent.id(),
                transactions_root: merkle_root(&[claim.id()]),
                state_root: post_state.root(),
                workshare_root: witness.root(),
                timestamp: GENESIS_TIMESTAMP + height * TARGET_BLOCK_TIME,
                bits: required_bits(parent.header.height, parent.header.timestamp),
                nonce,
            },
            reward_claim: Some(claim),
            transactions: Vec::new(),
            workshare_witness: witness,
        };
        (block, post_state)
    }

    fn chain_with_blocks(count: u64, finder: Address, nonce: u64) -> Vec<Block> {
        let mut blocks = vec![Block::genesis()];
        let mut state = LedgerState::default();
        for _ in 0..count {
            let (block, next_state) = next_block(&blocks, &state, finder, nonce);
            blocks.push(block);
            state = next_state;
        }
        blocks
    }

    #[test]
    fn rewards_are_committed_then_mature_exactly_at_height() {
        let finder = address(1);
        let blocks = chain_with_blocks(REWARD_MATURITY + 1, finder, 0);
        let chain = Chain::from_blocks(blocks, &FixedPow([0; 32]), &FixedTime).unwrap();
        assert_eq!(chain.state().account(finder).balance, INITIAL_SUBSIDY);
        assert_eq!(
            chain.state().pending_for(finder),
            INITIAL_SUBSIDY * REWARD_MATURITY
        );
        assert_eq!(chain.tip().header.state_root, chain.state().root());
    }

    #[test]
    fn heavier_valid_chain_reorganizes_beyond_one_hundred_blocks() {
        let mut active = Chain::from_blocks(
            chain_with_blocks(101, address(2), 0),
            &FixedPow([0; 32]),
            &FixedTime,
        )
        .unwrap();
        let candidate = chain_with_blocks(102, address(3), 1);
        let event = active
            .consider_chain(candidate, &FixedPow([0; 32]), &FixedTime)
            .unwrap();
        assert!(matches!(
            event,
            ChainEvent::Reorganized { fork_height: 0, .. }
        ));
        assert_eq!(active.height(), 102);
    }

    fn one_workshare_witness(miner: Address) -> WorkshareWitness {
        let history = vec![Block::genesis()];
        let parent = history.last().unwrap();
        let template = BlockTemplate {
            version: BlockTemplate::VERSION,
            height: 1,
            parent: parent.id(),
            transactions: Vec::new(),
        };
        let allocation = reward_allocation(1, miner, 0, &history).unwrap();
        let claim = RewardClaim {
            version: RewardClaim::VERSION,
            height: 1,
            finder: miner,
            amount: allocation.finder_amount,
            extranonce: 7,
        };
        let (post_state, _, _) =
            transition_for_candidate(1, &claim, &[], &history, &LedgerState::default()).unwrap();
        let prefix = WorkshareWitness::empty();
        let share = Workshare {
            version: Workshare::VERSION,
            previous_workshare: Hash32::ZERO,
            template_id: template.id(),
            miner,
            extranonce: 7,
            candidate_header: BlockHeader {
                version: HEADER_VERSION,
                protocol: PROTOCOL_VERSION,
                height: 1,
                previous_block: parent.id(),
                transactions_root: merkle_root(&[claim.id()]),
                state_root: post_state.root(),
                workshare_root: prefix.root(),
                timestamp: GENESIS_TIMESTAMP + 1,
                bits: required_bits(0, GENESIS_TIMESTAMP),
                nonce: 9,
            },
        };
        WorkshareWitness {
            version: WorkshareWitness::VERSION,
            templates: vec![template],
            workshares: vec![share],
        }
    }

    #[test]
    fn full_block_solution_cannot_also_be_claimed_as_workshare() {
        let history = vec![Block::genesis()];
        let witness = one_workshare_witness(address(4));
        assert!(matches!(
            validate_workshares(
                &witness,
                &history,
                &LedgerState::default(),
                &FixedPow([0; 32]),
                GENESIS_TIMESTAMP + 1,
            ),
            Err(ConsensusError::InvalidProofOfWork)
        ));
    }

    #[test]
    fn hash_strictly_between_targets_is_a_valid_workshare() {
        let history = vec![Block::genesis()];
        let witness = one_workshare_witness(address(5));
        let block_target = bits_to_target(required_bits(0, GENESIS_TIMESTAMP)).unwrap();
        let share_target =
            bits_to_target(workshare_bits(required_bits(0, GENESIS_TIMESTAMP)).unwrap()).unwrap();
        let share_hash = (block_target + U256::one()).to_little_endian();
        assert!(U256::from_little_endian(&share_hash) <= share_target);
        validate_workshares(
            &witness,
            &history,
            &LedgerState::default(),
            &FixedPow(share_hash),
            GENESIS_TIMESTAMP + 1,
        )
        .unwrap();
    }

    #[test]
    fn header_hash_input_is_exact_consensus_bytes() {
        let (block, _) = next_block(&[Block::genesis()], &LedgerState::default(), address(6), 0);
        assert_eq!(
            block.header.consensus_encode().len(),
            BlockHeader::ENCODED_LEN
        );
    }
}
