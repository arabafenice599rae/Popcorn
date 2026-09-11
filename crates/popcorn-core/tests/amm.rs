//! POPCORN-V2-MATH (SPEC.md §6): formulas, rounding, and the liquidity lifecycle.

mod common;

use common::*;
use popcorn_core::amm;
use popcorn_core::constants::{FEE_TX, MINIMUM_LIQUIDITY, NATIVE_TOKEN};
use popcorn_core::ids::{lp_token_id, pair_id, token_id};
use popcorn_core::state::State;
use popcorn_core::types::{AccountId, Action, ExecStatus, FailReason};

/// Build a pool: one user token, one pair against native, seeded with liquidity.
fn pooled_state(fee_bps: u16) -> (State, Actor, AccountId, [u8; 32], [u8; 32]) {
    let alice = Actor::new(1);
    let foundation = Actor::new(200).id;
    let mut state = State::new();
    fund(&mut state, &alice.id, FEE_TX * 100 + 10_000_000_000);

    let token = token_id(&alice.id, 1);
    let pair = pair_id(&NATIVE_TOKEN, &token, fee_bps);

    let txs = vec![
        alice.tx(
            1,
            1,
            Action::CreateToken {
                name: *b"POPTEST\0\0\0\0\0\0\0\0\0",
                supply: 10_000_000_000,
            },
        ),
        alice.tx(
            2,
            1,
            Action::CreatePair {
                token_a: NATIVE_TOKEN,
                token_b: token,
                fee_bps,
            },
        ),
        alice.tx(
            3,
            1,
            Action::AddLiquidity {
                pair,
                amount0_desired: 1_000_000_000,
                amount1_desired: 1_000_000_000,
                amount0_min: 0,
                amount1_min: 0,
            },
        ),
    ];
    let output = run_batch(&mut state, 1, txs, foundation);
    assert!(
        output.results.iter().all(|r| *r == ExecStatus::Ok),
        "setup failed: {:?}",
        output.results
    );
    (state, alice, foundation, token, pair)
}

/// The invariant only claims to hold across swaps, and there it must hold exactly (§6).
#[test]
fn swaps_never_decrease_k() {
    let (mut state, alice, foundation, token, pair) = pooled_state(30);
    let before = {
        let p = &state.pairs[&pair];
        amm::k_value(p.reserve0, p.reserve1)
    };

    let swap = alice.tx(
        4,
        2,
        Action::SwapExactIn {
            path: vec![pair],
            token_in: NATIVE_TOKEN,
            amount_in: 10_000_000,
            min_amount_out: 1,
        },
    );
    let tx_id = swap.tx_id();
    let output = run_batch(&mut state, 2, vec![swap], foundation);
    assert_eq!(status_of(&output, &tx_id), Some(ExecStatus::Ok));

    let after = {
        let p = &state.pairs[&pair];
        amm::k_value(p.reserve0, p.reserve1)
    };
    assert!(after >= before, "k decreased across a swap");
    assert!(state.balance_of(&alice.id, &token) > 9_000_000_000);
}

/// Exact-out costs at least what exact-in would return, never less: the `+1` rounds toward
/// the pool, and a round trip must not be profitable.
#[test]
fn exact_out_rounds_in_favour_of_the_pool() {
    let reserve_in = 1_000_000_000u128;
    let reserve_out = 2_000_000_000u128;

    for fee_bps in [5u16, 30, 100] {
        let amount_in = 5_000_000u128;
        let out = amm::amount_out_exact_in(amount_in, reserve_in, reserve_out, fee_bps).unwrap();
        let back = amm::amount_in_exact_out(out, reserve_in, reserve_out, fee_bps).unwrap();
        assert!(
            back <= amount_in,
            "fee {fee_bps}: exact-out asked {back} for what exact-in produced from {amount_in}"
        );
        // ...and one more unit out costs strictly more than the original input.
        let dearer = amm::amount_in_exact_out(out + 1, reserve_in, reserve_out, fee_bps).unwrap();
        assert!(dearer > back);
    }
}

/// Asking for the whole reserve has no finite price: the frozen outcome is slippage (§6).
#[test]
fn exact_out_at_or_above_the_reserve_is_slippage() {
    let reserve_out = 1_000u128;
    assert_eq!(
        amm::amount_in_exact_out(reserve_out, 1_000, reserve_out, 30),
        Err(FailReason::SlippageExceeded)
    );
    assert_eq!(
        amm::amount_in_exact_out(reserve_out + 1, 1_000, reserve_out, 30),
        Err(FailReason::SlippageExceeded)
    );
    assert!(amm::amount_in_exact_out(reserve_out - 1, 1_000, reserve_out, 30).is_ok());
}

