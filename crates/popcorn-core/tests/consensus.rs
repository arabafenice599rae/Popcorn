//! Gates on the commitment itself (SPEC.md §2.3, §4.3, §5.4, §13.1).
//!
//! These are the checks that a second implementation in another language has to reproduce
//! byte for byte; if one of them drifts, chains fork.

mod common;

use std::collections::BTreeMap;

use common::*;
use popcorn_core::constants::NATIVE_TOKEN;
use popcorn_core::crypto::{blake3_hash, sha256, signing_hash, verify_signature};
use popcorn_core::execute::{collection_root, rejected_root, results_root, txs_root};
use popcorn_core::ids::{lp_token_id, pair_id, token_id};
use popcorn_core::state::{Journal, State};
use popcorn_core::types::{Account, ExecStatus, FailReason, Global, RejectReason};

/// The canonicity gate of §2.3: same logical state, different insertion orders, identical
/// bytes. This is the test that makes "no unordered collections" enforceable rather than
/// aspirational.
#[test]
fn state_root_is_independent_of_insertion_order() {
    let actors: Vec<_> = (1u8..=8).map(Actor::new).collect();

    let mut forward = State::new();
    for actor in &actors {
        fund(&mut forward, &actor.id, 1_000_000);
    }

    let mut backward = State::new();
    for actor in actors.iter().rev() {
        fund(&mut backward, &actor.id, 1_000_000);
    }

    // Same accounts, opposite insertion order, plus balances written in a different order
    // inside each account.
    let token_a = [7u8; 32];
    let token_b = [3u8; 32];
    for actor in &actors {
        fund_token(&mut forward, &actor.id, &token_a, 5);
        fund_token(&mut forward, &actor.id, &token_b, 9);
        fund_token(&mut backward, &actor.id, &token_b, 9);
        fund_token(&mut backward, &actor.id, &token_a, 5);
    }

    assert_eq!(forward.state_root(), backward.state_root());
    assert_eq!(forward, backward);
}

/// Roots over empty lists are pinned in §4.3 and must not be special-cased.
#[test]
fn empty_roots_are_the_hash_of_nothing() {
    let empty = blake3_hash(&[]);
    assert_eq!(collection_root(&[]), empty);
    assert_eq!(txs_root(&[]), empty);
    assert_eq!(rejected_root(&[]), empty);

    // results_root is the odd one out: it hashes the Borsh encoding of the vector, which
    // for an empty vector is its four-byte length prefix, not an empty input.
    let encoded = borsh::to_vec(&Vec::<ExecStatus>::new()).unwrap();
    assert_eq!(results_root(&[]), blake3_hash(&encoded));
    assert_ne!(results_root(&[]), empty);
}

/// A slice must serialize exactly like the `Vec` the spec names, or `results_root` would
/// depend on how the implementation happens to hold the list.
#[test]
fn slice_and_vec_encode_identically() {
    let results = vec![
        ExecStatus::Ok,
        ExecStatus::Failed(FailReason::SlippageExceeded),
    ];
    assert_eq!(
        borsh::to_vec(&results).unwrap(),
        borsh::to_vec(results.as_slice()).unwrap()
    );
}

/// §13.1 tabulates these numbers; `results_root` and `rejected_root` commit to them.
#[test]
fn enum_discriminants_match_the_normative_table() {
    let cases: [(RejectReason, u8); 11] = [
        (RejectReason::Malformed, 0),
        (RejectReason::BadSignature, 1),
        (RejectReason::WrongRound, 2),
        (RejectReason::UnknownAccount, 3),
        (RejectReason::PubkeyMismatch, 4),
        (RejectReason::FieldOutOfRange, 5),
        (RejectReason::DuplicateNonce, 6),
        (RejectReason::NonceGap, 7),
        (RejectReason::OverBudget, 8),
        (RejectReason::FeeInsolvent, 9),
        (RejectReason::NonceExhausted, 10),
    ];
    for (reason, expected) in cases {
        assert_eq!(
            borsh::to_vec(&reason).unwrap(),
            vec![expected],
            "{reason:?}"
        );
    }

    let fails: [(FailReason, u8); 19] = [
        (FailReason::InsufficientBalance, 0),
        (FailReason::SlippageExceeded, 1),
        (FailReason::UnknownToken, 2),
        (FailReason::UnknownPair, 3),
        (FailReason::PairAlreadyExists, 4),
        (FailReason::LpTokenAsPairSide, 5),
        (FailReason::ZeroOutput, 6),
        (FailReason::LiquidityTooSmall, 7),
        (FailReason::ReGenesisGuard, 8),
        (FailReason::BadPath, 9),
        (FailReason::StakeLiquidityGuard, 10),
        (FailReason::Overflow, 11),
        (FailReason::SupplyOutOfRange, 12),
        (FailReason::SelfTransferNoop, 13),
        (FailReason::HtlcNotFound, 14),
        (FailReason::HtlcBadPreimage, 15),
        (FailReason::HtlcExpired, 16),
        (FailReason::HtlcNotExpired, 17),
        (FailReason::HtlcDuplicateHashlock, 18),
    ];
    for (reason, expected) in fails {
        assert_eq!(
            borsh::to_vec(&reason).unwrap(),
            vec![expected],
            "{reason:?}"
        );
    }

    // ExecStatus wraps the reason, so a Failed status is two bytes: variant then reason.
    assert_eq!(borsh::to_vec(&ExecStatus::Ok).unwrap(), vec![0]);
    assert_eq!(
        borsh::to_vec(&ExecStatus::Failed(FailReason::ZeroOutput)).unwrap(),
        vec![1, 6]
    );
}

