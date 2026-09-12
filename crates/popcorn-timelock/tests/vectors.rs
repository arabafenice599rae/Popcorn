//! Replay of the committed consensus-grade vector (SPEC.md §10).
//!
//! The fixture was produced against a real drand quicknet round by
//! `examples/make_vectors.rs`; this test replays it with no network at all. Any independent
//! implementation can do the same from `vectors/end_to_end.json`, which is the point: the
//! promise of reproducible verification has to cover encryption too, not only replay.

use std::collections::BTreeMap;

use ed25519_dalek::SigningKey;
use popcorn_core::constants::NATIVE_TOKEN;
use popcorn_core::crypto::{account_id_from_pubkey, blake3_hash, verify_signature};
use popcorn_core::execute::{execute_batch, BatchInput};
use popcorn_core::state::{Journal, State};
use popcorn_core::types::{ReceiptPayload, SignedTx};
use popcorn_timelock::{blob, Beacon};

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex"))
        .collect()
}

fn array32(text: &str) -> [u8; 32] {
    unhex(text).try_into().expect("32 bytes")
}

fn vector() -> BTreeMap<String, String> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../vectors/end_to_end.json");
    let raw = std::fs::read_to_string(path).expect("committed vector");
    serde_json::from_str(&raw).expect("vector parses")
}

/// The blob decrypts to exactly the recorded transaction bytes, with no network access.
#[test]
fn the_recorded_blob_decrypts_to_the_recorded_transaction() {
    let vector = vector();
    let chain_hash = array32(&vector["chain_hash"]);
    let beacon = Beacon {
        round: vector["round"].parse().unwrap(),
        signature: unhex(&vector["beacon_signature"]),
    };

    let blob_bytes = unhex(&vector["blob"]);
    assert_eq!(
        blake3_hash(&blob_bytes),
        array32(&vector["blob_hash"]),
        "the blob hash in the manifest must be blake3 of these exact bytes"
    );

    let plaintext = blob::decrypt(&blob_bytes, &chain_hash, &beacon).expect("decryption");
    assert_eq!(plaintext, unhex(&vector["tx_borsh"]));

    let tx: SignedTx = borsh::from_slice(&plaintext).expect("decode");
    assert_eq!(tx.tx_id(), array32(&vector["tx_id"]));
    assert_eq!(tx.signer(), array32(&vector["signer_account"]));

    // The signature inside verifies under the frozen semantics.
    assert!(verify_signature(
        &tx.signer_pubkey,
        &popcorn_core::crypto::signing_hash(&tx.payload),
        &tx.signature
    ));
}

/// Re-executing the recorded batch reproduces every root, byte for byte.
#[test]
fn replaying_the_recorded_batch_reproduces_every_root() {
    let vector = vector();
    let tx: SignedTx = borsh::from_slice(&unhex(&vector["tx_borsh"])).expect("decode");
    let signer = tx.signer();

    let mut state = State::new();
    let mut journal = Journal::new();
    state
        .credit(&signer, &NATIVE_TOKEN, 1_000_000, &mut journal)
        .unwrap();
    state.global_mut(&mut journal).native_emitted = 1_000_000;
    assert_eq!(state.state_root(), array32(&vector["pre_state_root"]));

    let output = execute_batch(
        &mut state,
        BatchInput {
            height: 1,
            prev_hash: [0u8; 32],
            drand_round: vector["round"].parse().unwrap(),
            drand_signature: unhex(&vector["beacon_signature"]),
            blob_manifest: vec![array32(&vector["blob_hash"])],
            unusable: vec![],
            txs: vec![tx],
            foundation: array32(&vector["foundation_account"]),
        },
    );

    assert_eq!(
        output.header.collection_root,
        array32(&vector["collection_root"])
    );
    assert_eq!(output.header.txs_root, array32(&vector["txs_root"]));
    assert_eq!(
        output.header.rejected_root,
        array32(&vector["rejected_root"])
    );
    assert_eq!(output.header.results_root, array32(&vector["results_root"]));
    assert_eq!(output.header.state_root, array32(&vector["state_root"]));
    assert_eq!(output.header.block_hash(), array32(&vector["block_hash"]));
    assert!(state.monetary_invariant_holds(0));
}

