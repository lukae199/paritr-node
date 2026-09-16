use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use crate::{
    codec::{CodecError, ConsensusDecode, ConsensusEncode, Reader, Writer},
    crypto::{domain_hash, Address, Hash32},
};

use super::{
    GENESIS_MESSAGE, GENESIS_TIMESTAMP, HEADER_VERSION, MAX_BLOCK_TRANSACTIONS,
    MAX_TEMPLATES_PER_BLOCK, MAX_TRANSACTION_BYTES, MAX_WORKSHARES_PER_BLOCK, PROTOCOL_VERSION,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Transaction {
    pub version: u16,
    pub sender: Address,
    pub recipient: Address,
    pub amount: u64,
    pub fee: u64,
    pub nonce: u64,
    #[serde(with = "hex_vec")]
    pub public_key: Vec<u8>,
    #[serde(with = "hex_vec")]
    pub signature: Vec<u8>,
}

impl Transaction {
    pub const VERSION: u16 = 1;

    pub fn signing_bytes(&self) -> Vec<u8> {
        let mut writer = Writer::with_capacity(128);
        writer.bytes(super::CHAIN_ID.as_bytes());
        writer.u16(self.version);
        self.sender.encode_to(&mut writer);
        self.recipient.encode_to(&mut writer);
        writer.u64(self.amount);
        writer.u64(self.fee);
        writer.u64(self.nonce);
        writer.into_inner()
    }

    pub fn signature_hash(&self) -> Hash32 {
        domain_hash(b"PARITR-P10-TX-SIGN-v1", &self.signing_bytes())
    }

    pub fn id(&self) -> Hash32 {
        domain_hash(b"PARITR-P10-TXID-v1", &self.consensus_encode())
    }
}

impl ConsensusEncode for Transaction {
    fn encode_to(&self, writer: &mut Writer) {
        writer.u16(self.version);
        self.sender.encode_to(writer);
        self.recipient.encode_to(writer);
        writer.u64(self.amount);
        writer.u64(self.fee);
        writer.u64(self.nonce);
        writer.bytes(&self.public_key);
        writer.bytes(&self.signature);
    }
}

impl ConsensusDecode for Transaction {
    fn decode_from(reader: &mut Reader<'_>) -> Result<Self, CodecError> {
        let value = Self {
            version: reader.u16()?,
            sender: Address::decode_from(reader)?,
            recipient: Address::decode_from(reader)?,
            amount: reader.u64()?,
            fee: reader.u64()?,
            nonce: reader.u64()?,
            public_key: reader.bytes(65)?.to_vec(),
            signature: reader.bytes(64)?.to_vec(),
        };
        if value.consensus_encode().len() > MAX_TRANSACTION_BYTES {
            return Err(CodecError::LimitExceeded {
                actual: value.consensus_encode().len(),
                limit: MAX_TRANSACTION_BYTES,
            });
        }
        Ok(value)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RewardClaim {
    pub version: u16,
    pub height: u64,
    pub finder: Address,
    pub amount: u64,
    pub extranonce: u64,
}

impl RewardClaim {
    pub const VERSION: u16 = 1;

    pub fn id(&self) -> Hash32 {
        domain_hash(b"PARITR-P10-REWARD-CLAIM-v1", &self.consensus_encode())
    }
}

impl ConsensusEncode for RewardClaim {
    fn encode_to(&self, writer: &mut Writer) {
        writer.u16(self.version);
        writer.u64(self.height);
        self.finder.encode_to(writer);
        writer.u64(self.amount);
        writer.u64(self.extranonce);
    }
}

impl ConsensusDecode for RewardClaim {
    fn decode_from(reader: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            version: reader.u16()?,
            height: reader.u64()?,
            finder: Address::decode_from(reader)?,
            amount: reader.u64()?,
            extranonce: reader.u64()?,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BlockHeader {
    pub version: u16,
    pub protocol: u16,
    pub height: u64,
    pub previous_block: Hash32,
    pub transactions_root: Hash32,
    pub state_root: Hash32,
    pub workshare_root: Hash32,
    pub timestamp: u64,
    pub bits: u32,
    pub nonce: u64,
}

impl BlockHeader {
    pub const ENCODED_LEN: usize = 160;

    pub fn id(&self) -> Hash32 {
        crate::crypto::sha256d(&self.consensus_encode())
    }
}

impl ConsensusEncode for BlockHeader {
    fn encode_to(&self, writer: &mut Writer) {
        writer.u16(self.version);
        writer.u16(self.protocol);
        writer.u64(self.height);
        self.previous_block.encode_to(writer);
        self.transactions_root.encode_to(writer);
        self.state_root.encode_to(writer);
        self.workshare_root.encode_to(writer);
        writer.u64(self.timestamp);
        writer.u32(self.bits);
        writer.u64(self.nonce);
    }
}

impl ConsensusDecode for BlockHeader {
    fn decode_from(reader: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            version: reader.u16()?,
            protocol: reader.u16()?,
            height: reader.u64()?,
            previous_block: Hash32::decode_from(reader)?,
            transactions_root: Hash32::decode_from(reader)?,
            state_root: Hash32::decode_from(reader)?,
            workshare_root: Hash32::decode_from(reader)?,
            timestamp: reader.u64()?,
            bits: reader.u32()?,
            nonce: reader.u64()?,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BlockTemplate {
    pub version: u16,
    pub height: u64,
    pub parent: Hash32,
    pub transactions: Vec<Transaction>,
}

impl BlockTemplate {
    pub const VERSION: u16 = 1;

    pub fn id(&self) -> Hash32 {
        domain_hash(b"PARITR-P10-TEMPLATE-v1", &self.consensus_encode())
    }
}

impl ConsensusEncode for BlockTemplate {
    fn encode_to(&self, writer: &mut Writer) {
        writer.u16(self.version);
        writer.u64(self.height);
        self.parent.encode_to(writer);
        writer.u32(u32::try_from(self.transactions.len()).expect("transaction count fits u32"));
        for transaction in &self.transactions {
            writer.bytes(&transaction.consensus_encode());
        }
    }
}

impl ConsensusDecode for BlockTemplate {
    fn decode_from(reader: &mut Reader<'_>) -> Result<Self, CodecError> {
        let version = reader.u16()?;
        let height = reader.u64()?;
        let parent = Hash32::decode_from(reader)?;
        let count = reader.vector_len(MAX_BLOCK_TRANSACTIONS)?;
        let mut transactions = Vec::with_capacity(count);
        for _ in 0..count {
            transactions.push(Transaction::consensus_decode(
                reader.bytes(MAX_TRANSACTION_BYTES)?,
            )?);
        }
        Ok(Self {
            version,
            height,
            parent,
            transactions,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Workshare {
    pub version: u16,
    pub previous_workshare: Hash32,
    pub template_id: Hash32,
    pub miner: Address,
    pub extranonce: u64,
    pub candidate_header: BlockHeader,
}

impl Workshare {
    pub const VERSION: u16 = 1;

    pub fn id(&self) -> Hash32 {
        domain_hash(b"PARITR-P10-WORKSHARE-v1", &self.consensus_encode())
    }
}

impl ConsensusEncode for Workshare {
    fn encode_to(&self, writer: &mut Writer) {
        writer.u16(self.version);
        self.previous_workshare.encode_to(writer);
        self.template_id.encode_to(writer);
        self.miner.encode_to(writer);
        writer.u64(self.extranonce);
        self.candidate_header.encode_to(writer);
    }
}

impl ConsensusDecode for Workshare {
    fn decode_from(reader: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            version: reader.u16()?,
            previous_workshare: Hash32::decode_from(reader)?,
            template_id: Hash32::decode_from(reader)?,
            miner: Address::decode_from(reader)?,
            extranonce: reader.u64()?,
            candidate_header: BlockHeader::decode_from(reader)?,
        })
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct WorkshareWitness {
    pub version: u16,
    pub templates: Vec<BlockTemplate>,
    pub workshares: Vec<Workshare>,
}

impl WorkshareWitness {
    pub const VERSION: u16 = 1;

    pub fn empty() -> Self {
        Self {
            version: Self::VERSION,
            templates: Vec::new(),
            workshares: Vec::new(),
        }
    }

    pub fn root(&self) -> Hash32 {
        domain_hash(b"PARITR-P10-WORKSHARE-WITNESS-v1", &self.consensus_encode())
    }

    pub fn is_canonical(&self) -> bool {
        let ids: Vec<_> = self.templates.iter().map(BlockTemplate::id).collect();
        if ids.windows(2).any(|pair| pair[0] >= pair[1]) {
            return false;
        }
        let referenced: BTreeSet<_> = self
            .workshares
            .iter()
            .map(|share| share.template_id)
            .collect();
        let available: BTreeSet<_> = ids.into_iter().collect();
        referenced == available
    }
}

impl ConsensusEncode for WorkshareWitness {
    fn encode_to(&self, writer: &mut Writer) {
        writer.u16(self.version);
        writer.u32(u32::try_from(self.templates.len()).expect("template count fits u32"));
        for template in &self.templates {
            writer.bytes(&template.consensus_encode());
        }
        writer.u32(u32::try_from(self.workshares.len()).expect("share count fits u32"));
        for share in &self.workshares {
            writer.bytes(&share.consensus_encode());
        }
    }
}

impl ConsensusDecode for WorkshareWitness {
    fn decode_from(reader: &mut Reader<'_>) -> Result<Self, CodecError> {
        let version = reader.u16()?;
        let template_count = reader.vector_len(MAX_TEMPLATES_PER_BLOCK)?;
        let mut templates = Vec::with_capacity(template_count);
        for _ in 0..template_count {
            templates.push(BlockTemplate::consensus_decode(
                reader.bytes(super::MAX_TEMPLATE_BYTES)?,
            )?);
        }
        let share_count = reader.vector_len(MAX_WORKSHARES_PER_BLOCK)?;
        let mut workshares = Vec::with_capacity(share_count);
        for _ in 0..share_count {
            workshares.push(Workshare::consensus_decode(reader.bytes(512)?)?);
        }
        Ok(Self {
            version,
            templates,
            workshares,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Block {
    pub header: BlockHeader,
    pub reward_claim: Option<RewardClaim>,
    pub transactions: Vec<Transaction>,
    pub workshare_witness: WorkshareWitness,
}

impl Block {
    pub fn id(&self) -> Hash32 {
        self.header.id()
    }

    pub fn transaction_root(&self) -> Hash32 {
        let mut leaves = Vec::with_capacity(self.transactions.len() + 1);
        if let Some(claim) = &self.reward_claim {
            leaves.push(claim.id());
        }
        leaves.extend(self.transactions.iter().map(Transaction::id));
        merkle_root(&leaves)
    }

    pub fn genesis() -> Self {
        let witness = WorkshareWitness::empty();
        Self {
            header: BlockHeader {
                version: HEADER_VERSION,
                protocol: PROTOCOL_VERSION,
                height: 0,
                previous_block: Hash32::ZERO,
                transactions_root: domain_hash(
                    b"PARITR-P10-GENESIS-v1",
                    GENESIS_MESSAGE.as_bytes(),
                ),
                state_root: super::LedgerState::default().root(),
                workshare_root: witness.root(),
                timestamp: GENESIS_TIMESTAMP,
                bits: super::target_to_bits(super::initial_target()),
                nonce: 0,
            },
            reward_claim: None,
            transactions: Vec::new(),
            workshare_witness: witness,
        }
    }
}

impl ConsensusEncode for Block {
    fn encode_to(&self, writer: &mut Writer) {
        self.header.encode_to(writer);
        match &self.reward_claim {
            Some(claim) => {
                writer.u8(1);
                claim.encode_to(writer);
            }
            None => writer.u8(0),
        }
        writer.u32(u32::try_from(self.transactions.len()).expect("transaction count fits u32"));
        for transaction in &self.transactions {
            writer.bytes(&transaction.consensus_encode());
        }
        writer.bytes(&self.workshare_witness.consensus_encode());
    }
}

impl ConsensusDecode for Block {
    fn decode_from(reader: &mut Reader<'_>) -> Result<Self, CodecError> {
        let header = BlockHeader::decode_from(reader)?;
        let reward_claim = match reader.u8()? {
            0 => None,
            1 => Some(RewardClaim::decode_from(reader)?),
            tag => return Err(CodecError::InvalidTag(tag)),
        };
        let count = reader.vector_len(MAX_BLOCK_TRANSACTIONS)?;
        let mut transactions = Vec::with_capacity(count);
        for _ in 0..count {
            transactions.push(Transaction::consensus_decode(
                reader.bytes(MAX_TRANSACTION_BYTES)?,
            )?);
        }
        let witness =
            WorkshareWitness::consensus_decode(reader.bytes(super::MAX_WORKSHARE_WITNESS_BYTES)?)?;
        Ok(Self {
            header,
            reward_claim,
            transactions,
            workshare_witness: witness,
        })
    }
}

pub fn merkle_root(leaves: &[Hash32]) -> Hash32 {
    if leaves.is_empty() {
        return domain_hash(b"PARITR-P10-MERKLE-EMPTY-v1", &[]);
    }
    let mut level = leaves.to_vec();
    while level.len() > 1 {
        if level.len() % 2 == 1 {
            level.push(*level.last().expect("nonempty"));
        }
        level = level
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| {
                let mut bytes = [0_u8; 64];
                bytes[..32].copy_from_slice(pair[0].as_bytes());
                bytes[32..].copy_from_slice(pair[1].as_bytes());
                domain_hash(b"PARITR-P10-MERKLE-NODE-v1", &bytes)
            })
            .collect();
    }
    level[0]
}

mod hex_vec {
    use serde::{de, Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if value.len() % 2 != 0 || value.bytes().any(|byte| !byte.is_ascii_hexdigit()) {
            return Err(de::Error::custom(
                "expected canonical even-length hexadecimal",
            ));
        }
        hex::decode(value).map_err(de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{ConsensusDecode, ConsensusEncode};

    #[test]
    fn header_has_fixed_cross_platform_length() {
        let genesis = Block::genesis();
        assert_eq!(
            genesis.header.consensus_encode().len(),
            BlockHeader::ENCODED_LEN
        );
        assert_eq!(
            BlockHeader::consensus_decode(&genesis.header.consensus_encode()).unwrap(),
            genesis.header
        );
    }

    #[test]
    fn genesis_is_stable_and_self_consistent() {
        let genesis = Block::genesis();
        assert_eq!(genesis.header.id(), genesis.id());
        assert_eq!(
            genesis.header.state_root,
            super::super::LedgerState::default().root()
        );
        assert_eq!(
            genesis.header.workshare_root,
            genesis.workshare_witness.root()
        );
        assert_eq!(
            Block::consensus_decode(&genesis.consensus_encode()).unwrap(),
            genesis
        );
    }
}
