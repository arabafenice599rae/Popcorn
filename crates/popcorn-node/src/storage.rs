//! Persistence (SPEC.md §4.3): a single redb file, one write transaction per batch.
//!
//! `blocks` is append-only and is the **source of truth**. The state is a rebuildable cache,
//! and this implementation treats it as exactly that: checkpoints are written periodically
//! and startup replays the blocks after the newest one. A corrupted or missing checkpoint
//! costs time, never correctness.

use std::path::Path;

use popcorn_core::constants::CONSENSUS_VERSION;
use popcorn_core::crypto::consensus_lock_digest;
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
    /// The chain was created under different consensus rules than this binary implements.
    ForeignConsensus {
        chain_version: u64,
        binary_version: u64,
        chain_lock: [u8; 32],
        binary_lock: [u8; 32],
    },
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageError::Database(m) => write!(f, "storage: {m}"),
            StorageError::Encoding(m) => write!(f, "encoding: {m}"),
            StorageError::MissingGenesis => write!(f, "chain is not initialized"),
            StorageError::MissingBlock(h) => write!(f, "block {h} is missing"),
            StorageError::ForeignConsensus {
                chain_version,
                binary_version,
                chain_lock,
                binary_lock,
            } => {
                // Refusing is the only safe answer: continuing would extend somebody else's
                // chain under rules it never agreed to, and every block after that would be
                // a divergence a third party has to discover for themselves.
                write!(
                    f,
                    "this chain was created under different consensus rules \
                     (chain {chain_version:#018x}, this binary {binary_version:#018x}"
                )?;
                if chain_lock != binary_lock {
                    write!(
                        f,
                        "; pinned dependency digests also differ: chain {}, this binary {}",
                        crate::encoding::to_hex(chain_lock),
                        crate::encoding::to_hex(binary_lock)
                    )?;
                }
                write!(f, ")")
            }
        }
    }
}

impl std::error::Error for StorageError {}

fn db_error<E: std::fmt::Display>(error: E) -> StorageError {
    StorageError::Database(error.to_string())
}

/// Borsh-encodable form of the genesis configuration, plus the consensus identity.
///
/// The identity is committed to in the state root (§13), which is what a third party checks
/// with nothing but the exported blocks. This copy is not that commitment: it is here so a
/// mismatched binary can say *which* version the chain was created under instead of only
/// reporting a root that does not match.
#[derive(borsh::BorshSerialize, borsh::BorshDeserialize)]
struct StoredGenesis {
    genesis_drand_round: u64,
    node_pubkey: [u8; 32],
    foundation_pubkey: [u8; 32],
    consensus_version: u64,
    lock_digest: [u8; 32],
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
            consensus_version: CONSENSUS_VERSION,
            lock_digest: consensus_lock_digest(),
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

    /// The consensus identity recorded when this chain was created (§13).
    pub fn stored_identity(&self) -> Result<Option<(u64, [u8; 32])>, StorageError> {
        Ok(self
            .stored_genesis()?
            .map(|stored| (stored.consensus_version, stored.lock_digest)))
    }

    fn stored_genesis(&self) -> Result<Option<StoredGenesis>, StorageError> {
        let txn = self.db.begin_read().map_err(db_error)?;
        let Ok(meta) = txn.open_table(META) else {
            return Ok(None);
        };
        let Some(raw) = meta.get(META_GENESIS).map_err(db_error)? else {
            return Ok(None);
        };
        let stored: StoredGenesis =
            borsh::from_slice(&raw.value()).map_err(|e| StorageError::Encoding(e.to_string()))?;
        Ok(Some(stored))
    }

    pub fn genesis_config(&self) -> Result<Option<GenesisConfig>, StorageError> {
        Ok(self.stored_genesis()?.map(|stored| GenesisConfig {
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
