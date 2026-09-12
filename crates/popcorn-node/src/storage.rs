//! Persistence (SPEC.md §4.3): a single redb file, one write transaction per batch.
//!
//! `blocks` is append-only and is the **source of truth**. The state is a rebuildable cache,
//! and this implementation treats it as exactly that: checkpoints are written periodically
//! and startup replays the blocks after the newest one. A corrupted or missing checkpoint
//! costs time, never correctness.

use std::path::Path;

use popcorn_core::genesis::GenesisConfig;
use popcorn_core::state::State;
use popcorn_core::types::Block;
use redb::{Database, ReadableDatabase, TableDefinition};

/// Blocks by height, Borsh-encoded. Append-only.
const BLOCKS: TableDefinition<u64, Vec<u8>> = TableDefinition::new("blocks");
/// State checkpoints by height, Borsh-encoded.
const CHECKPOINTS: TableDefinition<u64, Vec<u8>> = TableDefinition::new("state");
/// Chain metadata: genesis config, head height.
const META: TableDefinition<&str, Vec<u8>> = TableDefinition::new("meta");
/// Manifested blobs by their blake3 hash, so `GET /blob/{hash}` and the mirror can serve
/// them. Required for third-party collection audit (§10), hence stored, not cached.
const BLOBS: TableDefinition<[u8; 32], Vec<u8>> = TableDefinition::new("blobs");

/// How often a full state snapshot is written. Startup replays at most this many blocks.
const CHECKPOINT_INTERVAL: u64 = 512;

const META_HEAD: &str = "head";
const META_GENESIS: &str = "genesis";

#[derive(Debug)]
pub enum StorageError {
    Database(String),
    Encoding(String),
    MissingGenesis,
    MissingBlock(u64),
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageError::Database(m) => write!(f, "storage: {m}"),
            StorageError::Encoding(m) => write!(f, "encoding: {m}"),
            StorageError::MissingGenesis => write!(f, "chain is not initialized"),
            StorageError::MissingBlock(h) => write!(f, "block {h} is missing"),
        }
    }
}

impl std::error::Error for StorageError {}

fn db_error<E: std::fmt::Display>(error: E) -> StorageError {
    StorageError::Database(error.to_string())
}

/// Borsh-encodable form of the genesis configuration.
#[derive(borsh::BorshSerialize, borsh::BorshDeserialize)]
struct StoredGenesis {
    genesis_drand_round: u64,
    node_pubkey: [u8; 32],
    foundation_pubkey: [u8; 32],
}

pub struct Storage {
    db: Database,
}