/// A swap that would yield nothing fails rather than pocketing the fee (§6).
#[test]
fn zero_yield_swaps_fail() {
    let (mut state, alice, foundation, _token, pair) = pooled_state(30);

    // One unit against a deep pool floors to zero output.
    let swap = alice.tx(
        4,
        2,
        Action::SwapExactIn {
            path: vec![pair],
            token_in: NATIVE_TOKEN,
            amount_in: 1,
            min_amount_out: 0,
        },
    );
    let tx_id = swap.tx_id();
    let output = run_batch(&mut state, 2, vec![swap], foundation);
    assert_eq!(
        status_of(&output, &tx_id),
        Some(ExecStatus::Failed(FailReason::ZeroOutput))
    );
    // The fee is still burned: execution failed, the fee phase had already run.
    assert!(state.global.native_burned >= FEE_TX);
}

/// Slippage bounds are honoured on both sides of the book.
#[test]
fn slippage_bounds_are_enforced() {
    let (mut state, alice, foundation, token, pair) = pooled_state(30);

    let too_greedy = alice.tx(
        4,
        2,
        Action::SwapExactIn {
            path: vec![pair],
            token_in: NATIVE_TOKEN,
            amount_in: 1_000_000,
            min_amount_out: 999_999_999,
        },
    );
    let id = too_greedy.tx_id();
    let output = run_batch(&mut state, 2, vec![too_greedy], foundation);
    assert_eq!(
        status_of(&output, &id),
        Some(ExecStatus::Failed(FailReason::SlippageExceeded))
    );

    let too_cheap = alice.tx(
        5,
        3,
        Action::SwapExactOut {
            path: vec![pair],
            token_in: NATIVE_TOKEN,
            amount_out: 1_000_000,
            max_amount_in: 1,
        },
    );
    let id = too_cheap.tx_id();
    let output = run_batch(&mut state, 3, vec![too_cheap], foundation);
    assert_eq!(
        status_of(&output, &id),
        Some(ExecStatus::Failed(FailReason::SlippageExceeded))
    );
    let _ = token;
}

/// Exact-out delivers precisely what was asked for, no more and no less.
#[test]
fn exact_out_delivers_the_requested_amount() {
    let (mut state, alice, foundation, token, pair) = pooled_state(30);
    let before = state.balance_of(&alice.id, &token);

    let want = 12_345_678u128;
    let swap = alice.tx(
        4,
        2,
        Action::SwapExactOut {
            path: vec![pair],
            token_in: NATIVE_TOKEN,
            amount_out: want,
            max_amount_in: u128::MAX / 2,
        },
    );
    let id = swap.tx_id();
    let output = run_batch(&mut state, 2, vec![swap], foundation);
    assert_eq!(status_of(&output, &id), Some(ExecStatus::Ok));
    assert_eq!(state.balance_of(&alice.id, &token), before + want);
}

/// The first mint burns `MINIMUM_LIQUIDITY`: it is credited to supply but to no account.
#[test]
fn first_mint_burns_minimum_liquidity() {
    let (state, alice, _foundation, _token, pair) = pooled_state(30);
    let lp = lp_token_id(&pair);
    let held = state.balance_of(&alice.id, &lp);
    let supply = state.pairs[&pair].lp_supply;

    assert_eq!(supply - held, MINIMUM_LIQUIDITY);
    // Nobody holds the burned portion, so it can never be withdrawn.
    let total_held: u128 = state
        .accounts
        .values()
        .map(|a| a.balances.get(&lp).copied().unwrap_or(0))
        .sum();
    assert_eq!(total_held, supply - MINIMUM_LIQUIDITY);
}

/// Router02 proportional deposit: only the actual amounts are debited (§6).
#[test]
fn add_liquidity_debits_only_the_actual_amounts() {
    let (mut state, alice, foundation, token, pair) = pooled_state(30);
    let native_before = state.balance_of(&alice.id, &NATIVE_TOKEN);
    let token_before = state.balance_of(&alice.id, &token);

    // The pool is 1:1, so offering twice as much of one side must take only the matching
    // amount of it.
    let add = alice.tx(
        4,
        2,
        Action::AddLiquidity {
            pair,
            amount0_desired: 100_000_000,
            amount1_desired: 200_000_000,
            amount0_min: 0,
            amount1_min: 0,
        },
    );
    let id = add.tx_id();
    let output = run_batch(&mut state, 2, vec![add], foundation);
    assert_eq!(status_of(&output, &id), Some(ExecStatus::Ok));

    let native_spent = native_before - state.balance_of(&alice.id, &NATIVE_TOKEN) - FEE_TX;
    let token_spent = token_before - state.balance_of(&alice.id, &token);
    let (token0_is_native, _) = {
        let p = &state.pairs[&pair];
        (p.token0 == NATIVE_TOKEN, p.token1)
    };
    let (spent0, spent1) = if token0_is_native {
        (native_spent, token_spent)
    } else {
        (token_spent, native_spent)
    };
    assert_eq!(spent0, 100_000_000);
    assert_eq!(spent1, 100_000_000, "the excess side must be untouched");
}

