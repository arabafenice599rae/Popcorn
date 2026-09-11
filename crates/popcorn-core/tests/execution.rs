//! Validation pipeline, fee phase and batch close (SPEC.md §5.2, §5.3, §7.2, §7.4).

mod common;

use common::*;
use popcorn_core::constants::{
    EMISSION_0, FEE_TX, HALVING_INTERVAL, MAX_TX_PER_ACCOUNT_PER_BATCH, NATIVE_TOKEN,
    PUBLISH_BYTE_FEE, PUBLISH_FREE_BYTES,
};
use popcorn_core::emission;
use popcorn_core::execute::execute_batch;
use popcorn_core::state::{Journal, State};
use popcorn_core::types::{Action, ExecStatus, FailReason, RejectReason};

/// Every executed transaction pays the full fee, `Ok` or `Failed` alike (§5.2).
#[test]
fn failed_transactions_still_pay_the_full_fee() {
    let alice = Actor::new(1);
    let bob = Actor::new(2);
    let foundation = Actor::new(200).id;
    let mut state = State::new();
    fund(&mut state, &alice.id, FEE_TX * 10);

    // More than the balance: execution fails, the fee is already gone.
    let tx = alice.tx(
        1,
        1,
        Action::Transfer {
            token: NATIVE_TOKEN,
            to: bob.id,
            amount: u128::MAX / 2,
        },
    );
    let tx_id = tx.tx_id();
    let output = run_batch(&mut state, 1, vec![tx], foundation);

    assert_eq!(
        status_of(&output, &tx_id),
        Some(ExecStatus::Failed(FailReason::InsufficientBalance))
    );
    assert_eq!(state.global.native_burned, FEE_TX);
    assert_eq!(state.balance_of(&alice.id, &NATIVE_TOKEN), FEE_TX * 9);
    // The nonce is consumed even though nothing happened.
    assert_eq!(state.account(&alice.id).unwrap().nonce, 1);
}

/// The single fee phase closes the bypass where the first transaction drains the balance and
/// every later one rides free (§5.2, the change in v0.8.3).
#[test]
fn draining_the_balance_does_not_buy_free_transactions() {
    let alice = Actor::new(1);
    let bob = Actor::new(2);
    let foundation = Actor::new(200).id;
    let mut state = State::new();

    // Exactly enough for three fees plus a transferable remainder.
    let spendable = 1_000u128;
    fund(&mut state, &alice.id, FEE_TX * 3 + spendable);

    let txs: Vec<_> = (1..=3)
        .map(|nonce| {
            alice.tx(
                nonce,
                1,
                Action::Transfer {
                    token: NATIVE_TOKEN,
                    to: bob.id,
                    amount: spendable,
                },
            )
        })
        .collect();
    let output = run_batch(&mut state, 1, txs, foundation);

    // All three were admitted and all three paid.
    assert_eq!(output.txs.len(), 3);
    assert_eq!(state.global.native_burned, FEE_TX * 3);
    // Only the first transfer can succeed; the rest find an empty balance.
    let ok_count = output
        .results
        .iter()
        .filter(|r| **r == ExecStatus::Ok)
        .count();
    assert_eq!(ok_count, 1);
    assert_eq!(state.balance_of(&bob.id, &NATIVE_TOKEN), spendable);
    assert!(state.monetary_invariant_holds(0));
}

/// Rejected transactions cost nothing and leave the nonce untouched (§5.2).
#[test]
fn rejected_transactions_are_free_and_leave_no_trace() {
    let alice = Actor::new(1);
    let foundation = Actor::new(200).id;
    let mut state = State::new();
    fund(&mut state, &alice.id, FEE_TX * 10);

    // Wrong round.
    let tx = alice.tx(1, 99, Action::Stake { amount: 1 });
    let tx_id = tx.tx_id();
    let output = run_batch(&mut state, 1, vec![tx], foundation);

    assert_eq!(reject_of(&output, &tx_id), Some(RejectReason::WrongRound));
    assert!(output.txs.is_empty());
    assert_eq!(state.global.native_burned, 0);
    assert_eq!(state.account(&alice.id).unwrap().nonce, 0);
    // A rejected transaction must not fix the account's key either (§4.2).
    assert!(state.account(&alice.id).unwrap().pubkey.is_none());
}

