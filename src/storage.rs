use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{bail, Context};
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use crate::{
    codec::{ConsensusDecode, ConsensusEncode},
    consensus::{Block, Chain, CHAIN_ID, PROTOCOL_VERSION},
    crypto::Hash32,
};

#[derive(Clone, Debug)]
pub struct StoredPeer {
    pub url: String,
    pub score: i64,
    pub last_success: Option<u64>,
    pub banned_until: Option<u64>,
}

pub struct Storage {
    connection: Mutex<Connection>,
}

impl Storage {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(path)
            .with_context(|| format!("cannot open database {}", path.display()))?;
        connection.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=FULL;
             PRAGMA foreign_keys=ON;
             PRAGMA busy_timeout=5000;
             CREATE TABLE IF NOT EXISTS metadata (
                 key TEXT PRIMARY KEY,
                 value BLOB NOT NULL
             ) STRICT;
             CREATE TABLE IF NOT EXISTS blocks (
                 hash BLOB PRIMARY KEY CHECK(length(hash)=32),
                 height INTEGER NOT NULL CHECK(height>=0),
                 parent BLOB NOT NULL CHECK(length(parent)=32),
                 data BLOB NOT NULL,
                 received_at INTEGER NOT NULL
             ) STRICT;
             CREATE INDEX IF NOT EXISTS blocks_height ON blocks(height);
             CREATE TABLE IF NOT EXISTS active_chain (
                 height INTEGER PRIMARY KEY CHECK(height>=0),
                 hash BLOB NOT NULL UNIQUE REFERENCES blocks(hash)
             ) STRICT;
             CREATE TABLE IF NOT EXISTS state_snapshots (
                 height INTEGER PRIMARY KEY CHECK(height>=0),
                 block_hash BLOB NOT NULL CHECK(length(block_hash)=32),
                 state_root BLOB NOT NULL CHECK(length(state_root)=32),
                 data BLOB NOT NULL,
                 created_at INTEGER NOT NULL
             ) STRICT;
             CREATE TABLE IF NOT EXISTS peers (
                 url TEXT PRIMARY KEY,
                 score INTEGER NOT NULL DEFAULT 0,
                 last_success INTEGER,
                 banned_until INTEGER
             ) STRICT;",
        )?;
        let integrity: String = connection.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
        if integrity != "ok" {
            bail!("SQLite integrity check failed: {integrity}");
        }
        let storage = Self {
            connection: Mutex::new(connection),
        };
        storage.initialize_identity()?;
        Ok(storage)
    }

    fn initialize_identity(&self) -> anyhow::Result<()> {
        let mut connection = self.connection.lock();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing_network: Option<Vec<u8>> = transaction
            .query_row(
                "SELECT value FROM metadata WHERE key='network'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(network) = existing_network {
            if network != CHAIN_ID.as_bytes() {
                bail!(
                    "database belongs to network {}, expected {CHAIN_ID}",
                    String::from_utf8_lossy(&network)
                );
            }
            let protocol: Option<Vec<u8>> = transaction
                .query_row(
                    "SELECT value FROM metadata WHERE key='protocol'",
                    [],
                    |row| row.get(0),
                )
                .optional()?;
            let genesis: Option<Vec<u8>> = transaction
                .query_row(
                    "SELECT value FROM metadata WHERE key='genesis'",
                    [],
                    |row| row.get(0),
                )
                .optional()?;
            if protocol.as_deref() != Some(PROTOCOL_VERSION.to_le_bytes().as_slice())
                || genesis.as_deref() != Some(Block::genesis().id().as_bytes().as_slice())
            {
                bail!("database protocol/genesis identity does not match this binary");
            }
        } else {
            transaction.execute(
                "INSERT INTO metadata(key,value) VALUES('network',?1)",
                [CHAIN_ID.as_bytes()],
            )?;
            transaction.execute(
                "INSERT INTO metadata(key,value) VALUES('protocol',?1)",
                [PROTOCOL_VERSION.to_le_bytes().as_slice()],
            )?;
            transaction.execute(
                "INSERT INTO metadata(key,value) VALUES('genesis',?1)",
                [Block::genesis().id().as_bytes().as_slice()],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn load_active_blocks(&self) -> anyhow::Result<Vec<Block>> {
        let connection = self.connection.lock();
        let mut statement = connection.prepare(
            "SELECT b.data FROM active_chain a JOIN blocks b ON b.hash=a.hash ORDER BY a.height",
        )?;
        let rows = statement.query_map([], |row| row.get::<_, Vec<u8>>(0))?;
        let mut blocks = Vec::new();
        for row in rows {
            let bytes = row?;
            blocks.push(Block::consensus_decode(&bytes).context("invalid stored block encoding")?);
        }
        Ok(blocks)
    }

    /// Commit block records, active indexes, tip metadata, and the verified
    /// chainstate snapshot in one SQLite transaction. A crash exposes either the
    /// old complete state or the new complete state, never a half-reorg.
    pub fn save_chain(&self, chain: &Chain) -> anyhow::Result<()> {
        let now = unix_time();
        let mut connection = self.connection.lock();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let previous: Option<(u64, Vec<u8>)> = transaction
            .query_row(
                "SELECT height,hash FROM active_chain ORDER BY height DESC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let start = previous
            .and_then(|(height, hash)| {
                chain
                    .block_at(height)
                    .filter(|block| block.id().as_bytes().as_slice() == hash)
                    .and_then(|_| usize::try_from(height.saturating_add(1)).ok())
            })
            .unwrap_or(0);
        for block in &chain.blocks()[start..] {
            transaction.execute(
                "INSERT INTO blocks(hash,height,parent,data,received_at)
                 VALUES(?1,?2,?3,?4,?5)
                 ON CONFLICT(hash) DO NOTHING",
                params![
                    block.id().as_bytes().as_slice(),
                    i64::try_from(block.header.height)?,
                    block.header.previous_block.as_bytes().as_slice(),
                    block.consensus_encode(),
                    i64::try_from(now)?,
                ],
            )?;
        }
        transaction.execute(
            "DELETE FROM active_chain WHERE height >= ?1",
            [i64::try_from(start)?],
        )?;
        for block in &chain.blocks()[start..] {
            transaction.execute(
                "INSERT INTO active_chain(height,hash) VALUES(?1,?2)",
                params![
                    i64::try_from(block.header.height)?,
                    block.id().as_bytes().as_slice()
                ],
            )?;
        }
        let snapshot = chain.state().consensus_encode();
        transaction.execute(
            "INSERT INTO state_snapshots(height,block_hash,state_root,data,created_at)
             VALUES(?1,?2,?3,?4,?5)
             ON CONFLICT(height) DO UPDATE SET
               block_hash=excluded.block_hash,
               state_root=excluded.state_root,
               data=excluded.data,
               created_at=excluded.created_at",
            params![
                i64::try_from(chain.height())?,
                chain.tip().id().as_bytes().as_slice(),
                chain.state().root().as_bytes().as_slice(),
                snapshot,
                i64::try_from(now)?,
            ],
        )?;
        transaction.execute(
            "INSERT INTO metadata(key,value) VALUES('tip',?1)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [chain.tip().id().as_bytes().as_slice()],
        )?;
        transaction.execute(
            "DELETE FROM state_snapshots WHERE height NOT IN
             (SELECT height FROM state_snapshots ORDER BY height DESC LIMIT 3)",
            [],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn block(&self, hash: Hash32) -> anyhow::Result<Option<Block>> {
        let connection = self.connection.lock();
        let bytes: Option<Vec<u8>> = connection
            .query_row(
                "SELECT data FROM blocks WHERE hash=?1",
                [hash.as_bytes().as_slice()],
                |row| row.get(0),
            )
            .optional()?;
        bytes
            .map(|bytes| Block::consensus_decode(&bytes).context("invalid stored block"))
            .transpose()
    }

    pub fn store_side_block(&self, block: &Block) -> anyhow::Result<()> {
        let connection = self.connection.lock();
        connection.execute(
            "INSERT INTO blocks(hash,height,parent,data,received_at)
             VALUES(?1,?2,?3,?4,?5) ON CONFLICT(hash) DO NOTHING",
            params![
                block.id().as_bytes().as_slice(),
                i64::try_from(block.header.height)?,
                block.header.previous_block.as_bytes().as_slice(),
                block.consensus_encode(),
                i64::try_from(unix_time())?,
            ],
        )?;
        Ok(())
    }

    pub fn load_peers(&self) -> anyhow::Result<Vec<StoredPeer>> {
        let connection = self.connection.lock();
        let mut statement = connection.prepare(
            "SELECT url,score,last_success,banned_until FROM peers ORDER BY last_success DESC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(StoredPeer {
                url: row.get(0)?,
                score: row.get(1)?,
                last_success: row
                    .get::<_, Option<i64>>(2)?
                    .and_then(|value| value.try_into().ok()),
                banned_until: row
                    .get::<_, Option<i64>>(3)?
                    .and_then(|value| value.try_into().ok()),
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn save_peer(&self, peer: &StoredPeer) -> anyhow::Result<()> {
        let connection = self.connection.lock();
        connection.execute(
            "INSERT INTO peers(url,score,last_success,banned_until) VALUES(?1,?2,?3,?4)
             ON CONFLICT(url) DO UPDATE SET score=excluded.score,
                last_success=excluded.last_success,banned_until=excluded.banned_until",
            params![
                peer.url,
                peer.score,
                peer.last_success
                    .and_then(|value| i64::try_from(value).ok()),
                peer.banned_until
                    .and_then(|value| i64::try_from(value).ok()),
            ],
        )?;
        Ok(())
    }
}

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
        let path = directory.path().join("chain.sqlite");
        let storage = Storage::open(&path).unwrap();
        let chain = Chain::genesis();
        storage.save_chain(&chain).unwrap();
        storage
            .connection
            .lock()
            .execute_batch(
                "CREATE TEMP TRIGGER preserve_prefix BEFORE DELETE ON active_chain
             BEGIN SELECT RAISE(ABORT, 'unchanged chain prefix must not be rewritten'); END;",
            )
            .unwrap();
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
