//! HTLCs and Publish auto-settlement (SPEC.md §7.5, §7.6).

mod common;

use common::*;
use popcorn_core::constants::{FEE_TX, HTLC_MAX_LIFETIME_ROUNDS, NATIVE_TOKEN};
use popcorn_core::crypto::sha256;
use popcorn_core::ids::htlc_id;
use popcorn_core::state::State;
use popcorn_core::types::{Action, ExecStatus, FailReason, RejectReason};

fn preimage(seed: u8) -> [u8; 32] {
    [seed; 32]
}

/// Locked funds belong to no account: not the sender, not the recipient, not the node.
#[test]
fn locked_funds_leave_every_balance() {
    let alice = Actor::new(1);
    let bob = Actor::new(2);
    let foundation = Actor::new(200).id;
    let mut state = State::new();
    fund(&mut state, &alice.id, FEE_TX * 10 + 1_000_000);

    let secret = preimage(7);
    let lock = alice.tx(
        1,
        1,
        Action::HtlcLock {
            to: bob.id,
            token: NATIVE_TOKEN,
            amount: 500_000,
            hashlock: sha256(&secret),
            expiry_round: 100,
        },
    );
    let id = lock.tx_id();
    let output = run_batch(&mut state, 1, vec![lock], foundation);

    assert_eq!(status_of(&output, &id), Some(ExecStatus::Ok));
    let htlc = htlc_id(&alice.id, 1);
    assert_eq!(state.htlcs[&htlc].amount, 500_000);
    assert_eq!(state.balance_of(&bob.id, &NATIVE_TOKEN), 0);
    assert_eq!(
        state.balance_of(&alice.id, &NATIVE_TOKEN),
        1_000_000 + FEE_TX * 10 - 500_000 - FEE_TX
    );
    // Escrow is its own bucket in the invariant, so nothing has gone missing.
    assert!(state.monetary_invariant_holds(0));
}

/// Anyone can deliver the preimage: what counts is the secret, not the sender (§7.6).
#[test]
fn a_third_party_can_claim_with_the_preimage() {
    let alice = Actor::new(1);
    let bob = Actor::new(2);
    let carol = Actor::new(3);
    let foundation = Actor::new(200).id;
    let mut state = State::new();
    fund(&mut state, &alice.id, FEE_TX * 10 + 1_000_000);
    fund(&mut state, &carol.id, FEE_TX * 10);

    let secret = preimage(7);
    run_batch(
        &mut state,
        1,
        vec![alice.tx(
            1,
            1,
            Action::HtlcLock {
                to: bob.id,
                token: NATIVE_TOKEN,
                amount: 500_000,
                hashlock: sha256(&secret),
                expiry_round: 100,
            },
        )],
        foundation,
    );

    // Carol claims; Bob receives. Bob need not be online at all.
    let claim = carol.tx(
        1,
        2,
        Action::HtlcClaim {
            htlc_id: htlc_id(&alice.id, 1),
            preimage: secret,
        },
    );
    let id = claim.tx_id();
    let output = run_batch(&mut state, 2, vec![claim], foundation);

    assert_eq!(status_of(&output, &id), Some(ExecStatus::Ok));
    assert_eq!(state.balance_of(&bob.id, &NATIVE_TOKEN), 500_000);
    assert!(state.htlcs.is_empty());
}