/// The `global` singleton is hashed as one entry with an empty key (§5.4). A non-Rust
/// implementer reproduces the state root from this alone.
#[test]
fn global_singleton_encoding_is_explicit() {
    let state = State::new();

    let mut expected = blake3::Hasher::new();
    for tag in [0x01u8, 0x02, 0x03, 0x04] {
        expected.update(&[tag]);
    }
    expected.update(&[0x05u8]);
    let encoded = borsh::to_vec(&Global::default()).unwrap();
    expected.update(&0u32.to_le_bytes());
    expected.update(&(encoded.len() as u32).to_le_bytes());
    expected.update(&encoded);

    assert_eq!(state.state_root(), *expected.finalize().as_bytes());
}

/// Every identifier is domain-separated (§4.1): the same preimage material under different
/// tags must never collide.
#[test]
fn identifier_domains_do_not_collide() {
    let account = [9u8; 32];
    let token = token_id(&account, 1);
    let pair = pair_id(&NATIVE_TOKEN, &token, 30);
    let lp = lp_token_id(&pair);

    let mut seen = BTreeMap::new();
    for (name, id) in [
        ("token", token),
        ("pair", pair),
        ("lp", lp),
        ("htlc", popcorn_core::ids::htlc_id(&account, 1)),
    ] {
        assert!(seen.insert(id, name).is_none(), "{name} collided");
    }

    // The pair id is order-independent: the same couple always yields the same pair.
    assert_eq!(pair, pair_id(&token, &NATIVE_TOKEN, 30));
    // ...but the fee tier is part of the identity.
    assert_ne!(pair, pair_id(&NATIVE_TOKEN, &token, 5));
}

/// The signing domain is what stops a Solana-wallet signature from being replayed here.
#[test]
fn signatures_are_domain_separated_and_strict() {
    let actor = Actor::new(1);
    let tx = actor.tx(1, 10, popcorn_core::types::Action::Stake { amount: 1_000 });

    assert!(verify_signature(
        &actor.pubkey(),
        &signing_hash(&tx.payload),
        &tx.signature
    ));

    // The same payload hashed without the domain does not verify.
    let undomained = blake3_hash(&borsh::to_vec(&tx.payload).unwrap());
    assert!(!verify_signature(
        &actor.pubkey(),
        &undomained,
        &tx.signature
    ));

    // A different signer's key does not verify either.
    let other = Actor::new(2);
    assert!(!verify_signature(
        &other.pubkey(),
        &signing_hash(&tx.payload),
        &tx.signature
    ));
}