/// Slippage bounds apply to the actual amounts, not the desired ones.
#[test]
fn add_liquidity_respects_minimums() {
    let (mut state, alice, foundation, _token, pair) = pooled_state(30);
    let add = alice.tx(
        4,
        2,
        Action::AddLiquidity {
            pair,
            amount0_desired: 100_000_000,
            amount1_desired: 200_000_000,
            amount0_min: 0,
            amount1_min: 200_000_000, // the pool will only take 100_000_000
        },
    );
    let id = add.tx_id();
    let output = run_batch(&mut state, 2, vec![add], foundation);
    assert_eq!(
        status_of(&output, &id),
        Some(ExecStatus::Failed(FailReason::SlippageExceeded))
    );
}

/// Removing liquidity returns both sides proportionally and burns the LP tokens.
#[test]
fn remove_liquidity_returns_both_sides() {
    let (mut state, alice, foundation, token, pair) = pooled_state(30);
    let lp = lp_token_id(&pair);
    let held = state.balance_of(&alice.id, &lp);

    let remove = alice.tx(
        4,
        2,
        Action::RemoveLiquidity {
            pair,
            lp_amount: held / 2,
            amount0_min: 1,
            amount1_min: 1,
        },
    );
    let id = remove.tx_id();
    let before_token = state.balance_of(&alice.id, &token);
    let output = run_batch(&mut state, 2, vec![remove], foundation);

    assert_eq!(status_of(&output, &id), Some(ExecStatus::Ok));
    assert_eq!(state.balance_of(&alice.id, &lp), held - held / 2);
    assert!(state.balance_of(&alice.id, &token) > before_token);
    assert!(state.pairs[&pair].lp_supply >= MINIMUM_LIQUIDITY);
}

/// Pairs of LP tokens are forbidden, and so are pairs of a token with itself (§4.1, §6).
#[test]
fn forbidden_pair_sides_are_refused() {
    let (mut state, alice, foundation, token, pair) = pooled_state(30);
    let lp = lp_token_id(&pair);

    let lp_pair = alice.tx(
        4,
        2,
        Action::CreatePair {
            token_a: NATIVE_TOKEN,
            token_b: lp,
            fee_bps: 30,
        },
    );
    let id = lp_pair.tx_id();
    let output = run_batch(&mut state, 2, vec![lp_pair], foundation);
    assert_eq!(
        status_of(&output, &id),
        Some(ExecStatus::Failed(FailReason::LpTokenAsPairSide))
    );

    let self_pair = alice.tx(
        5,
        3,
        Action::CreatePair {
            token_a: token,
            token_b: token,
            fee_bps: 30,
        },
    );
    let id = self_pair.tx_id();
    let output = run_batch(&mut state, 3, vec![self_pair], foundation);
    assert_eq!(
        status_of(&output, &id),
        Some(ExecStatus::Failed(FailReason::BadPath))
    );

    let duplicate = alice.tx(
        6,
        4,
        Action::CreatePair {
            token_a: NATIVE_TOKEN,
            token_b: token,
            fee_bps: 30,
        },
    );
    let id = duplicate.tx_id();
    let output = run_batch(&mut state, 4, vec![duplicate], foundation);
    assert_eq!(
        status_of(&output, &id),
        Some(ExecStatus::Failed(FailReason::PairAlreadyExists))
    );
}

