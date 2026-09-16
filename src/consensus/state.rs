use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::{
    codec::{CodecError, ConsensusDecode, ConsensusEncode, Reader, Writer},
    crypto::{domain_hash, Address, Hash32},
};

use super::{ConsensusError, MAX_MONEY};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Account {
    pub balance: u64,
    pub nonce: u64,
}

impl ConsensusEncode for Account {
    fn encode_to(&self, writer: &mut Writer) {
        writer.u64(self.balance);
        writer.u64(self.nonce);
    }
}

impl ConsensusDecode for Account {
    fn decode_from(reader: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            balance: reader.u64()?,
            nonce: reader.u64()?,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PendingReward {
    pub maturity_height: u64,
    pub recipient: Address,
    pub amount: u64,
}

impl ConsensusEncode for PendingReward {
    fn encode_to(&self, writer: &mut Writer) {
        writer.u64(self.maturity_height);
        self.recipient.encode_to(writer);
        writer.u64(self.amount);
    }
}

impl ConsensusDecode for PendingReward {
    fn decode_from(reader: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            maturity_height: reader.u64()?,
            recipient: Address::decode_from(reader)?,
            amount: reader.u64()?,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum StateKey {
    Account(Address),
    PendingReward { maturity: u64, recipient: Address },
}

impl StateKey {
    pub fn hash(self) -> Hash32 {
        let mut writer = Writer::new();
        match self {
            Self::Account(address) => {
                writer.u8(0);
                address.encode_to(&mut writer);
            }
            Self::PendingReward {
                maturity,
                recipient,
            } => {
                writer.u8(1);
                writer.u64(maturity);
                recipient.encode_to(&mut writer);
            }
        }
        domain_hash(b"PARITR-P10-STATE-KEY-v1", &writer.into_inner())
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct NodeIndex {
    depth: u16,
    prefix: [u8; 32],
}

/// A real 256-level sparse Merkle tree with cached inner nodes.
///
/// Updating one account or maturity entry recalculates 256 paths, independent
/// of the total account count. Empty hashes are depth-specific and fixed.
#[derive(Clone, Debug)]
pub struct SparseMerkleTree {
    values: BTreeMap<Hash32, Vec<u8>>,
    nodes: HashMap<NodeIndex, Hash32>,
    empty: Vec<Hash32>,
}

impl Default for SparseMerkleTree {
    fn default() -> Self {
        let mut empty = vec![Hash32::ZERO; 257];
        empty[256] = domain_hash(b"PARITR-P10-SMT-EMPTY-LEAF-v1", &[]);
        for depth in (0..256).rev() {
            let mut children = [0_u8; 64];
            children[..32].copy_from_slice(empty[depth + 1].as_bytes());
            children[32..].copy_from_slice(empty[depth + 1].as_bytes());
            empty[depth] = domain_hash(b"PARITR-P10-SMT-NODE-v1", &children);
        }
        Self {
            values: BTreeMap::new(),
            nodes: HashMap::new(),
            empty,
        }
    }
}

impl SparseMerkleTree {
    pub fn root(&self) -> Hash32 {
        self.nodes
            .get(&NodeIndex {
                depth: 0,
                prefix: [0; 32],
            })
            .copied()
            .unwrap_or(self.empty[0])
    }

    pub fn get(&self, key: Hash32) -> Option<&[u8]> {
        self.values.get(&key).map(Vec::as_slice)
    }

    pub fn update(&mut self, key: Hash32, value: Option<Vec<u8>>) {
        let leaf_index = NodeIndex {
            depth: 256,
            prefix: key.0,
        };
        match value {
            Some(value) if !value.is_empty() => {
                let mut leaf_data = Vec::with_capacity(32 + value.len());
                leaf_data.extend_from_slice(key.as_bytes());
                leaf_data.extend_from_slice(&value);
                self.values.insert(key, value);
                self.nodes.insert(
                    leaf_index,
                    domain_hash(b"PARITR-P10-SMT-LEAF-v1", &leaf_data),
                );
            }
            _ => {
                self.values.remove(&key);
                self.nodes.remove(&leaf_index);
            }
        }

        for parent_depth in (0_u16..256).rev() {
            let child_depth = parent_depth + 1;
            let child_prefix = prefix(key.0, child_depth);
            let mut sibling_prefix = child_prefix;
            toggle_bit(&mut sibling_prefix, usize::from(parent_depth));
            let child = self.node(child_depth, child_prefix);
            let sibling = self.node(child_depth, sibling_prefix);
            let (left, right) = if bit(key.0, usize::from(parent_depth)) == 0 {
                (child, sibling)
            } else {
                (sibling, child)
            };
            let mut children = [0_u8; 64];
            children[..32].copy_from_slice(left.as_bytes());
            children[32..].copy_from_slice(right.as_bytes());
            let hash = domain_hash(b"PARITR-P10-SMT-NODE-v1", &children);
            let parent_index = NodeIndex {
                depth: parent_depth,
                prefix: prefix(key.0, parent_depth),
            };
            if hash == self.empty[usize::from(parent_depth)] {
                self.nodes.remove(&parent_index);
            } else {
                self.nodes.insert(parent_index, hash);
            }
        }
    }

    fn node(&self, depth: u16, prefix: [u8; 32]) -> Hash32 {
        self.nodes
            .get(&NodeIndex { depth, prefix })
            .copied()
            .unwrap_or(self.empty[usize::from(depth)])
    }
}

fn bit(key: [u8; 32], depth: usize) -> u8 {
    (key[depth / 8] >> (7 - depth % 8)) & 1
}

fn toggle_bit(key: &mut [u8; 32], depth: usize) {
    key[depth / 8] ^= 1 << (7 - depth % 8);
}

fn prefix(mut key: [u8; 32], depth: u16) -> [u8; 32] {
    let depth = usize::from(depth);
    let full_bytes = depth / 8;
    let remaining = depth % 8;
    if full_bytes < 32 {
        if remaining == 0 {
            key[full_bytes..].fill(0);
        } else {
            key[full_bytes] &= 0xff << (8 - remaining);
            key[full_bytes + 1..].fill(0);
        }
    }
    key
}

#[derive(Clone, Debug, Default)]
pub struct LedgerState {
    accounts: BTreeMap<Address, Account>,
    pending_rewards: BTreeMap<(u64, Address), u64>,
    tree: SparseMerkleTree,
}

impl LedgerState {
    pub const SNAPSHOT_VERSION: u16 = 1;

    pub fn root(&self) -> Hash32 {
        self.tree.root()
    }

    pub fn account(&self, address: Address) -> Account {
        self.accounts.get(&address).copied().unwrap_or_default()
    }

    pub fn accounts(&self) -> &BTreeMap<Address, Account> {
        &self.accounts
    }

    pub fn pending_rewards(&self) -> &BTreeMap<(u64, Address), u64> {
        &self.pending_rewards
    }

    pub fn pending_for(&self, address: Address) -> u64 {
        self.pending_rewards
            .iter()
            .filter(|((_, recipient), _)| *recipient == address)
            .map(|(_, amount)| *amount)
            .fold(0_u64, u64::saturating_add)
    }

    pub fn credit(&mut self, address: Address, amount: u64) -> Result<(), ConsensusError> {
        let mut account = self.account(address);
        account.balance = account
            .balance
            .checked_add(amount)
            .filter(|balance| *balance <= MAX_MONEY)
            .ok_or(ConsensusError::MoneyRange)?;
        self.set_account(address, account);
        Ok(())
    }

    pub fn debit(&mut self, address: Address, amount: u64) -> Result<(), ConsensusError> {
        let mut account = self.account(address);
        account.balance = account
            .balance
            .checked_sub(amount)
            .ok_or(ConsensusError::InsufficientBalance)?;
        self.set_account(address, account);
        Ok(())
    }

    pub fn set_nonce(&mut self, address: Address, nonce: u64) {
        let mut account = self.account(address);
        account.nonce = nonce;
        self.set_account(address, account);
    }

    pub fn schedule_reward(
        &mut self,
        maturity: u64,
        recipient: Address,
        amount: u64,
    ) -> Result<(), ConsensusError> {
        if amount == 0 {
            return Ok(());
        }
        let key = (maturity, recipient);
        let next = self
            .pending_rewards
            .get(&key)
            .copied()
            .unwrap_or(0)
            .checked_add(amount)
            .filter(|amount| *amount <= MAX_MONEY)
            .ok_or(ConsensusError::MoneyRange)?;
        self.pending_rewards.insert(key, next);
        self.tree.update(
            StateKey::PendingReward {
                maturity,
                recipient,
            }
            .hash(),
            Some(
                PendingReward {
                    maturity_height: maturity,
                    recipient,
                    amount: next,
                }
                .consensus_encode(),
            ),
        );
        Ok(())
    }

    pub fn mature(&mut self, height: u64) -> Result<Vec<(Address, u64)>, ConsensusError> {
        let matured: Vec<_> = self
            .pending_rewards
            .range((height, address_min())..=(height, address_max()))
            .map(|((_, address), amount)| (*address, *amount))
            .collect();
        for (address, amount) in &matured {
            self.pending_rewards.remove(&(height, *address));
            self.tree.update(
                StateKey::PendingReward {
                    maturity: height,
                    recipient: *address,
                }
                .hash(),
                None,
            );
            self.credit(*address, *amount)?;
        }
        Ok(matured)
    }

    fn set_account(&mut self, address: Address, account: Account) {
        let key = StateKey::Account(address).hash();
        if account == Account::default() {
            self.accounts.remove(&address);
            self.tree.update(key, None);
        } else {
            self.accounts.insert(address, account);
            self.tree.update(key, Some(account.consensus_encode()));
        }
    }
}

impl ConsensusEncode for LedgerState {
    fn encode_to(&self, writer: &mut Writer) {
        writer.u16(Self::SNAPSHOT_VERSION);
        writer.u32(u32::try_from(self.accounts.len()).expect("account count fits u32"));
        for (address, account) in &self.accounts {
            address.encode_to(writer);
            account.encode_to(writer);
        }
        writer.u32(u32::try_from(self.pending_rewards.len()).expect("reward count fits u32"));
        for ((maturity, recipient), amount) in &self.pending_rewards {
            PendingReward {
                maturity_height: *maturity,
                recipient: *recipient,
                amount: *amount,
            }
            .encode_to(writer);
        }
        self.root().encode_to(writer);
    }
}

impl ConsensusDecode for LedgerState {
    fn decode_from(reader: &mut Reader<'_>) -> Result<Self, CodecError> {
        if reader.u16()? != Self::SNAPSHOT_VERSION {
            return Err(CodecError::NonCanonical(
                "unsupported state snapshot version",
            ));
        }
        let account_count = reader.vector_len(2_000_000)?;
        let mut state = Self::default();
        let mut previous_address = None;
        for _ in 0..account_count {
            let address = Address::decode_from(reader)?;
            if previous_address.is_some_and(|previous| previous >= address) {
                return Err(CodecError::NonCanonical("accounts not strictly sorted"));
            }
            let account = Account::decode_from(reader)?;
            if account == Account::default() || account.balance > MAX_MONEY {
                return Err(CodecError::NonCanonical(
                    "invalid empty/out-of-range account",
                ));
            }
            state.set_account(address, account);
            previous_address = Some(address);
        }
        let reward_count = reader.vector_len(4_000_000)?;
        let mut previous_reward = None;
        for _ in 0..reward_count {
            let reward = PendingReward::decode_from(reader)?;
            let reward_key = (reward.maturity_height, reward.recipient);
            if previous_reward.is_some_and(|previous| previous >= reward_key)
                || reward.amount == 0
                || reward.amount > MAX_MONEY
            {
                return Err(CodecError::NonCanonical(
                    "invalid or unsorted pending reward",
                ));
            }
            state
                .schedule_reward(reward.maturity_height, reward.recipient, reward.amount)
                .map_err(|_| CodecError::NonCanonical("pending reward overflow"))?;
            previous_reward = Some(reward_key);
        }
        let committed_root = Hash32::decode_from(reader)?;
        if committed_root != state.root() {
            return Err(CodecError::NonCanonical("state snapshot root mismatch"));
        }
        Ok(state)
    }
}

fn address_min() -> Address {
    Address::from_public_key_hash([0; 20])
}

fn address_max() -> Address {
    Address::from_public_key_hash([0xff; 20])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{ConsensusDecode, ConsensusEncode};
    use secp256k1::{PublicKey, Secp256k1, SecretKey};

    fn address(byte: u8) -> Address {
        let secret = SecretKey::from_slice(&[byte; 32]).unwrap();
        Address::from_public_key(&PublicKey::from_secret_key(&Secp256k1::new(), &secret))
    }

    #[test]
    fn updates_are_reversible_and_order_independent() {
        let a = address(1);
        let b = address(2);
        let empty = LedgerState::default().root();
        let mut first = LedgerState::default();
        first.credit(a, 10).unwrap();
        first.credit(b, 20).unwrap();
        let mut second = LedgerState::default();
        second.credit(b, 20).unwrap();
        second.credit(a, 10).unwrap();
        assert_eq!(first.root(), second.root());
        first.debit(a, 10).unwrap();
        first.debit(b, 20).unwrap();
        assert_eq!(first.root(), empty);
    }

    #[test]
    fn snapshot_rebuilds_and_checks_the_root() {
        let mut state = LedgerState::default();
        state.credit(address(3), 99).unwrap();
        state.schedule_reward(100, address(4), 123).unwrap();
        let bytes = state.consensus_encode();
        let restored = LedgerState::consensus_decode(&bytes).unwrap();
        assert_eq!(state.root(), restored.root());
    }
}
