//! POPCORN-V2-MATH (SPEC.md §6): Uniswap V2 generalized to fee tiers.
//!
//! Every intermediate is computed in `U256` and converted back to `u128` with a checked
//! conversion; every division floors; rounding always favours the pool. These are consensus
//! rules — a different rounding is a different chain.

use primitive_types::U256;

use crate::types::{Amount, FailReason};

/// Basis-point denominator.
const BPS: u128 = 10_000;

/// Convert a U256 intermediate back to `u128`, or report the frozen overflow outcome.
fn to_u128(value: U256) -> Result<Amount, FailReason> {
    if value > U256::from(u128::MAX) {
        Err(FailReason::Overflow)
    } else {
        Ok(value.low_u128())
    }
}

/// Output of one exact-in hop.
///
/// `amount_out = (amount_in·fee_num·reserve_out) / (reserve_in·10_000 + amount_in·fee_num)`
pub fn amount_out_exact_in(
    amount_in: Amount,
    reserve_in: Amount,
    reserve_out: Amount,
    fee_bps: u16,
) -> Result<Amount, FailReason> {
    let fee_num = BPS - fee_bps as u128;
    let amount_in_with_fee = U256::from(amount_in) * U256::from(fee_num);
    let numerator = amount_in_with_fee * U256::from(reserve_out);
    let denominator = U256::from(reserve_in) * U256::from(BPS) + amount_in_with_fee;
    if denominator.is_zero() {
        // Only reachable with a zero input into an empty reserve; static validation
        // already forbids a zero input, so this is defence in depth.
        return Err(FailReason::ZeroOutput);
    }
    to_u128(numerator / denominator)
}

/// Input required by one exact-out hop.
///
/// `amount_in = (reserve_in·amount_out·10_000) / ((reserve_out − amount_out)·fee_num) + 1`
///
/// The `+1` is the pool-favouring correction: it makes the pool whole after flooring.
pub fn amount_in_exact_out(
    amount_out: Amount,
    reserve_in: Amount,
    reserve_out: Amount,
    fee_bps: u16,
) -> Result<Amount, FailReason> {
    // Asking for the whole reserve (or more) has no finite price: the frozen outcome is
    // slippage, not an overflow or a panic (§6).
    if amount_out >= reserve_out {
        return Err(FailReason::SlippageExceeded);
    }
    let fee_num = BPS - fee_bps as u128;
    let numerator = U256::from(reserve_in) * U256::from(amount_out) * U256::from(BPS);
    let denominator = U256::from(reserve_out - amount_out) * U256::from(fee_num);
    let quotient = numerator / denominator;
    to_u128(quotient + U256::one())
}

/// Liquidity minted by the first deposit into a pair (genesis and re-genesis branches).
///
/// `MINIMUM_LIQUIDITY` is credited to `lp_supply` but to no account: it is burned, which is
/// what stops the first depositor from manipulating the share price to zero.
pub fn initial_liquidity(
    amount0: Amount,
    amount1: Amount,
    minimum_liquidity: Amount,
) -> Result<Amount, FailReason> {
    let product = U256::from(amount0) * U256::from(amount1);
    let root = to_u128(product.integer_sqrt())?;
    let liquidity = root
        .checked_sub(minimum_liquidity)
        .ok_or(FailReason::LiquidityTooSmall)?;
    if liquidity == 0 {
        return Err(FailReason::LiquidityTooSmall);
    }
    Ok(liquidity)
}

/// Liquidity minted by a deposit into a pair that already holds reserves.
pub fn subsequent_liquidity(
    amount0: Amount,
    amount1: Amount,
    reserve0: Amount,
    reserve1: Amount,
    lp_supply: Amount,
) -> Result<Amount, FailReason> {
    if reserve0 == 0 || reserve1 == 0 {
        return Err(FailReason::ReGenesisGuard);
    }
    let from0 = U256::from(amount0) * U256::from(lp_supply) / U256::from(reserve0);
    let from1 = U256::from(amount1) * U256::from(lp_supply) / U256::from(reserve1);
    let liquidity = to_u128(from0.min(from1))?;
    if liquidity == 0 {
        return Err(FailReason::LiquidityTooSmall);
    }
    Ok(liquidity)
}

/// The Router02 proportional deposit (§6): how much of each side is actually taken.
///
/// Only the returned amounts are ever debited — the excess a depositor offered is never
/// touched.
pub fn actual_deposit(
    amount0_desired: Amount,
    amount1_desired: Amount,
    reserve0: Amount,
    reserve1: Amount,
) -> Result<(Amount, Amount), FailReason> {
    let a1_opt = to_u128(U256::from(amount0_desired) * U256::from(reserve1) / U256::from(reserve0))?;
    if a1_opt <= amount1_desired {
        Ok((amount0_desired, a1_opt))
    } else {
        let a0_opt =
            to_u128(U256::from(amount1_desired) * U256::from(reserve0) / U256::from(reserve1))?;
        Ok((a0_opt, amount1_desired))
    }
}

/// Withdrawal proceeds for `lp_amount` burned against a reserve.
pub fn withdrawal_amount(
    lp_amount: Amount,
    reserve: Amount,
    lp_supply: Amount,
) -> Result<Amount, FailReason> {
    if lp_supply == 0 {
        return Err(FailReason::UnknownPair);
    }
    to_u128(U256::from(lp_amount) * U256::from(reserve) / U256::from(lp_supply))
}

/// The constant-product value `k`, in U256 so it never overflows.
///
/// Only meaningful across successful swap hops (§6): liquidity actions change `k` by
/// definition.
pub fn k_value(reserve0: Amount, reserve1: Amount) -> U256 {
    U256::from(reserve0) * U256::from(reserve1)
}
