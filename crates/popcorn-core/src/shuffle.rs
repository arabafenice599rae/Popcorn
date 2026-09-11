//! Deterministic ordering (SPEC.md §3.7 and §5.3).
//!
//! The order of a batch is a function of the beacon and nothing else. The operator picks no
//! positions, and a verifier recomputes the same permutation from the block's own drand
//! signature.

use std::collections::BTreeMap;

use crate::types::{AccountId, SignedTx};

/// The XOF stream seeded by `blake3(drand_signature ‖ LE64(height))`.
///
/// The stream only ever advances: a rejected sample consumes its bytes and the next draw
/// reads the following eight. That is part of the consensus definition, not an
/// implementation choice — re-reading would yield a different permutation.
pub struct BeaconRng {
    reader: blake3::OutputReader,
}

impl BeaconRng {
    pub fn new(drand_signature: &[u8], height: u64) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(drand_signature);
        hasher.update(&height.to_le_bytes());
        Self {
            reader: hasher.finalize_xof(),
        }
    }

    /// Eight bytes from the stream, little-endian.
    pub fn next_u64(&mut self) -> u64 {
        let mut buf = [0u8; 8];
        self.reader.fill(&mut buf);
        u64::from_le_bytes(buf)
    }

    /// Uniform sample over `[0, n)` by rejection sampling.
    ///
    /// The accepted set `[0, limit)` has cardinality `limit`, an exact multiple of `n`, so
    /// `x % n` is exactly uniform — no modulo bias.
    pub fn uniform(&mut self, n: u64) -> u64 {
        assert!(n > 0, "uniform(0) is undefined");
        let limit = u64::MAX - (u64::MAX % n);
        loop {
            let x = self.next_u64();
            if x < limit {
                return x % n;
            }
        }
    }
}

/// Fisher-Yates over a list already sorted by ascending `tx_id`.
pub fn shuffle<T>(items: &mut [T], rng: &mut BeaconRng) {
    if items.len() < 2 {
        return;
    }
    for i in (1..items.len()).rev() {
        let j = rng.uniform(i as u64 + 1) as usize;
        items.swap(i, j);
    }
}

/// Per-account nonce normalization (§5.3).
///
/// The shuffle assigns positions; this reassigns each account's own transactions to its own
/// positions in ascending nonce order. Transactions of different accounts never move, so the
/// distribution of positions stays uniform while a contiguous nonce run can no longer fail
/// because of internal disorder.
pub fn normalize_nonces(txs: &mut [SignedTx]) {
    let mut positions: BTreeMap<AccountId, Vec<usize>> = BTreeMap::new();
    for (index, tx) in txs.iter().enumerate() {
        positions.entry(tx.signer()).or_default().push(index);
    }

    for slots in positions.values() {
        if slots.len() < 2 {
            continue;
        }
        // `slots` is ascending by construction (we enumerated in order).
        let mut owned: Vec<SignedTx> = slots.iter().map(|&i| txs[i].clone()).collect();
        owned.sort_by_key(|tx| tx.payload.nonce);
        for (slot, tx) in slots.iter().zip(owned) {
            txs[*slot] = tx;
        }
    }
}
