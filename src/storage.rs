use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{bail, Context};
use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};

use crate::{
    codec::{ConsensusDecode, ConsensusEncode},
    consensus::{Block, Chain, CHAIN_ID, PROTOCOL_VERSION},
    crypto::Hash32,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredPeer {
    pub url: String,
    pub score: i64,
    pub last_success: Option<u64>,
    pub banned_until: Option<u64>,
}

const TABLE_METADATA: TableDefinition<&str, &[u8]> = TableDefinition::new("metadata");
const TABLE_BLOCKS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("blocks");
const TABLE_ACTIVE_CHAIN: TableDefinition<u64, &[u8]> = TableDefinition::new("active_chain");
const TABLE_STATE_SNAPSHOTS: TableDefinition<u64, &[u8]> = TableDefinition::new("state_snapshots");
const TABLE_PEERS: TableDefinition<&str, &[u8]> = TableDefinition::new("peers");

pub struct Storage {
    db: Database,
}

impl Storage {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let db = Database::create(path)
            .with_context(|| format!("cannot open redb database {}", path.display()))?;
        let storage = Self { db };
        storage.initialize_identity()?;
        Ok(storage)
    }

    fn initialize_identity(&self) -> anyhow::Result<()> {
        let write_txn = self.db.begin_write()?;
        {
            let _ = write_txn.open_table(TABLE_BLOCKS)?;
            let _ = write_txn.open_table(TABLE_ACTIVE_CHAIN)?;
            let _ = write_txn.open_table(TABLE_STATE_SNAPSHOTS)?;
            let _ = write_txn.open_table(TABLE_PEERS)?;
            let mut meta = write_txn.open_table(TABLE_METADATA)?;

            let existing_network = meta.get("network")?.map(|v| v.value().to_vec());
            if let Some(network) = existing_network {
                if network != CHAIN_ID.as_bytes() {
                    bail!(
                        "database belongs to network {}, expected {CHAIN_ID}",
                        String::from_utf8_lossy(&network)
                    );
                }
                let protocol = meta.get("protocol")?.map(|v| v.value().to_vec());
                let genesis = meta.get("genesis")?.map(|v| v.value().to_vec());
                if protocol.as_deref() != Some(PROTOCOL_VERSION.to_le_bytes().as_slice())
                    || genesis.as_deref() != Some(Block::genesis().id().as_bytes().as_slice())
                {
                    bail!("database protocol/genesis identity does not match this binary");
                }
            } else {
                meta.insert("network", CHAIN_ID.as_bytes())?;
                meta.insert("protocol", PROTOCOL_VERSION.to_le_bytes().as_slice())?;
                meta.insert("genesis", Block::genesis().id().as_bytes().as_slice())?;
            }
        }
        write_txn.commit()?;
        Ok(())
    }

    pub fn load_active_blocks(&self) -> anyhow::Result<Vec<Block>> {
        let read_txn = self.db.begin_read()?;
        let active = read_txn.open_table(TABLE_ACTIVE_CHAIN)?;
        let blocks_table = read_txn.open_table(TABLE_BLOCKS)?;
        let mut blocks = Vec::new();
        for item in active.iter()? {
            let (_height, hash) = item?;
            let block_data = blocks_table
                .get(hash.value())?
                .ok_or_else(|| anyhow::anyhow!("missing block data for active hash"))?;
            let block = Block::consensus_decode(block_data.value())
                .context("invalid stored block encoding")?;
            blocks.push(block);
        }
        Ok(blocks)
    }

    pub fn save_chain(&self, chain: &Chain) -> anyhow::Result<()> {
        let write_txn = self.db.begin_write()?;
        {
            let mut meta = write_txn.open_table(TABLE_METADATA)?;
            let mut blocks_table = write_txn.open_table(TABLE_BLOCKS)?;
            let mut active = write_txn.open_table(TABLE_ACTIVE_CHAIN)?;
            let mut snapshots = write_txn.open_table(TABLE_STATE_SNAPSHOTS)?;

            let previous = active
                .last()?
                .map(|(h, hash)| (h.value(), hash.value().to_vec()));
            let start = previous
                .and_then(|(height, hash)| {
                    chain
                        .block_at(height)
                        .filter(|block| block.id().as_bytes().as_slice() == hash.as_slice())
                        .and_then(|_| usize::try_from(height.saturating_add(1)).ok())
                })
                .unwrap_or(0);

            for block in &chain.blocks()[start..] {
                let hash = block.id();
                if blocks_table.get(hash.as_bytes().as_slice())?.is_none() {
                    blocks_table.insert(
                        hash.as_bytes().as_slice(),
                        block.consensus_encode().as_slice(),
                    )?;
                }
            }

            let heights_to_delete: Vec<u64> = active
                .range(start as u64..)?
                .map(|item| item.map(|(k, _)| k.value()))
                .collect::<Result<Vec<_>, _>>()?;
            for h in heights_to_delete {
                active.remove(h)?;
            }

            for block in &chain.blocks()[start..] {
                active.insert(block.header.height, block.id().as_bytes().as_slice())?;
            }

            let snapshot = chain.state().consensus_encode();
            snapshots.insert(chain.height(), snapshot.as_slice())?;

            meta.insert("tip", chain.tip().id().as_bytes().as_slice())?;

            let all_snapshot_heights: Vec<u64> = snapshots
                .iter()?
                .map(|item| item.map(|(k, _)| k.value()))
                .collect::<Result<Vec<_>, _>>()?;
            if all_snapshot_heights.len() > 3 {
                let remove_count = all_snapshot_heights.len() - 3;
                for h in &all_snapshot_heights[..remove_count] {
                    snapshots.remove(*h)?;
                }
            }
        }
        write_txn.commit()?;
        Ok(())
    }

    pub fn block(&self, hash: Hash32) -> anyhow::Result<Option<Block>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(TABLE_BLOCKS)?;
        let bytes = table.get(hash.as_bytes().as_slice())?;
        bytes
            .map(|val| Block::consensus_decode(val.value()).context("invalid stored block"))
            .transpose()
    }

    pub fn store_side_block(&self, block: &Block) -> anyhow::Result<()> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(TABLE_BLOCKS)?;
            let hash = block.id();
            if table.get(hash.as_bytes().as_slice())?.is_none() {
                table.insert(
                    hash.as_bytes().as_slice(),
                    block.consensus_encode().as_slice(),
                )?;
            }
        }
        write_txn.commit()?;
        Ok(())
    }

    pub fn load_peers(&self) -> anyhow::Result<Vec<StoredPeer>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(TABLE_PEERS)?;
        let mut peers = Vec::new();
        for item in table.iter()? {
            let (_url, val) = item?;
            let peer: StoredPeer = serde_json::from_slice(val.value())?;
            peers.push(peer);
        }
        peers.sort_by(|a, b| b.last_success.cmp(&a.last_success));
        Ok(peers)
    }

    pub fn save_peer(&self, peer: &StoredPeer) -> anyhow::Result<()> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(TABLE_PEERS)?;
            let data = serde_json::to_vec(peer)?;
            table.insert(peer.url.as_str(), data.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }
}

#[allow(dead_code)]
fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_genesis_round_trip_preserves_identity() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("chain.redb");
        let storage = Storage::open(&path).unwrap();
        let chain = Chain::genesis();
        storage.save_chain(&chain).unwrap();
        drop(storage);

        let reopened = Storage::open(&path).unwrap();
        let blocks = reopened.load_active_blocks().unwrap();
        assert_eq!(blocks, vec![Block::genesis()]);
        assert_eq!(
            reopened.block(Block::genesis().id()).unwrap(),
            blocks.first().cloned()
        );
    }
}
