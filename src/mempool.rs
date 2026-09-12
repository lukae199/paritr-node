use std::collections::{BTreeMap, HashMap, HashSet};

use thiserror::Error;

use crate::{
    codec::ConsensusEncode,
    consensus::{LedgerState, Transaction, MAX_MONEY},
    crypto::{parse_and_validate_signature, Address, Hash32},
    policy::{RelayPolicy, RBF_PERCENT},
};

#[derive(Clone, Debug)]
struct Entry {
    transaction: Transaction,
    received_at: u64,
    size: usize,
}

#[derive(Debug, Error)]
pub enum MempoolError {
    #[error("transaction does not satisfy local relay policy")]
    Policy,
    #[error("invalid transaction structure or signature")]
    Invalid,
    #[error("nonce is stale or too far ahead")]
    Nonce,
    #[error("sender cannot fund the queued transaction sequence")]
    Balance,
    #[error("replacement fee increase is too small")]
    ReplacementFee,
}

#[derive(Debug)]
pub struct Mempool {
    policy: RelayPolicy,
    entries: HashMap<Hash32, Entry>,
    by_sender_nonce: BTreeMap<(Address, u64), Hash32>,
    bytes: usize,
}

impl Mempool {
    pub fn new(policy: RelayPolicy) -> Self {
        Self {
            policy,
            entries: HashMap::new(),
            by_sender_nonce: BTreeMap::new(),
            bytes: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn contains(&self, id: Hash32) -> bool {
        self.entries.contains_key(&id)
    }

    pub fn get(&self, id: Hash32) -> Option<Transaction> {
        self.entries.get(&id).map(|entry| entry.transaction.clone())
    }

    pub fn accept(
        &mut self,
        transaction: Transaction,
        state: &LedgerState,
        now: u64,
    ) -> Result<Hash32, MempoolError> {
        validate_for_relay(&transaction, &self.policy, state)?;
        let id = transaction.id();
        if self.entries.contains_key(&id) {
            return Ok(id);
        }
        let slot = (transaction.sender, transaction.nonce);
        if let Some(existing_id) = self.by_sender_nonce.get(&slot).copied() {
            let existing = &self.entries[&existing_id].transaction;
            let required = existing.fee.saturating_add(
                self.policy
                    .min_relay_fee
                    .max(existing.fee.saturating_mul(RBF_PERCENT).saturating_add(99) / 100),
            );
            if transaction.fee < required {
                return Err(MempoolError::ReplacementFee);
            }
            self.remove(existing_id);
        }

        let size = transaction.consensus_encode().len();
        self.bytes += size;
        self.by_sender_nonce.insert(slot, id);
        self.entries.insert(
            id,
            Entry {
                transaction,
                received_at: now,
                size,
            },
        );
        if !self.sender_is_funded(slot.0, state) {
            self.remove(id);
            return Err(MempoolError::Balance);
        }
        self.expire_and_trim(now);
        Ok(id)
    }

    pub fn remove_confirmed(&mut self, transactions: &[Transaction]) {
        for transaction in transactions {
            self.remove(transaction.id());
        }
    }

    pub fn select(&self, state: &LedgerState, maximum: usize) -> Vec<Transaction> {
        let mut working = state.clone();
        let mut selected = Vec::new();
        let mut included = HashSet::new();
        loop {
            let mut candidates: Vec<_> = self
                .entries
                .iter()
                .filter(|(id, entry)| {
                    !included.contains(*id)
                        && working.account(entry.transaction.sender).nonce
                            == entry.transaction.nonce
                        && working.account(entry.transaction.sender).balance
                            >= entry
                                .transaction
                                .amount
                                .saturating_add(entry.transaction.fee)
                })
                .collect();
            candidates.sort_by(|(left_id, left), (right_id, right)| {
                let left_rate = u128::from(left.transaction.fee) * right.size as u128;
                let right_rate = u128::from(right.transaction.fee) * left.size as u128;
                right_rate
                    .cmp(&left_rate)
                    .then_with(|| left_id.cmp(right_id))
            });
            let Some((id, entry)) = candidates.first() else {
                break;
            };
            let transaction = &entry.transaction;
            let debit = transaction.amount + transaction.fee;
            // These operations cannot fail after the eligibility checks.
            working
                .debit(transaction.sender, debit)
                .expect("checked debit");
            working
                .credit(transaction.recipient, transaction.amount)
                .expect("checked credit");
            working.set_nonce(transaction.sender, transaction.nonce + 1);
            included.insert(**id);
            selected.push(transaction.clone());
            if selected.len() >= maximum {
                break;
            }
        }
        selected
    }

    pub fn expire_and_trim(&mut self, now: u64) {
        let expired: Vec<_> = self
            .entries
            .iter()
            .filter_map(|(id, entry)| {
                (now.saturating_sub(entry.received_at) > self.policy.ttl_seconds).then_some(*id)
            })
            .collect();
        for id in expired {
            self.remove(id);
        }
        while self.bytes > self.policy.max_mempool_bytes {
            let lowest = self
                .entries
                .iter()
                .min_by(|(left_id, left), (right_id, right)| {
                    let left_rate = u128::from(left.transaction.fee) * right.size as u128;
                    let right_rate = u128::from(right.transaction.fee) * left.size as u128;
                    left_rate
                        .cmp(&right_rate)
                        .then_with(|| left_id.cmp(right_id))
                })
                .map(|(id, _)| *id);
            if let Some(id) = lowest {
                self.remove(id);
            } else {
                break;
            }
        }
    }

    fn sender_is_funded(&self, sender: Address, state: &LedgerState) -> bool {
        let account = state.account(sender);
        let mut balance = account.balance;
        for (expected_nonce, ((entry_sender, nonce), id)) in (account.nonce..).zip(
            self.by_sender_nonce
                .range((sender, account.nonce)..=(sender, u64::MAX)),
        ) {
            if *entry_sender != sender || *nonce != expected_nonce {
                break;
            }
            let transaction = &self.entries[id].transaction;
            let Some(needed) = transaction.amount.checked_add(transaction.fee) else {
                return false;
            };
            if needed > balance {
                return false;
            }
            balance -= needed;
        }
        true
    }

    fn remove(&mut self, id: Hash32) {
        if let Some(entry) = self.entries.remove(&id) {
            self.bytes = self.bytes.saturating_sub(entry.size);
            self.by_sender_nonce
                .remove(&(entry.transaction.sender, entry.transaction.nonce));
        }
    }
}

fn validate_for_relay(
    transaction: &Transaction,
    policy: &RelayPolicy,
    state: &LedgerState,
) -> Result<(), MempoolError> {
    if transaction.version != Transaction::VERSION
        || transaction.amount == 0
        || transaction.amount > MAX_MONEY
        || transaction.fee > MAX_MONEY
        || transaction.nonce == u64::MAX
        || transaction.amount < policy.dust_limit
        || transaction.fee < policy.min_relay_fee
        || transaction.sender == transaction.recipient
        || !matches!(transaction.public_key.len(), 33 | 65)
        || transaction.signature.len() != 64
    {
        return Err(MempoolError::Policy);
    }
    let account = state.account(transaction.sender);
    if transaction.nonce < account.nonce
        || transaction.nonce > account.nonce.saturating_add(policy.max_nonce_gap)
    {
        return Err(MempoolError::Nonce);
    }
    let signature: [u8; 64] = transaction
        .signature
        .as_slice()
        .try_into()
        .map_err(|_| MempoolError::Invalid)?;
    parse_and_validate_signature(
        transaction.sender,
        &transaction.public_key,
        &signature,
        transaction.signature_hash(),
    )
    .map_err(|_| MempoolError::Invalid)
}
