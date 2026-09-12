//! The chain state machine: open, replay, extend (SPEC.md §5.1, §10).

use std::path::Path;

use ed25519_dalek::SigningKey;
use popcorn_core::constants::CONSENSUS_VERSION;
use popcorn_core::crypto::{consensus_lock_digest, sign};
use popcorn_core::execute::{execute_batch, BatchInput};
use popcorn_core::genesis::{genesis_block, genesis_state, round_for_height, GenesisConfig};
use popcorn_core::state::State;
use popcorn_core::types::{Block, Header, SignedTx};

use crate::storage::{Storage, StorageError};

pub struct Chain {
    storage: Storage,
    config: GenesisConfig,
    state: State,
    head: Header,
}

impl Chain {
    /// Create a chain at `path`: genesis block, empty state, nothing allocated (§7.1).
    pub fn initialize(path: &Path, config: GenesisConfig) -> Result<Self, StorageError> {
        let storage = Storage::open(path)?;
        let state = genesis_state();
        // The genesis block carries no beacon, so there is nothing for the node key to
        // attest beyond the header itself.
        let block = genesis_block(&state, [0u8; 64]);
        storage.initialize(&config, &block)?;
        let head = block.header.clone();
        Ok(Self {
            storage,
            config,
            state,
            head,
        })
    }

    /// Open an existing chain, rebuilding state from the newest checkpoint plus replay.
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        let storage = Storage::open(path)?;
        let config = storage
            .genesis_config()?
            .ok_or(StorageError::MissingGenesis)?;
        let head_height = storage.head_height()?;

        // The identity recorded when this chain was created (§13). Checked before anything
        // is replayed, because the error it produces can name both versions — a state-root
        // mismatch can only say that they differ.
        if let Some((chain_version, chain_lock)) = storage.stored_identity()? {
            if chain_version != CONSENSUS_VERSION || chain_lock != consensus_lock_digest() {
                return Err(StorageError::ForeignConsensus {
                    chain_version,
                    binary_version: CONSENSUS_VERSION,
                    chain_lock,
                    binary_lock: consensus_lock_digest(),
                });
            }
        }

        let (mut state, replay_from) = match storage.latest_checkpoint(head_height)? {
            Some((height, state)) => (state, height + 1),
            None => {
                // Block 0 commits to the genesis state root, and §13's identity is inside it.
                // This is the check that does not depend on the local metadata being honest:
                // it is the same one a third party makes with nothing but exported blocks.
                let state = genesis_state();
                let genesis = storage.block(0)?.ok_or(StorageError::MissingBlock(0))?;
                if genesis.header.state_root != state.state_root() {
                    return Err(StorageError::Database(
                        "the genesis state root is not the one this binary computes: the chain \
                         was created under different rules"
                            .to_string(),
                    ));
                }
                (state, 1)
            }
        };

        // Replay whatever the checkpoint does not cover. The blocks are the source of truth,
        // so this can only ever agree with them.
        let foundation = config.foundation_account();
        for height in replay_from..=head_height {
            let block = storage
                .block(height)?
                .ok_or(StorageError::MissingBlock(height))?;
            let output = execute_batch(&mut state, batch_input_from_block(&block, foundation));
            if output.header.state_root != block.header.state_root {
                return Err(StorageError::Database(format!(
                    "replay diverged at height {height}"
                )));
            }
        }

        // And the identity carried by the state itself, which is what every state root
        // commits to. A checkpoint restored from disk keeps whatever it was stamped with, so
        // this catches a mismatch the metadata alone would not.
        if state.global.consensus_version != CONSENSUS_VERSION
            || state.global.lock_digest != consensus_lock_digest()
        {
            return Err(StorageError::ForeignConsensus {
                chain_version: state.global.consensus_version,
                binary_version: CONSENSUS_VERSION,
                chain_lock: state.global.lock_digest,
                binary_lock: consensus_lock_digest(),
            });
        }

        let head = storage
            .block(head_height)?
            .ok_or(StorageError::MissingBlock(head_height))?
            .header;

        Ok(Self {
            storage,
            config,
            state,
            head,
        })
    }

    pub fn config(&self) -> &GenesisConfig {
        &self.config
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    pub fn head(&self) -> &Header {
        &self.head
    }

    pub fn storage(&self) -> &Storage {
        &self.storage
    }

    /// Height of the block to be produced next.
    pub fn next_height(&self) -> u64 {
        self.head.height + 1
    }

    /// The drand round the next block maps to (§3.4). Rounds are never skipped, so after
    /// downtime this walks forward one block at a time rather than jumping to "now".
    pub fn next_round(&self) -> u64 {
        round_for_height(self.config.genesis_drand_round, self.next_height())
    }

    /// Execute a batch, sign the header and commit — block and state in one transaction.
    pub fn produce(
        &mut self,
        drand_signature: Vec<u8>,
        blob_manifest: Vec<[u8; 32]>,
        unusable: Vec<[u8; 32]>,
        txs: Vec<SignedTx>,
        blobs: Vec<([u8; 32], Vec<u8>)>,
        node_key: &SigningKey,
    ) -> Result<Block, StorageError> {
        let input = BatchInput {
            height: self.next_height(),
            prev_hash: self.head.block_hash(),
            drand_round: self.next_round(),
            drand_signature,
            blob_manifest,
            unusable,
            txs,
            foundation: self.config.foundation_account(),
        };

        let output = execute_batch(&mut self.state, input);
        let signature = sign(node_key, &output.header.block_hash());
        let block = output.into_block(signature);

        self.storage.commit_batch(&block, &self.state, &blobs)?;
        self.head = block.header.clone();
        Ok(block)
    }
}

/// Rebuild the batch input a block records, for replay.
///
/// Everything here comes out of the block itself, which is what makes replay self-contained:
/// the transactions are in the clear, the beacon signature is stored, and the ordering is
/// recomputed rather than trusted.
pub fn batch_input_from_block(block: &Block, foundation: [u8; 32]) -> BatchInput {
    BatchInput {
        height: block.header.height,
        prev_hash: block.header.prev_hash,
        drand_round: block.header.drand_round,
        drand_signature: block.drand_signature.clone(),
        blob_manifest: block.blob_manifest.clone(),
        unusable: block.unusable.clone(),
        // Replay feeds back both executed and rejected transactions? No: rejected ones carry
        // only their id in the block, so replay re-runs the executed set and re-derives the
        // same rejections only if it also sees the rejected transactions. It does not, by
        // design — see verify.rs, which checks `rejected_root` against the recorded pairs
        // instead of re-deriving them.
        txs: block.txs.clone(),
        foundation,
    }
}
