//! Blind collection (SPEC.md §5.1 phase 1, §9.2, §11).
//!
//! Everything here happens before the node can read anything: it queues bytes it cannot
//! decrypt, and signs a receipt for each one. That receipt is the whole accountability story
//! — omitting a receipted blob from the manifest is a contradiction between two signatures by
//! the same node — so issuing it is not optional bookkeeping, it is the product.
//!
//! Because the phase is blind there are no fees to throttle it, so the defences are
//! wire-level and sit outside consensus (§11).

use std::collections::BTreeMap;
use std::sync::Mutex;

use ed25519_dalek::SigningKey;
use popcorn_core::constants::{
    BLOB_ROUND_HORIZON, MAX_BLOB_SIZE, MAX_TX_PER_BATCH, RECEIPT_DOMAIN,
};
use popcorn_core::crypto::{blake3_hash, sign};
use popcorn_core::types::{Receipt, ReceiptPayload};

/// Byte ceiling accepted per round (§11). At least `MAX_TX_PER_BATCH × MAX_BLOB_SIZE`.
pub const MAX_TOTAL_INGRESS_PER_ROUND: usize = MAX_TX_PER_BATCH * MAX_BLOB_SIZE;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubmitError {
    /// Larger than the wire limit on an encrypted blob.
    TooLarge(usize),
    /// The target round has already been collected or is in the past.
    RoundClosed { target: u64, current: u64 },
    /// Beyond `BLOB_ROUND_HORIZON`. The timelock is fair ordering over minutes, never
    /// long-term storage (§3.5).
    BeyondHorizon { target: u64, current: u64 },
    /// The round's admission budget is spent.
    RoundFull,
}

impl std::fmt::Display for SubmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SubmitError::TooLarge(size) => {
                write!(f, "blob is {size} bytes, limit is {MAX_BLOB_SIZE}")
            }
            SubmitError::RoundClosed { target, current } => {
                write!(f, "round {target} is closed (current {current})")
            }
            SubmitError::BeyondHorizon { target, current } => write!(
                f,
                "round {target} is beyond the {BLOB_ROUND_HORIZON}-round horizon from {current}"
            ),
            SubmitError::RoundFull => write!(f, "round admission budget is spent"),
        }
    }
}

#[derive(Default)]
struct RoundQueue {
    /// Keyed by blob hash, which makes the queue a set: the same blob submitted twice is one
    /// manifest entry, however many receipts it earned (§5.1).
    blobs: BTreeMap<[u8; 32], Vec<u8>>,
    bytes: usize,
}

pub struct Mempool {
    rounds: Mutex<BTreeMap<u64, RoundQueue>>,
}

impl Mempool {
    pub fn new() -> Self {
        Self {
            rounds: Mutex::new(BTreeMap::new()),
        }
    }

    /// Queue a blob blindly and return its signed receipt.
    ///
    /// The node has no idea what it just accepted — that is the point — so the only checks
    /// possible here are on size, round and budget.
    pub fn submit(
        &self,
        blob: Vec<u8>,
        target_round: u64,
        current_round: u64,
        node_key: &SigningKey,
        timestamp_ms: u64,
    ) -> Result<Receipt, SubmitError> {
        if blob.len() > MAX_BLOB_SIZE {
            return Err(SubmitError::TooLarge(blob.len()));
        }
        if target_round < current_round {
            return Err(SubmitError::RoundClosed {
                target: target_round,
                current: current_round,
            });
        }
        if target_round > current_round + BLOB_ROUND_HORIZON {
            return Err(SubmitError::BeyondHorizon {
                target: target_round,
                current: current_round,
            });
        }

        let blob_hash = blake3_hash(&blob);
        {
            let mut rounds = self.rounds.lock().unwrap();
            let queue = rounds.entry(target_round).or_default();
            if !queue.blobs.contains_key(&blob_hash) {
                if queue.blobs.len() >= MAX_TX_PER_BATCH
                    || queue.bytes + blob.len() > MAX_TOTAL_INGRESS_PER_ROUND
                {
                    return Err(SubmitError::RoundFull);
                }
                queue.bytes += blob.len();
                queue.blobs.insert(blob_hash, blob);
            }
        }

        // A receipt is issued even for a blob already queued: the submitter is entitled to
        // evidence of receipt, and the manifest entry is the same either way.
        let payload = ReceiptPayload {
            domain: RECEIPT_DOMAIN.to_string(),
            blob_hash,
            target_round,
            timestamp_ms,
        };
        let signature = sign(node_key, &payload.receipt_hash());
        Ok(Receipt {
            payload,
            node_pubkey: node_key.verifying_key().to_bytes(),
            signature,
        })
    }

    /// Freeze and remove a round's collection, in lexicographic hash order.
    ///
    /// This is the commitment moment of §5.1: after it, what was received is fixed.
    pub fn take_round(&self, round: u64) -> Vec<([u8; 32], Vec<u8>)> {
        let mut rounds = self.rounds.lock().unwrap();
        let queue = rounds.remove(&round).unwrap_or_default();
        // BTreeMap iteration is already lexicographic by hash.
        queue.blobs.into_iter().collect()
    }

    /// Drop rounds that can no longer be produced (after downtime, say).
    pub fn discard_before(&self, round: u64) {
        let mut rounds = self.rounds.lock().unwrap();
        let stale: Vec<u64> = rounds.range(..round).map(|(k, _)| *k).collect();
        for key in stale {
            rounds.remove(&key);
        }
    }

    /// Number of blobs queued for a round.
    pub fn queued(&self, round: u64) -> usize {
        self.rounds
            .lock()
            .unwrap()
            .get(&round)
            .map(|q| q.blobs.len())
            .unwrap_or(0)
    }
}

impl Default for Mempool {
    fn default() -> Self {
        Self::new()
    }
}