/// The key materializes on the first *executed* transaction, and never afterwards changes.
#[test]
fn pubkey_materializes_on_execution_only() {
    let alice = Actor::new(1);
    let foundation = Actor::new(200).id;
    let mut state = State::new();
    fund(&mut state, &alice.id, FEE_TX * 4);

    let tx = alice.tx(1, 1, Action::ClaimRewards {});
    run_batch(&mut state, 1, vec![tx], foundation);

    assert_eq!(
        state.account(&alice.id).unwrap().pubkey,
        Some(alice.pubkey())
    );
}

/// Step 4's sub-order is pinned: `PubkeyMismatch` wins over `NonceExhausted` (§5.2).
#[test]
fn pubkey_mismatch_is_evaluated_before_nonce_exhaustion() {
    let alice = Actor::new(1);
    let foundation = Actor::new(200).id;
    let mut state = State::new();
    fund(&mut state, &alice.id, FEE_TX * 4);

    // Craft an account that is both terminal and bound to a different key.
    {
        let mut journal = Journal::new();
        let account = state.account_mut(&alice.id, &mut journal).unwrap();
        account.nonce = u64::MAX;
        account.pubkey = Some([9u8; 32]);
    }

    let tx = alice.tx(1, 1, Action::ClaimRewards {});
    let tx_id = tx.tx_id();
    let output = run_batch(&mut state, 1, vec![tx], foundation);
    assert_eq!(
        reject_of(&output, &tx_id),
        Some(RejectReason::PubkeyMismatch)
    );

    // With the key coherent, the terminal nonce is what rejects it.
    {
        let mut journal = Journal::new();
        state.account_mut(&alice.id, &mut journal).unwrap().pubkey = Some(alice.pubkey());
    }
    let tx = alice.tx(1, 2, Action::ClaimRewards {});
    let tx_id = tx.tx_id();
    let output = run_batch(&mut state, 2, vec![tx], foundation);
    assert_eq!(
        reject_of(&output, &tx_id),
        Some(RejectReason::NonceExhausted)
    );
}

/// Two signatures over the same payload are two transactions; the smaller tx_id survives.
#[test]
fn duplicate_nonce_keeps_the_smallest_tx_id() {
    let alice = Actor::new(1);
    let bob = Actor::new(2);
    let foundation = Actor::new(200).id;
    let mut state = State::new();
    fund(&mut state, &alice.id, FEE_TX * 10);

    // Same nonce, different actions, hence different ids.
    let first = alice.tx(1, 1, Action::ClaimRewards {});
    let second = alice.tx(
        1,
        1,
        Action::Transfer {
            token: NATIVE_TOKEN,
            to: bob.id,
            amount: 1,
        },
    );
    let (winner, loser) = if first.tx_id() < second.tx_id() {
        (first.clone(), second.clone())
    } else {
        (second.clone(), first.clone())
    };

    let output = run_batch(&mut state, 1, vec![first, second], foundation);
    assert_eq!(output.txs.len(), 1);
    assert_eq!(output.txs[0].tx_id(), winner.tx_id());
    assert_eq!(
        reject_of(&output, &loser.tx_id()),
        Some(RejectReason::DuplicateNonce)
    );
}

/// Everything from the first nonce gap on is dropped (§5.2, step 7).
#[test]
fn nonce_gaps_drop_the_tail() {
    let alice = Actor::new(1);
    let foundation = Actor::new(200).id;
    let mut state = State::new();
    fund(&mut state, &alice.id, FEE_TX * 10);

    let one = alice.tx(1, 1, Action::ClaimRewards {});
    let three = alice.tx(3, 1, Action::ClaimRewards {});
    let four = alice.tx(4, 1, Action::ClaimRewards {});
    let (id1, id3, id4) = (one.tx_id(), three.tx_id(), four.tx_id());

    let output = run_batch(&mut state, 1, vec![one, three, four], foundation);
    assert_eq!(status_of(&output, &id1), Some(ExecStatus::Ok));
    assert_eq!(reject_of(&output, &id3), Some(RejectReason::NonceGap));
    assert_eq!(reject_of(&output, &id4), Some(RejectReason::NonceGap));
}

