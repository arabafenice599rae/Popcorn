//! Third-party verification (SPEC.md §10).
//!
//! Two properties, deliberately kept apart because they need different inputs:
//!
//! * **state replay** is self-contained — transactions travel in the clear inside blocks, so
//!   `/chain/export` alone re-derives every state root;
//! * the **collection audit** is not — deciding whether `unusable` is honest needs the blobs
//!   (from the node or a mirror) and the round's beacon.
//!
//! Conflating them is how a system ends up claiming more than it can show.

use popcorn_core::crypto::{blake3_hash, verify_signature};
use popcorn_core::execute::{
    collection_root, execute_batch, rejected_root, results_root, txs_root,
};
use popcorn_core::genesis::{genesis_state, round_for_height, GenesisConfig};
use popcorn_core::state::State;
use popcorn_core::types::Block;
use popcorn_timelock::{blob as blob_crypto, Beacon};

use crate::chain::batch_input_from_block;
use crate::encoding::to_hex;

/// A divergence between what a block claims and what re-execution produces.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Divergence {
    pub height: u64,
    pub what: String,
}

impl std::fmt::Display for Divergence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "block {}: {}", self.height, self.what)
    }
}

/// Outcome of a replay.
pub struct VerificationReport {
    pub blocks_checked: u64,
    pub divergences: Vec<Divergence>,
    pub final_state: State,
}

impl VerificationReport {
    pub fn is_clean(&self) -> bool {
        self.divergences.is_empty()
    }
}

/// Replay a chain from genesis and check every commitment it makes.
///
/// `blocks` must start at height 0 and be contiguous.
pub fn verify_chain(config: &GenesisConfig, blocks: &[Block]) -> VerificationReport {
    let mut state = genesis_state();
    let mut divergences = Vec::new();
    let mut previous_hash = [0u8; 32];
    let foundation = config.foundation_account();

    for (index, block) in blocks.iter().enumerate() {
        let height = block.header.height;
        let mut push = |what: String| divergences.push(Divergence { height, what });

        if height != index as u64 {
            push(format!("expected height {index}, found {height}"));
            continue;
        }

        // 1. the chain of headers
        if block.header.prev_hash != previous_hash {
            push(format!(
                "prev_hash {} does not match the previous block",
                to_hex(&block.header.prev_hash)
            ));
        }

        if height == 0 {
            // Genesis carries no beacon and no transactions; its state root is the empty
            // state, and nothing is allocated (§7.1).
            if block.header.state_root != state.state_root() {
                push("genesis state root is not the empty state".to_string());
            }
            if !block.txs.is_empty() || !block.rejected.is_empty() {
                push("genesis must contain no transactions".to_string());
            }
            previous_hash = block.header.block_hash();
            continue;
        }

        // 2. the node's signature over the header
        if !verify_signature(
            &config.node_pubkey,
            &block.header.block_hash(),
            &block.node_signature,
        ) {
            push("node signature does not verify".to_string());
        }

        // 3. the beacon commitment and the round mapping (§3.4)
        if block.header.drand_sig_hash != blake3_hash(&block.drand_signature) {
            push("drand_sig_hash does not match the stored signature".to_string());
        }
        let expected_round = round_for_height(config.genesis_drand_round, height);
        if block.header.drand_round != expected_round {
            push(format!(
                "round {} breaks the mapping (expected {expected_round})",
                block.header.drand_round
            ));
        }

        // 4. the roots that commit to the block's own lists
        if block.header.collection_root != collection_root(&block.blob_manifest) {
            push("collection_root does not match blob_manifest".to_string());
        }
        if block.header.rejected_root != rejected_root(&block.rejected) {
            push("rejected_root does not match the rejected list".to_string());
        }
        if block.results.len() != block.txs.len() {
            push("results and txs are not index-aligned".to_string());
        }

        // 5. manifest coherence: unusable is a subset, and both are sorted sets
        if !is_sorted_set(&block.blob_manifest) {
            push("blob_manifest is not a sorted set".to_string());
        }
        if !is_sorted_set(&block.unusable) {
            push("unusable is not a sorted set".to_string());
        }
        for hash in &block.unusable {
            if block.blob_manifest.binary_search(hash).is_err() {
                push(format!(
                    "unusable entry {} is not in the manifest",
                    to_hex(hash)
                ));
            }
        }

        // 6. re-execute: ordering, results and the state root are all re-derived, never
        //    trusted. This is the check the operator cannot cheat.
        let output = execute_batch(&mut state, batch_input_from_block(block, foundation));
        if output.header.txs_root != block.header.txs_root
            || txs_root(&output.txs) != block.header.txs_root
        {
            push("txs_root does not match the re-derived execution order".to_string());
        }
        if results_root(&output.results) != block.header.results_root {
            push("results_root does not match re-execution".to_string());
        }
        if output.results != block.results {
            push("execution results differ from those recorded".to_string());
        }
        if output.header.state_root != block.header.state_root {
            push(format!(
                "state root diverges: recorded {}, replayed {}",
                to_hex(&block.header.state_root),
                to_hex(&output.header.state_root)
            ));
        }

        // 7. the monetary invariant, at every block (§5.5)
        if !state.monetary_invariant_holds(popcorn_core::constants::GENESIS_SUPPLY) {
            push("four-bucket monetary invariant broken".to_string());
        }
        if state.global.staking_reserved < state.total_pending() {
            push("staking reserve does not cover outstanding claims".to_string());
        }

        previous_hash = block.header.block_hash();
    }

    VerificationReport {
        blocks_checked: blocks.len() as u64,
        divergences,
        final_state: state,
    }
}