/// The claim/refund boundary is sharp: `<=` against `>`, with no overlap (§7.6).
#[test]
fn claim_and_refund_windows_do_not_overlap() {
    let alice = Actor::new(1);
    let bob = Actor::new(2);
    let foundation = Actor::new(200).id;
    let secret = preimage(7);
    let expiry = 5u64;

    // At exactly the expiry round a claim still works and a refund does not.
    let mut state = State::new();
    fund(&mut state, &alice.id, FEE_TX * 10 + 1_000_000);
    run_batch(
        &mut state,
        1,
        vec![alice.tx(
            1,
            1,
            Action::HtlcLock {
                to: bob.id,
                token: NATIVE_TOKEN,
                amount: 500_000,
                hashlock: sha256(&secret),
                expiry_round: expiry,
            },
        )],
        foundation,
    );

    let refund = alice.tx(
        2,
        expiry,
        Action::HtlcRefund {
            htlc_id: htlc_id(&alice.id, 1),
        },
    );
    let id = refund.tx_id();
    let output = run_batch(&mut state, expiry, vec![refund], foundation);
    assert_eq!(
        status_of(&output, &id),
        Some(ExecStatus::Failed(FailReason::HtlcNotExpired))
    );

    // One round later the refund works and the claim is too late.
    let claim = bob_claim(&alice, expiry + 1, secret, 3);
    let id = claim.tx_id();
    fund(&mut state, &Actor::new(3).id, FEE_TX * 10);
    let output = run_batch(&mut state, expiry + 1, vec![claim], foundation);
    assert_eq!(
        status_of(&output, &id),
        Some(ExecStatus::Failed(FailReason::HtlcExpired))
    );

    let refund = alice.tx(
        3,
        expiry + 2,
        Action::HtlcRefund {
            htlc_id: htlc_id(&alice.id, 1),
        },
    );
    let id = refund.tx_id();
    let output = run_batch(&mut state, expiry + 2, vec![refund], foundation);
    assert_eq!(status_of(&output, &id), Some(ExecStatus::Ok));
    assert!(state.htlcs.is_empty());
    assert!(state.monetary_invariant_holds(0));
}

fn bob_claim(sender: &Actor, round: u64, secret: [u8; 32], seed: u8) -> popcorn_core::SignedTx {
    let carol = Actor::new(seed);
    carol.tx(
        1,
        round,
        Action::HtlcClaim {
            htlc_id: htlc_id(&sender.id, 1),
            preimage: secret,
        },
    )
}

/// A wrong preimage fails and leaves the lock standing.
#[test]
fn a_wrong_preimage_does_not_open_the_lock() {
    let alice = Actor::new(1);
    let bob = Actor::new(2);
    let foundation = Actor::new(200).id;
    let mut state = State::new();
    fund(&mut state, &alice.id, FEE_TX * 10 + 1_000_000);
    fund(&mut state, &bob.id, FEE_TX * 10);

    run_batch(
        &mut state,
        1,
        vec![alice.tx(
            1,
            1,
            Action::HtlcLock {
                to: bob.id,
                token: NATIVE_TOKEN,
                amount: 500_000,
                hashlock: sha256(&preimage(7)),
                expiry_round: 100,
            },
        )],
        foundation,
    );

    let claim = bob.tx(
        1,
        2,
        Action::HtlcClaim {
            htlc_id: htlc_id(&alice.id, 1),
            preimage: preimage(8),
        },
    );
    let id = claim.tx_id();
    let output = run_batch(&mut state, 2, vec![claim], foundation);
    assert_eq!(
        status_of(&output, &id),
        Some(ExecStatus::Failed(FailReason::HtlcBadPreimage))
    );
    assert_eq!(state.htlcs.len(), 1);
}

/// A published preimage settles the lock by itself: carrier-independent settlement (§7.6).
#[test]
fn publishing_the_preimage_settles_the_lock() {
    let alice = Actor::new(1);
    let bob = Actor::new(2);
    let carol = Actor::new(3);
    let foundation = Actor::new(200).id;
    let mut state = State::new();
    fund(&mut state, &alice.id, FEE_TX * 10 + 1_000_000);
    fund(&mut state, &carol.id, FEE_TX * 10);

    let secret = preimage(7);
    run_batch(
        &mut state,
        1,
        vec![alice.tx(
            1,
            1,
            Action::HtlcLock {
                to: bob.id,
                token: NATIVE_TOKEN,
                amount: 500_000,
                hashlock: sha256(&secret),
                expiry_round: 100,
            },
        )],
        foundation,
    );

    // Carol simply publishes the 32 bytes. She is not the recipient and sends no claim.
    let publish = carol.tx(
        1,
        2,
        Action::Publish {
            topic: [0u8; 32],
            data: secret.to_vec(),
        },
    );
    run_batch(&mut state, 2, vec![publish], foundation);

    assert_eq!(state.balance_of(&bob.id, &NATIVE_TOKEN), 500_000);
    assert!(state.htlcs.is_empty());
}