/// A stale nonce is a gap too: there is no separate reason for replaying an old one.
#[test]
fn stale_nonces_are_rejected_as_gaps() {
    let alice = Actor::new(1);
    let foundation = Actor::new(200).id;
    let mut state = State::new();
    fund(&mut state, &alice.id, FEE_TX * 10);

    run_batch(
        &mut state,
        1,
        vec![alice.tx(1, 1, Action::ClaimRewards {})],
        foundation,
    );
    assert_eq!(state.account(&alice.id).unwrap().nonce, 1);

    let replay = alice.tx(1, 2, Action::ClaimRewards {});
    let replay_id = replay.tx_id();
    let output = run_batch(&mut state, 2, vec![replay], foundation);
    assert_eq!(reject_of(&output, &replay_id), Some(RejectReason::NonceGap));
}

/// The per-account budget keeps the lowest nonces (§5.2, step 8).
#[test]
fn budget_keeps_the_lowest_nonces() {
    let alice = Actor::new(1);
    let foundation = Actor::new(200).id;
    let mut state = State::new();
    fund(&mut state, &alice.id, FEE_TX * 40);

    let count = MAX_TX_PER_ACCOUNT_PER_BATCH as u64 + 3;
    let txs: Vec<_> = (1..=count)
        .map(|nonce| alice.tx(nonce, 1, Action::ClaimRewards {}))
        .collect();
    let over_budget: Vec<_> = txs[MAX_TX_PER_ACCOUNT_PER_BATCH..]
        .iter()
        .map(|tx| tx.tx_id())
        .collect();

    let output = run_batch(&mut state, 1, txs, foundation);
    assert_eq!(output.txs.len(), MAX_TX_PER_ACCOUNT_PER_BATCH);
    for tx_id in over_budget {
        assert_eq!(reject_of(&output, &tx_id), Some(RejectReason::OverBudget));
    }
    assert_eq!(
        state.account(&alice.id).unwrap().nonce,
        MAX_TX_PER_ACCOUNT_PER_BATCH as u64
    );
}

/// Insolvency drops from the highest nonce downward, so the earliest transactions survive.
#[test]
fn fee_insolvency_drops_from_the_highest_nonce() {
    let alice = Actor::new(1);
    let foundation = Actor::new(200).id;
    let mut state = State::new();
    fund(&mut state, &alice.id, FEE_TX * 2);

    let txs: Vec<_> = (1..=4)
        .map(|nonce| alice.tx(nonce, 1, Action::ClaimRewards {}))
        .collect();
    let ids: Vec<_> = txs.iter().map(|tx| tx.tx_id()).collect();

    let output = run_batch(&mut state, 1, txs, foundation);
    assert_eq!(output.txs.len(), 2);
    assert_eq!(
        reject_of(&output, &ids[2]),
        Some(RejectReason::FeeInsolvent)
    );
    assert_eq!(
        reject_of(&output, &ids[3]),
        Some(RejectReason::FeeInsolvent)
    );
    assert_eq!(state.balance_of(&alice.id, &NATIVE_TOKEN), 0);
}

/// Solvency is measured against the PRE-batch balance: money arriving in the same batch does
/// not pay for that batch's fees (§5.2).
#[test]
fn an_account_funded_in_a_batch_transacts_only_from_the_next_one() {
    let alice = Actor::new(1);
    let newcomer = Actor::new(2);
    let foundation = Actor::new(200).id;
    let mut state = State::new();
    fund(&mut state, &alice.id, FEE_TX * 10);

    let funding = alice.tx(
        1,
        1,
        Action::Transfer {
            token: NATIVE_TOKEN,
            to: newcomer.id,
            amount: FEE_TX * 5,
        },
    );
    let spend = newcomer.tx(1, 1, Action::ClaimRewards {});
    let spend_id = spend.tx_id();

    let output = run_batch(&mut state, 1, vec![funding, spend], foundation);
    assert_eq!(
        reject_of(&output, &spend_id),
        Some(RejectReason::UnknownAccount)
    );

    // Next batch it works.
    let spend = newcomer.tx(1, 2, Action::ClaimRewards {});
    let spend_id = spend.tx_id();
    let output = run_batch(&mut state, 2, vec![spend], foundation);
    assert_eq!(status_of(&output, &spend_id), Some(ExecStatus::Ok));
}