/// The receipt preimage is pinned: one normative encoding, reproducible in any language.
#[test]
fn the_recorded_receipt_reproduces_and_verifies() {
    let vector = vector();
    let payload = ReceiptPayload {
        domain: popcorn_core::constants::RECEIPT_DOMAIN.to_string(),
        blob_hash: array32(&vector["blob_hash"]),
        target_round: vector["round"].parse().unwrap(),
        timestamp_ms: 1_700_000_000_000,
    };

    assert_eq!(
        borsh::to_vec(&payload).unwrap(),
        unhex(&vector["receipt_borsh"])
    );
    assert_eq!(payload.receipt_hash(), array32(&vector["receipt_hash"]));

    let signature: [u8; 64] = unhex(&vector["receipt_signature"]).try_into().unwrap();
    assert!(verify_signature(
        &array32(&vector["node_pubkey"]),
        &payload.receipt_hash(),
        &signature
    ));

    // The node key in the vector is a test key; the account it would own is not the
    // foundation's, which is the separation §1 requires.
    let node_key = SigningKey::from_bytes(&[3u8; 32]);
    assert_ne!(
        account_id_from_pubkey(&node_key.verifying_key().to_bytes()),
        array32(&vector["foundation_account"])
    );
}

/// A beacon from the wrong round must not decrypt the blob: that is the timelock working.
#[test]
fn a_wrong_round_beacon_does_not_open_the_blob() {
    let vector = vector();
    let chain_hash = array32(&vector["chain_hash"]);
    let blob_bytes = unhex(&vector["blob"]);

    let wrong = Beacon {
        round: vector["round"].parse::<u64>().unwrap() + 1,
        signature: unhex(&vector["beacon_signature"]),
    };
    // The profile check catches it before any cryptography: the header names its round.
    assert!(blob::decrypt(&blob_bytes, &chain_hash, &wrong).is_err());

    // A right round with a forged signature fails in the AEAD instead.
    let forged = Beacon {
        round: vector["round"].parse().unwrap(),
        signature: vec![0u8; 48],
    };
    assert!(blob::decrypt(&blob_bytes, &chain_hash, &forged).is_err());
}

/// A profile-valid blob whose decryption terminates abnormally in the pinned tlock is a
/// defined `unusable`, not a crash (SPEC.md §3.6, §5.1). Before the containment in
/// `blob::decrypt` this input halted a node's block production; this replays the committed
/// vector and asserts the total-function outcome, without the test process dying.
#[test]
fn an_abnormal_termination_is_contained_as_unusable() {
    use popcorn_timelock::{blob, Beacon, TimelockError};

    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../vectors/halt.json");
    let raw = std::fs::read_to_string(path).expect("vectors/halt.json");
    let get = |key: &str| -> String {
        let needle = format!("\"{key}\": \"");
        let start = raw.find(&needle).expect("field") + needle.len();
        raw[start..start + raw[start..].find('"').unwrap()].to_string()
    };
    let unhex = |t: &str| -> Vec<u8> {
        (0..t.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&t[i..i + 2], 16).unwrap())
            .collect()
    };

    let chain: [u8; 32] = unhex(&get("chain_hash")).try_into().unwrap();
    let round: u64 = raw[raw.find("\"round\": ").unwrap() + 9..]
        .split(|c: char| !c.is_ascii_digit())
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let beacon = Beacon {
        round,
        signature: unhex(&get("beacon_signature")),
    };
    let bytes = unhex(&get("blob"));

    // The profile accepts it — this is not a malformed blob, it is a crafted one.
    assert!(
        popcorn_timelock::profile::validate(&bytes, round, &chain).is_ok(),
        "the halt vector must pass the profile; that is what makes it dangerous"
    );

    // And decryption returns — it does not panic, and the outcome is the contained one.
    match blob::decrypt(&bytes, &chain, &beacon) {
        Err(TimelockError::Aborted) => {}
        other => panic!("expected Aborted (unusable), got {other:?}"),
    }

    // The whole point: as a transaction, it is `unusable` — no tx, so no tx_id.
    let provider = popcorn_timelock::static_provider::StaticTimelock::new(chain, Vec::new(), 0, 3)
        .with_beacon(beacon.clone());
    assert!(
        popcorn_timelock::decrypt_transaction(&provider, &bytes, &beacon).is_none(),
        "a contained abnormal termination yields no transaction"
    );
}