/// Audit one block's collection against the blobs it manifested (§5.1).
///
/// This is the half of verification that replay cannot do alone: it needs the blobs. Every
/// manifest entry must resolve into exactly one of a transaction in `txs`, a `rejected`
/// entry, or `unusable` — and a false `unusable` claim is refutable by anyone holding the
/// blob and the public beacon.
pub fn audit_collection(
    block: &Block,
    chain_hash: &[u8; 32],
    blobs: &dyn Fn(&[u8; 32]) -> Option<Vec<u8>>,
) -> Vec<Divergence> {
    let mut divergences = Vec::new();
    let height = block.header.height;
    let beacon = Beacon {
        round: block.header.drand_round,
        signature: block.drand_signature.clone(),
    };

    let executed: std::collections::BTreeSet<[u8; 32]> =
        block.txs.iter().map(|tx| tx.tx_id()).collect();
    let rejected: std::collections::BTreeSet<[u8; 32]> =
        block.rejected.iter().map(|(id, _)| *id).collect();

    for hash in &block.blob_manifest {
        let Some(bytes) = blobs(hash) else {
            divergences.push(Divergence {
                height,
                what: format!("blob {} is manifested but not served", to_hex(hash)),
            });
            continue;
        };

        // Re-derive the outcome from the bytes: what the block claims is evidence, not input.
        let decoded = blob_crypto::decrypt(&bytes, chain_hash, &beacon)
            .ok()
            .and_then(|plain| borsh::from_slice::<popcorn_core::SignedTx>(&plain).ok());

        match decoded {
            None => {
                if block.unusable.binary_search(hash).is_err() {
                    divergences.push(Divergence {
                        height,
                        what: format!(
                            "blob {} yields no transaction but is not listed unusable",
                            to_hex(hash)
                        ),
                    });
                }
            }
            Some(tx) => {
                let tx_id = tx.tx_id();
                if block.unusable.binary_search(hash).is_ok() {
                    // The refutation the spec promises: anyone can show the claim is false.
                    divergences.push(Divergence {
                        height,
                        what: format!(
                            "blob {} is claimed unusable but decrypts to transaction {}",
                            to_hex(hash),
                            to_hex(&tx_id)
                        ),
                    });
                } else if !executed.contains(&tx_id) && !rejected.contains(&tx_id) {
                    divergences.push(Divergence {
                        height,
                        what: format!(
                            "transaction {} from a manifested blob appears nowhere in the block",
                            to_hex(&tx_id)
                        ),
                    });
                }
            }
        }
    }

    divergences
}

fn is_sorted_set(items: &[[u8; 32]]) -> bool {
    items.windows(2).all(|pair| pair[0] < pair[1])
}