/// A self-transfer is a fee-paying no-op, so it is refused outright (§5.2).
#[test]
fn self_transfers_fail() {
    let alice = Actor::new(1);
    let foundation = Actor::new(200).id;
    let mut state = State::new();
    fund(&mut state, &alice.id, FEE_TX * 10);

    let tx = alice.tx(
        1,
        1,
        Action::Transfer {
            token: NATIVE_TOKEN,
            to: alice.id,
            amount: 1,
        },
    );
    let tx_id = tx.tx_id();
    let output = run_batch(&mut state, 1, vec![tx], foundation);
    assert_eq!(
        status_of(&output, &tx_id),
        Some(ExecStatus::Failed(FailReason::SelfTransferNoop))
    );
}

/// A zero amount is a static range error, not a runtime failure (§5.2, step 5).
#[test]
fn zero_amounts_are_rejected_statically() {
    let alice = Actor::new(1);
    let bob = Actor::new(2);
    let foundation = Actor::new(200).id;
    let mut state = State::new();
    fund(&mut state, &alice.id, FEE_TX * 10);

    for action in [
        Action::Transfer {
            token: NATIVE_TOKEN,
            to: bob.id,
            amount: 0,
        },
        Action::Stake { amount: 0 },
        Action::Unstake { amount: 0 },
    ] {
        let tx = alice.tx(1, 1, action);
        let tx_id = tx.tx_id();
        let output = run_batch(&mut state, 1, vec![tx], foundation);
        assert_eq!(
            reject_of(&output, &tx_id),
            Some(RejectReason::FieldOutOfRange)
        );
    }
}

/// The publish surcharge applies only above the free threshold (§5.2).
#[test]
fn publish_fee_scales_past_the_free_threshold() {
    let alice = Actor::new(1);
    let foundation = Actor::new(200).id;
    let mut state = State::new();
    fund(&mut state, &alice.id, FEE_TX * 100);

    let small = alice.tx(
        1,
        1,
        Action::Publish {
            topic: [1u8; 32],
            data: vec![0u8; PUBLISH_FREE_BYTES],
        },
    );
    run_batch(&mut state, 1, vec![small], foundation);
    assert_eq!(state.global.native_burned, FEE_TX);

    let extra = 64usize;
    let large = alice.tx(
        2,
        2,
        Action::Publish {
            topic: [1u8; 32],
            data: vec![0u8; PUBLISH_FREE_BYTES + extra],
        },
    );
    run_batch(&mut state, 2, vec![large], foundation);
    assert_eq!(
        state.global.native_burned,
        FEE_TX * 2 + PUBLISH_BYTE_FEE * extra as u128
    );
}

/// With nothing staked, the staker share is never born (§7.2).
#[test]
fn emission_without_stakers_mints_only_the_foundation_share() {
    let foundation = Actor::new(200).id;
    let mut state = State::new();

    run_batch(&mut state, 1, vec![], foundation);

    let (staker_share, foundation_share) = emission::split(EMISSION_0);
    assert_eq!(state.global.native_emitted, foundation_share);
    assert_eq!(
        state.balance_of(&foundation, &NATIVE_TOKEN),
        foundation_share
    );
    assert_eq!(state.global.staking_reserved, 0);
    assert_eq!(state.global.acc_per_stake, 0);
    assert!(
        staker_share > 0,
        "the share exists nominally, it just is not minted"
    );
    assert!(state.monetary_invariant_holds(0));
}

