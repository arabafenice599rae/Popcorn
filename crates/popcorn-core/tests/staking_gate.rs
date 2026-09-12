//! The mandatory pre-genesis staking gate (SPEC.md §8).
//!
//! This is the test whose absence let an earlier design ship a false inequality. Four-bucket
//! equality alone did not catch it: the reserve could be drained below what pending claims
//! would draw, and the first claim afterwards underflowed. Assertion (3) below —
//! `staking_reserved ≥ Σ pending ≥ 0` — is the one that matters, and it is checked after
//! *every single operation*, not at the end of a run.

mod common;

use common::*;
use popcorn_core::constants::{FEE_TX, NATIVE_TOKEN, PRECISION};
use popcorn_core::emission;
use popcorn_core::staking::{accumulator_increment, settle_amount};
use popcorn_core::state::{Journal, State};
use popcorn_core::types::{AccountId, Amount};

/// A deterministic PRNG: the gate must fail reproducibly, with a seed you can re-run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*: small, fast, and good enough to explore operation orderings.
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// The model under test: the four buckets plus the accumulator, driven directly.
///
/// It deliberately bypasses transactions. Fees, nonces and signatures cannot mask or create
/// the rounding behaviour this gate is about.
struct Model {
    state: State,
    accounts: Vec<AccountId>,
    emitted_outside: Amount,
}

impl Model {
    fn new(actors: &[Actor], funding: Amount) -> Self {
        let mut state = State::new();
        for actor in actors {
            fund(&mut state, &actor.id, funding);
        }
        Self {
            accounts: actors.iter().map(|a| a.id).collect(),
            emitted_outside: funding * actors.len() as u128,
            state,
        }
    }

    /// One batch of emission, exactly as `apply_emission` does it.
    fn emit(&mut self, staker_share: Amount, foundation: &AccountId) {
        let mut journal = Journal::new();
        let total_staked = self.state.global.total_staked;
        if total_staked == 0 {
            // The staker share is not born at all in this case (§7.2).
            return;
        }
        let increment = accumulator_increment(staker_share, total_staked).unwrap();
        let global = self.state.global_mut(&mut journal);
        global.acc_per_stake += increment;
        global.staking_reserved += staker_share;
        global.native_emitted += staker_share;
        let _ = foundation;
    }

    fn settle(&mut self, id: &AccountId) -> Amount {
        let acc = self.state.global.acc_per_stake;
        let reserved = self.state.global.staking_reserved;
        let account = self.state.account(id).cloned().unwrap_or_default();
        let payout = settle_amount(&account, acc, reserved).expect("reserve must be solvent");

        let mut journal = Journal::new();
        if payout > 0 {
            self.state.global_mut(&mut journal).staking_reserved -= payout;
            self.state
                .credit(id, &NATIVE_TOKEN, payout, &mut journal)
                .unwrap();
        }
        if let Some(account) = self.state.account_mut(id, &mut journal) {
            account.paid_acc = acc;
        }
        payout
    }

    fn stake(&mut self, id: &AccountId, amount: Amount) {
        self.settle(id);
        let balance = self.state.balance_of(id, &NATIVE_TOKEN);
        if amount == 0 || balance < amount + FEE_TX {
            return; // the liquidity guard would reject this
        }
        let mut journal = Journal::new();
        self.state
            .debit(id, &NATIVE_TOKEN, amount, &mut journal)
            .unwrap();
        let acc = self.state.global.acc_per_stake;
        let account = self.state.account_mut(id, &mut journal).unwrap();
        account.staked += amount;
        account.paid_acc = acc;
        self.state.global_mut(&mut journal).total_staked += amount;
    }

    fn unstake(&mut self, id: &AccountId, amount: Amount) {
        self.settle(id);
        let staked = self.state.account(id).map(|a| a.staked).unwrap_or(0);
        let amount = amount.min(staked);
        if amount == 0 {
            return;
        }
        let mut journal = Journal::new();
        let acc = self.state.global.acc_per_stake;
        let account = self.state.account_mut(id, &mut journal).unwrap();
        account.staked -= amount;
        account.paid_acc = acc;
        self.state.global_mut(&mut journal).total_staked -= amount;
        self.state
            .credit(id, &NATIVE_TOKEN, amount, &mut journal)
            .unwrap();
    }

    /// The three assertions of the gate, checked after every operation.
    fn check(&self, label: &str) {
        // (1) four-bucket monetary invariant, as an exact equality
        assert!(
            self.state.monetary_invariant_holds(0),
            "{label}: four-bucket invariant broken"
        );

        // (3) solvency: the reserve covers every claim outstanding against it
        let total_pending = self.state.total_pending();
        assert!(
            self.state.global.staking_reserved >= total_pending,
            "{label}: reserve {} < Σ pending {}",
            self.state.global.staking_reserved,
            total_pending
        );
    }

    /// (2) conservation: a settle moves units, it never creates or destroys them.
    fn settle_conserves(&mut self, id: &AccountId) {
        let before = self.state.balance_of(id, &NATIVE_TOKEN) + self.state.global.staking_reserved;
        let payout = self.settle(id);
        let after = self.state.balance_of(id, &NATIVE_TOKEN) + self.state.global.staking_reserved;
        assert_eq!(before, after, "settle of {payout} was not a pure transfer");
    }
}

