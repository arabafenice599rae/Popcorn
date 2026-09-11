//! Staking distribution (SPEC.md §8): a literal transcription of Synthetix
//! `StakingRewards`.
//!
//! The shape matters more than it looks. The reserve takes `staker_share` **whole**, and the
//! only floor is the user-side one over the accumulator *difference*. That is what makes
//! `staking_reserved ≥ Σ pending ≥ 0` hold by construction rather than by hope: an earlier
//! design floored twice over different bases, and the first claim after a dust payout could
//! underflow the reserve.

use primitive_types::U256;

use crate::constants::PRECISION;
use crate::types::{Account, Amount, FailReason};

/// Claimable reward of an account: `⌊staked × (acc − paid_acc) / PRECISION⌋`.
///
/// One floor, over the difference — Synthetix `earned`. The product needs U256 (it reaches
/// ~10⁵⁰ in the dust-staker case); the result always fits in `u128` because supply is
/// finite.
pub fn pending(account: &Account, acc_per_stake: u128) -> Amount {
    if account.staked == 0 {
        return 0;
    }
    // The accumulator is monotonically non-decreasing, so this cannot underflow; a
    // saturating difference keeps a corrupted state from panicking a node.
    let delta = acc_per_stake.saturating_sub(account.paid_acc);
    if delta == 0 {
        return 0;
    }
    let product = U256::from(account.staked) * U256::from(delta);
    let quotient = product / U256::from(PRECISION);
    // Bounded by total emission, hence always below u128::MAX (§8).
    debug_assert!(quotient <= U256::from(u128::MAX));
    quotient.low_u128()
}

/// Accumulator increment for one batch: `⌊staker_share × PRECISION / total_staked⌋`.
///
/// Returns `None` when nothing is staked — in that batch the staker share is not emitted at
/// all (§7.2), so there is nothing to distribute.
pub fn accumulator_increment(staker_share: Amount, total_staked: Amount) -> Option<u128> {
    if total_staked == 0 {
        return None;
    }
    let numerator = U256::from(staker_share) * U256::from(PRECISION);
    let quotient = numerator / U256::from(total_staked);
    debug_assert!(quotient <= U256::from(u128::MAX));
    Some(quotient.low_u128())
}

/// Settle an account's pending reward: move it out of the reserve and into the balance.
///
/// This is a **transfer**, not an emission: `reserve − p` and `balance + p` keep the
/// four-bucket total unchanged, which is exactly what the property-test gate asserts after
/// every operation.
///
/// Returns the settled amount so the caller can credit the balance; the caller must then
/// update `staked` and finally snapshot `paid_acc`, in that order (§8).
pub fn settle_amount(
    account: &Account,
    acc_per_stake: u128,
    staking_reserved: Amount,
) -> Result<Amount, FailReason> {
    let p = pending(account, acc_per_stake);
    if p > staking_reserved {
        // Unreachable by the solvency proof in §8; treating it as a failure rather than a
        // silent wrap means a broken implementation shows up as a failed transaction
        // instead of a corrupted ledger.
        return Err(FailReason::Overflow);
    }
    Ok(p)
}