/// With someone staked, the whole nominal emission is minted and split 85/15 (§7.2).
#[test]
fn emission_with_stakers_splits_eighty_five_fifteen() {
    let alice = Actor::new(1);
    let foundation = Actor::new(200).id;
    let mut state = State::new();
    fund(&mut state, &alice.id, FEE_TX * 10 + 1_000_000);

    let stake = alice.tx(1, 1, Action::Stake { amount: 500_000 });
    run_batch(&mut state, 1, vec![stake], foundation);

    let (staker_share, foundation_share) = emission::split(EMISSION_0);
    // The stake executed before this batch closed, so batch 1's own emission already
    // reached the reserve: emission is applied at close, over the post-execution stake.
    assert_eq!(state.global.staking_reserved, staker_share);

    let emitted_before = state.global.native_emitted;
    run_batch(&mut state, 2, vec![], foundation);
    assert_eq!(
        state.global.native_emitted - emitted_before,
        staker_share + foundation_share
    );
    assert_eq!(state.global.staking_reserved, staker_share * 2);
    assert!(state.monetary_invariant_holds(0));

    // The only staker can claim the whole reserve, up to the declared rounding residue.
    let pending = popcorn_core::execute::account_pending(&state, &alice.id);
    let reserved = state.global.staking_reserved;
    assert!(pending <= reserved);
    assert!(
        reserved - pending < 1_000,
        "residue is larger than rounding"
    );
}

/// Epoch boundaries are exact, and the 0-based index is what makes them so (§7.2).
#[test]
fn halving_boundaries_are_exact() {
    assert_eq!(emission::emission_at(1), EMISSION_0);
    assert_eq!(emission::emission_at(HALVING_INTERVAL), EMISSION_0);
    assert_eq!(emission::emission_at(HALVING_INTERVAL + 1), EMISSION_0 / 2);
    assert_eq!(emission::emission_at(2 * HALVING_INTERVAL), EMISSION_0 / 2);
    assert_eq!(
        emission::emission_at(2 * HALVING_INTERVAL + 1),
        EMISSION_0 / 4
    );
    // Genesis mints nothing.
    assert_eq!(emission::emission_at(0), 0);

    // The upper bound the spec quotes: below 21.03 M native.
    let bound = emission::supply_upper_bound(0);
    assert!(bound < 21_030_000_000_000_000, "bound was {bound}");
    assert!(bound > 21_000_000_000_000_000, "bound was {bound}");
}

/// Ordering is a function of the beacon: same inputs, same permutation, every time.
#[test]
fn ordering_is_reproducible_from_the_beacon() {
    let actors: Vec<_> = (1u8..=12).map(Actor::new).collect();
    let foundation = Actor::new(200).id;

    let build = || {
        let mut state = State::new();
        for actor in &actors {
            fund(&mut state, &actor.id, FEE_TX * 4);
        }
        let txs: Vec<_> = actors
            .iter()
            .map(|actor| actor.tx(1, 1, Action::ClaimRewards {}))
            .collect();
        (state, txs)
    };

    let (mut state_a, txs_a) = build();
    let (mut state_b, txs_b) = build();
    // The second run receives the transactions in a different submission order; the
    // resulting block must be identical, because the order is derived, not observed.
    let mut reversed = txs_b.clone();
    reversed.reverse();

    let out_a = run_batch(&mut state_a, 1, txs_a, foundation);
    let out_b = run_batch(&mut state_b, 1, reversed, foundation);

    assert_eq!(out_a.header, out_b.header);
    assert_eq!(state_a.state_root(), state_b.state_root());

    // A different beacon yields a different order (with 12 transactions, overwhelmingly).
    let (mut state_c, txs_c) = build();
    let input = popcorn_core::execute::BatchInput {
        height: 1,
        prev_hash: [0u8; 32],
        drand_round: 1,
        drand_signature: vec![7u8; 48],
        blob_manifest: txs_c.iter().map(|tx| tx.tx_id()).collect(),
        unusable: vec![],
        txs: txs_c,
        foundation,
    };
    let out_c = execute_batch(&mut state_c, input);
    assert_ne!(out_a.header.txs_root, out_c.header.txs_root);
}

/// Nonce normalization: an account's own transactions always execute in ascending nonce
/// order, whatever positions the shuffle handed them (§5.3).
#[test]
fn nonce_normalization_orders_each_account_ascending() {
    let actors: Vec<_> = (1u8..=4).map(Actor::new).collect();
    let foundation = Actor::new(200).id;
    let mut state = State::new();
    for actor in &actors {
        fund(&mut state, &actor.id, FEE_TX * 10);
    }

    let mut txs = Vec::new();
    for actor in &actors {
        for nonce in 1..=3 {
            txs.push(actor.tx(nonce, 1, Action::ClaimRewards {}));
        }
    }
    let output = run_batch(&mut state, 1, txs, foundation);

    for actor in &actors {
        let nonces: Vec<u64> = output
            .txs
            .iter()
            .filter(|tx| tx.signer() == actor.id)
            .map(|tx| tx.payload.nonce)
            .collect();
        assert_eq!(nonces, vec![1, 2, 3]);
    }
    // Every transaction executed, so no contiguous run was broken by the shuffle.
    assert!(output.results.iter().all(|r| *r == ExecStatus::Ok));
}