impl Storage {
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        let db = Database::create(path).map_err(db_error)?;
        Ok(Self { db })
    }

    /// Write the genesis block and configuration. Refuses to overwrite an existing chain.
    pub fn initialize(
        &self,
        config: &GenesisConfig,
        genesis_block: &Block,
    ) -> Result<(), StorageError> {
        if self.genesis_config()?.is_some() {
            return Err(StorageError::Database(
                "chain already initialized at this path".to_string(),
            ));
        }
        let stored = StoredGenesis {
            genesis_drand_round: config.genesis_drand_round,
            node_pubkey: config.node_pubkey,
            foundation_pubkey: config.foundation_pubkey,
        };

        let txn = self.db.begin_write().map_err(db_error)?;
        {
            let mut meta = txn.open_table(META).map_err(db_error)?;
            meta.insert(
                META_GENESIS,
                borsh::to_vec(&stored).map_err(|e| StorageError::Encoding(e.to_string()))?,
            )
            .map_err(db_error)?;
            meta.insert(META_HEAD, 0u64.to_le_bytes().to_vec())
                .map_err(db_error)?;

            let mut blocks = txn.open_table(BLOCKS).map_err(db_error)?;
            blocks
                .insert(
                    0u64,
                    borsh::to_vec(genesis_block)
                        .map_err(|e| StorageError::Encoding(e.to_string()))?,
                )
                .map_err(db_error)?;

            // Tables are created on first open, so open the rest now: a fresh chain should
            // answer queries about blobs and checkpoints rather than error.
            txn.open_table(CHECKPOINTS).map_err(db_error)?;
            txn.open_table(BLOBS).map_err(db_error)?;
        }
        txn.commit().map_err(db_error)?;
        Ok(())
    }

    pub fn genesis_config(&self) -> Result<Option<GenesisConfig>, StorageError> {
        let txn = self.db.begin_read().map_err(db_error)?;
        let Ok(meta) = txn.open_table(META) else {
            return Ok(None);
        };
        let Some(raw) = meta.get(META_GENESIS).map_err(db_error)? else {
            return Ok(None);
        };
        let stored: StoredGenesis =
            borsh::from_slice(&raw.value()).map_err(|e| StorageError::Encoding(e.to_string()))?;
        Ok(Some(GenesisConfig {
            genesis_drand_round: stored.genesis_drand_round,
            node_pubkey: stored.node_pubkey,
            foundation_pubkey: stored.foundation_pubkey,
        }))
    }

    pub fn head_height(&self) -> Result<u64, StorageError> {
        let txn = self.db.begin_read().map_err(db_error)?;
        let meta = txn
            .open_table(META)
            .map_err(|_| StorageError::MissingGenesis)?;
        let raw = meta
            .get(META_HEAD)
            .map_err(db_error)?
            .ok_or(StorageError::MissingGenesis)?;
        let bytes: [u8; 8] = raw
            .value()
            .try_into()
            .map_err(|_| StorageError::Encoding("head height".to_string()))?;
        Ok(u64::from_le_bytes(bytes))
    }

    pub fn block(&self, height: u64) -> Result<Option<Block>, StorageError> {
        let txn = self.db.begin_read().map_err(db_error)?;
        let Ok(blocks) = txn.open_table(BLOCKS) else {
            return Ok(None);
        };
        let Some(raw) = blocks.get(height).map_err(db_error)? else {
            return Ok(None);
        };
        let block =
            borsh::from_slice(&raw.value()).map_err(|e| StorageError::Encoding(e.to_string()))?;
        Ok(Some(block))
    }

    pub fn blob(&self, hash: &[u8; 32]) -> Result<Option<Vec<u8>>, StorageError> {
        let txn = self.db.begin_read().map_err(db_error)?;
        let Ok(blobs) = txn.open_table(BLOBS) else {
            return Ok(None);
        };
        Ok(blobs.get(*hash).map_err(db_error)?.map(|v| v.value()))
    }

    /// Commit a batch: block, head, manifested blobs, and periodically a state checkpoint —
    /// all in one write transaction, so the block and the state it implies can never
    /// disagree on disk (§4.3).
    pub fn commit_batch(
        &self,
        block: &Block,
        state: &State,
        blobs: &[([u8; 32], Vec<u8>)],
    ) -> Result<(), StorageError> {
        let height = block.header.height;
        let encoded_block =
            borsh::to_vec(block).map_err(|e| StorageError::Encoding(e.to_string()))?;

        let txn = self.db.begin_write().map_err(db_error)?;
        {
            let mut blocks = txn.open_table(BLOCKS).map_err(db_error)?;
            blocks.insert(height, encoded_block).map_err(db_error)?;

            let mut meta = txn.open_table(META).map_err(db_error)?;
            meta.insert(META_HEAD, height.to_le_bytes().to_vec())
                .map_err(db_error)?;

            let mut blob_table = txn.open_table(BLOBS).map_err(db_error)?;
            for (hash, bytes) in blobs {
                blob_table.insert(*hash, bytes.clone()).map_err(db_error)?;
            }

            if height % CHECKPOINT_INTERVAL == 0 {
                let mut checkpoints = txn.open_table(CHECKPOINTS).map_err(db_error)?;
                let encoded =
                    borsh::to_vec(state).map_err(|e| StorageError::Encoding(e.to_string()))?;
                checkpoints.insert(height, encoded).map_err(db_error)?;
            }
        }
        txn.commit().map_err(db_error)?;
        Ok(())
    }

    /// The newest checkpoint at or below `height`, if any.
    pub fn latest_checkpoint(&self, height: u64) -> Result<Option<(u64, State)>, StorageError> {
        let txn = self.db.begin_read().map_err(db_error)?;
        let Ok(checkpoints) = txn.open_table(CHECKPOINTS) else {
            return Ok(None);
        };
        let mut best: Option<(u64, State)> = None;
        for entry in checkpoints.range(0..=height).map_err(db_error)? {
            let (key, value) = entry.map_err(db_error)?;
            let state: State = borsh::from_slice(&value.value())
                .map_err(|e| StorageError::Encoding(e.to_string()))?;
            best = Some((key.value(), state));
        }
        Ok(best)
    }

    /// Iterate blocks from `from` to the head, inclusive.
    pub fn blocks_from(&self, from: u64) -> Result<Vec<Block>, StorageError> {
        let head = self.head_height()?;
        let mut out = Vec::new();
        for height in from..=head {
            out.push(
                self.block(height)?
                    .ok_or(StorageError::MissingBlock(height))?,
            );
        }
        Ok(out)
    }
}
