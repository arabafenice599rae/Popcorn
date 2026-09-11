//! Per-batch emission with halving (SPEC.md §7.2) — the only source of new supply.

use crate::constants::{EMISSION_0, EMISSION_STAKER_BPS, HALVING_INTERVAL};
use crate::types::Amount;

/// Nominal emission for a block height.
///
/// The index is 0-based (`height − 1`), so each epoch contains exactly `HALVING_INTERVAL`
/// batches: blocks `1..=HALVING_INTERVAL` are epoch 0.
pub fn emission_at(height: u64) -> Amount {
    if height == 0 {
        return 0; // genesis emits nothing
    }
    let emission_index = height - 1;
    let epoch = emission_index / HALVING_INTERVAL;
    if epoch >= 128 {
        return 0;
    }
    EMISSION_0 >> epoch
}

/// Split of a nominal emission into staker and foundation shares.
///
/// The foundation share is the remainder, so the two always sum back to the nominal amount:
/// the 15% cap is exact on the nominal share, never one unit more.
pub fn split(emission: Amount) -> (Amount, Amount) {
    let staker_share = emission * EMISSION_STAKER_BPS / 10_000;
    let foundation_share = emission - staker_share;
    (staker_share, foundation_share)
}

/// Exact upper bound on total supply: `GENESIS_SUPPLY + HALVING_INTERVAL × Σ (EMISSION_0 >> i)`.
///
/// The effective figure is lower, since staker-less batches never mint their staker share.
pub fn supply_upper_bound(genesis_supply: Amount) -> Amount {
    let mut total: Amount = 0;
    for i in 0..128u32 {
        let per_batch = EMISSION_0 >> i;
        if per_batch == 0 {
            break;
        }
        total += per_batch * HALVING_INTERVAL as u128;
    }
    genesis_supply + total
}