/// Hashlocks are SHA-256 — the single non-blake3 point in the protocol (§7.6). Getting this
/// wrong would silently break every cross-chain swap.
#[test]
fn hashlocks_use_sha256_not_blake3() {
    let preimage = [42u8; 32];
    assert_ne!(sha256(&preimage), blake3_hash(&preimage));
    // RFC 6234 test vector for the empty string.
    assert_eq!(
        hex(&sha256(b"")),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

/// A zero balance is removed rather than stored (§14.1): otherwise two logically identical
/// states would hash differently.
#[test]
fn zero_balances_are_removed_from_state() {
    let actor = Actor::new(1);
    let mut state = State::new();
    fund(&mut state, &actor.id, 100);

    let mut journal = Journal::new();
    state
        .debit(&actor.id, &NATIVE_TOKEN, 100, &mut journal)
        .unwrap();

    let account = state.account(&actor.id).unwrap();
    assert!(account.balances.is_empty(), "a zero entry was left behind");

    // A drained account is byte-identical to one that never held anything, so the two
    // cannot be told apart by the state root.
    assert_eq!(*account, Account::default());
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The consensus identity of §13 is inside the state root, and moving it moves the root.
///
/// This is the property the whole stamping design rests on: if a changed version did not
/// change the root, a chain created under one set of rules could be replayed under another
/// without anything noticing, which is exactly the failure the version exists to prevent.
#[test]
fn the_consensus_identity_is_committed_to_by_every_state_root() {
    let honest = State::new();
    assert_eq!(
        honest.global.consensus_version,
        popcorn_core::constants::CONSENSUS_VERSION,
        "a fresh state carries this binary's version, not zero"
    );
    assert_eq!(
        honest.global.lock_digest,
        popcorn_core::crypto::consensus_lock_digest()
    );

    let mut other_version = State::new();
    other_version.global.consensus_version = popcorn_core::constants::CONSENSUS_VERSION + 1;
    assert_ne!(
        honest.state_root(),
        other_version.state_root(),
        "a different consensus version must produce a different genesis root"
    );

    let mut other_lock = State::new();
    other_lock.global.lock_digest = [0xaa; 32];
    assert_ne!(
        honest.state_root(),
        other_lock.state_root(),
        "a different pinned-dependency digest must produce a different genesis root"
    );
}

/// The digest is over the list, and every part of every entry is inside it.
#[test]
fn the_lock_digest_covers_each_pinned_version() {
    use popcorn_core::constants::CONSENSUS_LOCK;

    let digest = popcorn_core::crypto::consensus_lock_digest();
    assert_eq!(CONSENSUS_LOCK.len(), 12);

    // Length-prefixed encoding, so no re-cutting of a name and a version can collide: the
    // pair ("age", "0.11.5") must not hash like ("age0", ".11.5").
    let mut recut = String::new();
    for (name, version) in CONSENSUS_LOCK {
        recut.push_str(name);
        recut.push_str(version);
    }
    let flat = popcorn_core::crypto::blake3_hash(recut.as_bytes());
    assert_ne!(digest, flat, "the digest is not a bare concatenation");
}

/// The digest and genesis root written into CONSENSUS-LOCK.md and SPEC.md must equal what the
/// code computes. A verifier is told it can reconstruct `lock_digest` from the documented
/// list and algorithm (§13); this fails the build if the document and the code ever disagree,
/// which is the only thing that makes that promise real rather than decorative.
#[test]
fn the_documented_lock_identity_matches_the_code() {
    let digest: String = popcorn_core::crypto::consensus_lock_digest()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let genesis_root: String = popcorn_core::genesis::genesis_state()
        .state_root()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();

    for path in ["/../../CONSENSUS-LOCK.md", "/../../SPEC.md"] {
        let doc = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR")).to_string() + path)
            .expect("read doc");
        assert!(
            doc.contains(&digest),
            "{path} does not contain the current lock digest {digest}"
        );
        assert!(
            doc.contains(&genesis_root),
            "{path} does not contain the current genesis state root {genesis_root}"
        );
    }

    // CONSENSUS_VERSION is cited in more places than the digest, and this project's history is
    // that a number written in two spots diverges the moment it can. Derive its documented
    // form from the constant and require it verbatim where it is normative — both SPEC tables
    // (§11 parameters and §13), the lock annex, and the README banner.
    let version = popcorn_core::constants::CONSENSUS_VERSION;
    let documented = format!(
        "0x{:04x}_{:04x}_{:04x}",
        (version >> 32) & 0xffff,
        (version >> 16) & 0xffff,
        version & 0xffff
    );
    let root = concat!(env!("CARGO_MANIFEST_DIR")).to_string();
    let spec = std::fs::read_to_string(root.clone() + "/../../SPEC.md").unwrap();

    // Section-precise: it is not enough that the value appears somewhere in SPEC.md — the two
    // tables that must never disagree are §11 and §13.
    let section = |from: &str, to: &str| -> String {
        let start = spec
            .find(from)
            .unwrap_or_else(|| panic!("SPEC.md has no {from}"));
        let end = spec[start + from.len()..]
            .find(to)
            .map(|i| start + from.len() + i)
            .unwrap_or(spec.len());
        spec[start..end].to_string()
    };
    assert!(
        section("## 11. Protocol parameters", "## 12.").contains(&documented),
        "§11 parameter table does not cite CONSENSUS_VERSION as {documented}"
    );
    assert!(
        section("## 13. POPCORN-CONSENSUS", "## 14.").contains(&documented),
        "§13 does not cite CONSENSUS_VERSION as {documented}"
    );
    for path in ["/../../CONSENSUS-LOCK.md", "/../../README.md"] {
        let doc = std::fs::read_to_string(root.clone() + path).unwrap();
        assert!(
            doc.contains(&documented),
            "{path} does not cite CONSENSUS_VERSION as {documented}"
        );
    }
}