/// A publish of the wrong length is just data: only exactly 32 bytes can settle (§7.6).
#[test]
fn only_thirty_two_byte_publishes_can_settle() {
    let alice = Actor::new(1);
    let bob = Actor::new(2);
    let carol = Actor::new(3);
    let foundation = Actor::new(200).id;
    let mut state = State::new();
    fund(&mut state, &alice.id, FEE_TX * 10 + 1_000_000);
    fund(&mut state, &carol.id, FEE_TX * 20);

    let secret = preimage(7);
    run_batch(
        &mut state,
        1,
        vec![alice.tx(
            1,
            1,
            Action::HtlcLock {
                to: bob.id,
                token: NATIVE_TOKEN,
                amount: 500_000,
                hashlock: sha256(&secret),
                expiry_round: 100,
            },
        )],
        foundation,
    );

    let mut padded = secret.to_vec();
    padded.push(0);
    run_batch(
        &mut state,
        2,
        vec![carol.tx(
            1,
            2,
            Action::Publish {
                topic: [0u8; 32],
                data: padded,
            },
        )],
        foundation,
    );
    assert_eq!(
        state.htlcs.len(),
        1,
        "a 33-byte publish must not settle anything"
    );
}

/// If a claim and a matching publish land in the same batch, the claim wins and the publish
/// is a deterministic no-op (§7.6) — auto-settlement runs after execution, not during it.
#[test]
fn a_claim_in_the_same_batch_wins_over_the_publish() {
    let alice = Actor::new(1);
    let bob = Actor::new(2);
    let carol = Actor::new(3);
    let foundation = Actor::new(200).id;
    let mut state = State::new();
    fund(&mut state, &alice.id, FEE_TX * 10 + 1_000_000);
    fund(&mut state, &bob.id, FEE_TX * 10);
    fund(&mut state, &carol.id, FEE_TX * 10);

    let secret = preimage(7);
    run_batch(
        &mut state,
        1,
        vec![alice.tx(
            1,
            1,
            Action::HtlcLock {
                to: bob.id,
                token: NATIVE_TOKEN,
                amount: 500_000,
                hashlock: sha256(&secret),
                expiry_round: 100,
            },
        )],
        foundation,
    );

    let claim = bob.tx(
        1,
        2,
        Action::HtlcClaim {
            htlc_id: htlc_id(&alice.id, 1),
            preimage: secret,
        },
    );
    let publish = carol.tx(
        1,
        2,
        Action::Publish {
            topic: [0u8; 32],
            data: secret.to_vec(),
        },
    );
    let claim_id = claim.tx_id();
    let publish_id = publish.tx_id();

    let output = run_batch(&mut state, 2, vec![claim, publish], foundation);

    // Both executed and both paid; the claim settled, the publish found nothing to do.
    assert_eq!(status_of(&output, &claim_id), Some(ExecStatus::Ok));
    assert_eq!(status_of(&output, &publish_id), Some(ExecStatus::Ok));
    assert_eq!(
        state.balance_of(&bob.id, &NATIVE_TOKEN),
        500_000 + FEE_TX * 9
    );
    assert!(state.htlcs.is_empty());
    assert!(state.monetary_invariant_holds(0));
}