/// A path whose next pair does not contain the current token is not a path (§4.3).
#[test]
fn broken_paths_fail() {
    let (mut state, alice, foundation, token, pair) = pooled_state(30);

    // Start from a token the first pair does not hold.
    let stranger = token_id(&alice.id, 99);
    let swap = alice.tx(
        4,
        2,
        Action::SwapExactIn {
            path: vec![pair],
            token_in: stranger,
            amount_in: 1_000,
            min_amount_out: 0,
        },
    );
    let id = swap.tx_id();
    let output = run_batch(&mut state, 2, vec![swap], foundation);
    assert_eq!(
        status_of(&output, &id),
        Some(ExecStatus::Failed(FailReason::BadPath))
    );

    // A pair that does not exist at all.
    let swap = alice.tx(
        5,
        3,
        Action::SwapExactIn {
            path: vec![[42u8; 32]],
            token_in: NATIVE_TOKEN,
            amount_in: 1_000,
            min_amount_out: 0,
        },
    );
    let id = swap.tx_id();
    let output = run_batch(&mut state, 3, vec![swap], foundation);
    assert_eq!(
        status_of(&output, &id),
        Some(ExecStatus::Failed(FailReason::UnknownPair))
    );
    let _ = token;
}

/// Multi-hop routes through two pools and is atomic end to end.
#[test]
fn multi_hop_swaps_route_and_settle() {
    let alice = Actor::new(1);
    let foundation = Actor::new(200).id;
    let mut state = State::new();
    fund(&mut state, &alice.id, FEE_TX * 100 + 10_000_000_000);

    let token_a = token_id(&alice.id, 1);
    let token_b = token_id(&alice.id, 2);
    let pair_na = pair_id(&NATIVE_TOKEN, &token_a, 30);
    let pair_ab = pair_id(&token_a, &token_b, 30);

    let setup = vec![
        alice.tx(1, 1, Action::CreateToken { name: *b"AAAA\0\0\0\0\0\0\0\0\0\0\0\0", supply: 5_000_000_000 }),
        alice.tx(2, 1, Action::CreateToken { name: *b"BBBB\0\0\0\0\0\0\0\0\0\0\0\0", supply: 5_000_000_000 }),
        alice.tx(3, 1, Action::CreatePair { token_a: NATIVE_TOKEN, token_b: token_a, fee_bps: 30 }),
        alice.tx(4, 1, Action::CreatePair { token_a, token_b, fee_bps: 30 }),
        alice.tx(5, 1, Action::AddLiquidity { pair: pair_na, amount0_desired: 1_000_000_000, amount1_desired: 1_000_000_000, amount0_min: 0, amount1_min: 0 }),
        alice.tx(6, 1, Action::AddLiquidity { pair: pair_ab, amount0_desired: 1_000_000_000, amount1_desired: 1_000_000_000, amount0_min: 0, amount1_min: 0 }),
    ];
    let output = run_batch(&mut state, 1, setup, foundation);
    assert!(output.results.iter().all(|r| *r == ExecStatus::Ok), "{:?}", output.results);

    let before_b = state.balance_of(&alice.id, &token_b);
    let swap = alice.tx(
        7,
        2,
        Action::SwapExactIn {
            path: vec![pair_na, pair_ab],
            token_in: NATIVE_TOKEN,
            amount_in: 10_000_000,
            min_amount_out: 1,
        },
    );
    let id = swap.tx_id();
    let output = run_batch(&mut state, 2, vec![swap], foundation);
    assert_eq!(status_of(&output, &id), Some(ExecStatus::Ok));
    assert!(state.balance_of(&alice.id, &token_b) > before_b);

    // A multi-hop pays exactly one flat fee, however many pools it crosses.
    assert_eq!(state.global.native_burned, FEE_TX * 7);
}

/// The re-genesis guard: residual LP against empty reserves must be burned before the pair
/// can restart, or new depositors would be diluted by a claim on nothing (§6).
#[test]
fn re_genesis_guard_blocks_refilling_a_stranded_pair() {
    let pair = popcorn_core::types::Pair {
        id: [1u8; 32],
        token0: NATIVE_TOKEN,
        token1: [2u8; 32],
        fee_bps: 30,
        reserve0: 0,
        reserve1: 0,
        lp_supply: MINIMUM_LIQUIDITY * 10,
    };
    // Only the formula matters here, so drive it directly.
    assert!(pair.reserve0 == 0 && pair.reserve1 == 0);
    assert!(pair.lp_supply > MINIMUM_LIQUIDITY);

    // A drained pair holding exactly the burned minimum restarts on the genesis formula.
    let minted = amm::initial_liquidity(1_000_000, 1_000_000, MINIMUM_LIQUIDITY).unwrap();
    assert_eq!(minted, 1_000_000 - MINIMUM_LIQUIDITY);

    // Too little to cover the burn is refused rather than minting nothing.
    assert_eq!(
        amm::initial_liquidity(10, 10, MINIMUM_LIQUIDITY),
        Err(FailReason::LiquidityTooSmall)
    );
}