fn iterations() -> usize {
    // §8 asks for millions of sequences. That is what CI runs (POPCORN_GATE_ITERS=2000000,
    // about four seconds in release); the default here is a fast slice so the gate stays
    // usable in a local edit-test loop.
    std::env::var("POPCORN_GATE_ITERS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20_000)
}

#[test]
fn staking_reserve_stays_solvent_under_random_sequences() {
    let actors: Vec<_> = (1u8..=6).map(Actor::new).collect();
    let foundation = Actor::new(200).id;

    // Several seeds, and deliberately hostile distributions: dust stakers next to whales are
    // exactly where double flooring used to bite.
    for seed in [1u64, 7, 99, 123_456_789] {
        let mut rng = Rng(seed);
        let mut model = Model::new(&actors, 10_000_000_000);
        model.check("start");

        for step in 0..iterations() {
            let account = model.accounts[rng.below(model.accounts.len() as u64) as usize];
            match rng.below(10) {
                0..=3 => {
                    // emission of a batch, with a share that is often not divisible
                    let share = match rng.below(4) {
                        0 => 1,
                        1 => rng.below(1_000) as u128,
                        2 => emission::split(emission::emission_at(1)).0,
                        _ => rng.below(u32::MAX as u64) as u128,
                    };
                    model.emit(share, &foundation);
                }
                4..=5 => {
                    // stakes of wildly different magnitudes, dust included
                    let amount = match rng.below(3) {
                        0 => 1,
                        1 => rng.below(10_000) as u128,
                        _ => rng.below(1_000_000_000) as u128,
                    };
                    model.stake(&account, amount);
                }
                6..=7 => {
                    let amount = rng.below(1_000_000_000) as u128;
                    model.unstake(&account, amount);
                }
                _ => model.settle_conserves(&account),
            }
            model.check(&format!("seed {seed}, step {step}"));
        }

        // Everyone exits: the reserve must still be solvent, and only the declared rounding
        // residue may remain.
        for account in model.accounts.clone() {
            model.unstake(&account, u128::MAX);
            model.settle_conserves(&account);
            model.check("drain");
        }
        assert_eq!(
            model.state.total_pending(),
            0,
            "claims left after full exit"
        );
        assert!(
            model.state.global.staking_reserved < PRECISION,
            "residue {} is larger than one accumulator step; that is not rounding",
            model.state.global.staking_reserved
        );
        let _ = model.emitted_outside;
    }
}

/// The counterexample that killed the old design, made explicit: a single dust staker whose
/// entitlement is entirely below the accumulator's resolution must never make the first
/// claim underflow the reserve.
#[test]
fn dust_staker_cannot_underflow_the_reserve() {
    let whale = Actor::new(1);
    let dust = Actor::new(2);
    let mut model = Model::new(&[whale, dust], 1_000_000_000);
    let whale_id = model.accounts[0];
    let dust_id = model.accounts[1];

    model.stake(&whale_id, 999_999_000);
    model.stake(&dust_id, 1);
    model.check("staked");

    for _ in 0..1_000 {
        model.emit(1, &whale_id);
        model.check("after emission");
        model.settle_conserves(&dust_id);
        model.check("after dust claim");
    }

    model.settle_conserves(&whale_id);
    model.check("after whale claim");
    assert!(model.state.global.staking_reserved >= model.state.total_pending());
}

/// A gate nobody has seen fail is a gate nobody trusts.
///
/// This models the *superseded* rule — the reserve took only the share that flooring could
/// attribute, with the dust routed to the foundation — and shows the solvency assertion
/// fires on it. Three stakers of one unit each, a share of two per batch: flooring
/// attributes one unit per batch to the reserve while each staker's claim grows on the
/// unfloored accumulator, so by the second batch the claims outrun the reserve.
#[test]
fn superseded_dust_design_fails_the_solvency_assertion() {
    let total_staked: u128 = 3;
    let staker_stake: u128 = 1;
    let share_per_batch: u128 = 2;

    let mut acc: u128 = 0;
    let mut reserved_legacy: u128 = 0;
    let mut foundation_dust: u128 = 0;

    for _ in 0..2 {
        let increment = accumulator_increment(share_per_batch, total_staked).unwrap();
        acc += increment;
        // The superseded rule: credit only what the accumulator can attribute, hand the
        // remainder to the foundation.
        let attributable = increment * total_staked / PRECISION;
        reserved_legacy += attributable;
        foundation_dust += share_per_batch - attributable;
    }

    let pending_each = staker_stake * acc / PRECISION;
    let total_claims = pending_each * 3;

    assert_eq!(reserved_legacy, 2);
    assert_eq!(foundation_dust, 2);
    assert_eq!(total_claims, 3);
    assert!(
        total_claims > reserved_legacy,
        "the superseded design is supposed to be insolvent here"
    );

    // The design in force takes the share whole, and the same sequence stays solvent.
    let mut reserved_current: u128 = 0;
    for _ in 0..2 {
        reserved_current += share_per_batch;
    }
    assert!(reserved_current >= total_claims);
}