/// Staking the whole balance is refused: a fee must stay liquid, or the account could never
/// unstake (§8).
#[test]
fn stake_liquidity_guard_keeps_an_exit_fee() {
    let alice = Actor::new(1);
    let foundation = Actor::new(200).id;
    let mut state = State::new();
    let balance = FEE_TX * 10;
    fund(&mut state, &alice.id, balance);

    // After its own fee, the account holds balance − FEE_TX; staking all of it would leave
    // nothing for an Unstake.
    let all_in = alice.tx(
        1,
        1,
        Action::Stake {
            amount: balance - FEE_TX,
        },
    );
    let tx_id = all_in.tx_id();
    let output = run_batch(&mut state, 1, vec![all_in], foundation);
    assert_eq!(
        status_of(&output, &tx_id),
        Some(ExecStatus::Failed(FailReason::StakeLiquidityGuard))
    );

    // One fee less is fine.
    let sized = alice.tx(
        2,
        2,
        Action::Stake {
            amount: balance - 3 * FEE_TX,
        },
    );
    let tx_id = sized.tx_id();
    let output = run_batch(&mut state, 2, vec![sized], foundation);
    assert_eq!(status_of(&output, &tx_id), Some(ExecStatus::Ok));
    assert!(state.balance_of(&alice.id, &NATIVE_TOKEN) >= FEE_TX);
}

/// A full unstake returns stake and rewards as liquid balance, so nobody gets stuck (§8).
#[test]
fn full_unstake_returns_stake_and_rewards() {
    let alice = Actor::new(1);
    let foundation = Actor::new(200).id;
    let mut state = State::new();
    fund(&mut state, &alice.id, FEE_TX * 10 + 1_000_000);

    run_batch(
        &mut state,
        1,
        vec![alice.tx(1, 1, Action::Stake { amount: 900_000 })],
        foundation,
    );
    run_batch(&mut state, 2, vec![], foundation); // an emission accrues to the staker
    run_batch(
        &mut state,
        3,
        vec![alice.tx(2, 3, Action::Unstake { amount: 900_000 })],
        foundation,
    );

    assert_eq!(state.account(&alice.id).unwrap().staked, 0);
    assert_eq!(state.global.total_staked, 0);
    assert!(state.balance_of(&alice.id, &NATIVE_TOKEN) > 900_000);
    assert!(state.monetary_invariant_holds(0));
}

/// The four-bucket invariant holds at every block, which is what replay checks (§5.5).
#[test]
fn monetary_invariant_holds_across_a_run_of_batches() {
    let actors: Vec<_> = (1u8..=5).map(Actor::new).collect();
    let foundation = Actor::new(200).id;
    let mut state = State::new();
    for actor in &actors {
        fund(&mut state, &actor.id, 10_000_000);
    }

    let mut nonces = [0u64; 5];
    for height in 1..=25u64 {
        let mut txs = Vec::new();
        for (index, actor) in actors.iter().enumerate() {
            nonces[index] += 1;
            let action = match height % 4 {
                0 => Action::Stake { amount: 10_000 },
                1 => Action::ClaimRewards {},
                2 => Action::Transfer {
                    token: NATIVE_TOKEN,
                    to: actors[(index + 1) % actors.len()].id,
                    amount: 1_000,
                },
                _ => Action::Unstake { amount: 5_000 },
            };
            txs.push(actor.tx(nonces[index], height, action));
        }
        run_batch(&mut state, height, txs, foundation);
        assert!(
            state.monetary_invariant_holds(0),
            "invariant broken at height {height}"
        );
        assert!(state.global.staking_reserved >= state.total_pending());
    }
}