/// One hashlock, one HTLC: the index has to stay injective for settlement to be
/// unambiguous. The griefing this enables is a declared threat model, not an oversight.
#[test]
fn a_hashlock_cannot_be_locked_twice() {
    let alice = Actor::new(1);
    let bob = Actor::new(2);
    let foundation = Actor::new(200).id;
    let mut state = State::new();
    fund(&mut state, &alice.id, FEE_TX * 10 + 1_000_000);

    let hashlock = sha256(&preimage(7));
    let first = alice.tx(
        1,
        1,
        Action::HtlcLock {
            to: bob.id,
            token: NATIVE_TOKEN,
            amount: 100_000,
            hashlock,
            expiry_round: 100,
        },
    );
    let second = alice.tx(
        2,
        1,
        Action::HtlcLock {
            to: bob.id,
            token: NATIVE_TOKEN,
            amount: 100_000,
            hashlock,
            expiry_round: 100,
        },
    );
    let second_id = second.tx_id();

    let output = run_batch(&mut state, 1, vec![first, second], foundation);
    assert_eq!(
        status_of(&output, &second_id),
        Some(ExecStatus::Failed(FailReason::HtlcDuplicateHashlock))
    );
    assert_eq!(state.htlcs.len(), 1);
}

/// Expiry windows are bounded, so state cannot accumulate eternal locks (§7.6).
#[test]
fn expiry_windows_are_bounded_and_forward_looking() {
    let alice = Actor::new(1);
    let bob = Actor::new(2);
    let foundation = Actor::new(200).id;
    let mut state = State::new();
    fund(&mut state, &alice.id, FEE_TX * 10 + 1_000_000);

    for expiry in [1u64, 0, 1 + HTLC_MAX_LIFETIME_ROUNDS + 1] {
        let lock = alice.tx(
            1,
            1,
            Action::HtlcLock {
                to: bob.id,
                token: NATIVE_TOKEN,
                amount: 1,
                hashlock: sha256(&preimage(expiry as u8)),
                expiry_round: expiry,
            },
        );
        let id = lock.tx_id();
        let output = run_batch(&mut state, 1, vec![lock], foundation);
        assert_eq!(
            reject_of(&output, &id),
            Some(RejectReason::FieldOutOfRange),
            "expiry {expiry} should not be accepted at round 1"
        );
    }

    // The upper bound itself is admissible.
    let lock = alice.tx(
        1,
        1,
        Action::HtlcLock {
            to: bob.id,
            token: NATIVE_TOKEN,
            amount: 1,
            hashlock: sha256(&preimage(9)),
            expiry_round: 1 + HTLC_MAX_LIFETIME_ROUNDS,
        },
    );
    let id = lock.tx_id();
    let output = run_batch(&mut state, 1, vec![lock], foundation);
    assert_eq!(status_of(&output, &id), Some(ExecStatus::Ok));
}

/// Refund is garbage collection: anyone may trigger it, and the funds go home to the sender.
#[test]
fn anyone_can_refund_an_expired_lock() {
    let alice = Actor::new(1);
    let bob = Actor::new(2);
    let stranger = Actor::new(9);
    let foundation = Actor::new(200).id;
    let mut state = State::new();
    fund(&mut state, &alice.id, FEE_TX * 10 + 1_000_000);
    fund(&mut state, &stranger.id, FEE_TX * 10);

    run_batch(
        &mut state,
        1,
        vec![alice.tx(
            1,
            1,
            Action::HtlcLock {
                to: bob.id,
                token: NATIVE_TOKEN,
                amount: 500_000,
                hashlock: sha256(&preimage(7)),
                expiry_round: 3,
            },
        )],
        foundation,
    );

    let refund = stranger.tx(
        1,
        4,
        Action::HtlcRefund {
            htlc_id: htlc_id(&alice.id, 1),
        },
    );
    let id = refund.tx_id();
    let output = run_batch(&mut state, 4, vec![refund], foundation);

    assert_eq!(status_of(&output, &id), Some(ExecStatus::Ok));
    // The refund goes to the sender, never to whoever triggered it.
    assert_eq!(state.balance_of(&bob.id, &NATIVE_TOKEN), 0);
    assert!(state.balance_of(&alice.id, &NATIVE_TOKEN) > 500_000);
}
