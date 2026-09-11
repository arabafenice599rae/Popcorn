//! Generate the consensus-grade end-to-end test vectors of SPEC.md §10.
//!
//! This is the only thing in the repository that needs the network: it reaches a real drand
//! quicknet round, encrypts a real transaction toward it, and records every intermediate
//! value. The committed fixture is then replayed offline by `tests/vectors.rs`, and by any
//! independent implementation that wants to check itself against this one.
//!
//!     cargo run -p popcorn-timelock --example make_vectors -- vectors/end_to_end.json

use std::collections::BTreeMap;

use ed25519_dalek::SigningKey;
use popcorn_core::constants::{DRAND_CHAIN_HASH, NATIVE_TOKEN};
use popcorn_core::crypto::{account_id_from_pubkey, sign_payload};
use popcorn_core::execute::{execute_batch, BatchInput};
use popcorn_core::state::{Journal, State};
use popcorn_core::types::{Action, SignedTx, TxPayload};
use popcorn_timelock::{BeaconOutcome, DrandTimelock, TimelockProvider};

/// A round far enough in the past that its beacon is permanently available.
const VECTOR_ROUND: u64 = 1_000;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "vectors/end_to_end.json".to_string());

    let timelock = DrandTimelock::connect(&["https://api.drand.sh"]).expect("drand reachable");

    // A deterministic signer, so the vector is reproducible from the seed alone.
    let key = SigningKey::from_bytes(&[7u8; 32]);
    let signer_pubkey = key.verifying_key().to_bytes();
    let signer = account_id_from_pubkey(&signer_pubkey);
    let recipient = account_id_from_pubkey(&[9u8; 32]);

    let payload = TxPayload {
        nonce: 1,
        target_round: VECTOR_ROUND,
        action: Action::Transfer {
            token: NATIVE_TOKEN,
            to: recipient,
            amount: 250_000,
        },
    };
    let tx = SignedTx {
        signature: sign_payload(&key, &payload),
        payload,
        signer_pubkey,
    };
    let tx_bytes = borsh::to_vec(&tx).expect("borsh");

    let blob = timelock
        .encrypt(&tx_bytes, VECTOR_ROUND)
        .expect("encryption");
    let blob_hash = popcorn_core::crypto::blake3_hash(&blob);

    let beacon = match timelock.get_beacon(VECTOR_ROUND) {
        BeaconOutcome::Available(beacon) => beacon,
        other => panic!("beacon unavailable: {other:?}"),
    };

    // Round-trip through the profile and back into a transaction.
    let decrypted = timelock.decrypt(&blob, &beacon).expect("decryption");
    assert_eq!(decrypted, tx_bytes, "round trip is not byte-identical");

    // Execute a one-transaction batch and record the resulting commitment.
    let mut state = State::new();
    let mut journal = Journal::new();
    state
        .credit(&signer, &NATIVE_TOKEN, 1_000_000, &mut journal)
        .unwrap();
    state.global_mut(&mut journal).native_emitted = 1_000_000;
    let pre_state_root = state.state_root();

    let foundation = account_id_from_pubkey(&[200u8; 32]);
    let output = execute_batch(
        &mut state,
        BatchInput {
            height: 1,
            prev_hash: [0u8; 32],
            drand_round: VECTOR_ROUND,
            drand_signature: beacon.signature.clone(),
            blob_manifest: vec![blob_hash],
            unusable: vec![],
            txs: vec![tx.clone()],
            foundation,
        },
    );

    let mut fields: BTreeMap<&str, String> = BTreeMap::new();
    fields.insert("chain_hash", DRAND_CHAIN_HASH.to_string());
    fields.insert("round", VECTOR_ROUND.to_string());
    fields.insert("beacon_signature", hex(&beacon.signature));
    fields.insert("signer_seed", hex(&[7u8; 32]));
    fields.insert("signer_pubkey", hex(&signer_pubkey));
    fields.insert("signer_account", hex(&signer));
    fields.insert("recipient_account", hex(&recipient));
    fields.insert("foundation_account", hex(&foundation));
    fields.insert("tx_borsh", hex(&tx_bytes));
    fields.insert("tx_id", hex(&tx.tx_id()));
    fields.insert("blob", hex(&blob));
    fields.insert("blob_hash", hex(&blob_hash));
    fields.insert("pre_state_root", hex(&pre_state_root));
    fields.insert("collection_root", hex(&output.header.collection_root));
    fields.insert("txs_root", hex(&output.header.txs_root));
    fields.insert("rejected_root", hex(&output.header.rejected_root));
    fields.insert("results_root", hex(&output.header.results_root));
    fields.insert("state_root", hex(&output.header.state_root));
    fields.insert("block_hash", hex(&output.header.block_hash()));

    // A receipt over the same blob, so the receipt preimage is pinned too (§9.2).
    let node_key = SigningKey::from_bytes(&[3u8; 32]);
    let receipt_payload = popcorn_core::types::ReceiptPayload {
        domain: popcorn_core::constants::RECEIPT_DOMAIN.to_string(),
        blob_hash,
        target_round: VECTOR_ROUND,
        timestamp_ms: 1_700_000_000_000,
    };
    fields.insert("node_pubkey", hex(&node_key.verifying_key().to_bytes()));
    fields.insert("receipt_borsh", hex(&borsh::to_vec(&receipt_payload).unwrap()));
    fields.insert("receipt_hash", hex(&receipt_payload.receipt_hash()));
    fields.insert(
        "receipt_signature",
        hex(&popcorn_core::crypto::sign(&node_key, &receipt_payload.receipt_hash())),
    );

    let json = serde_json::to_string_pretty(&fields).unwrap();
    if let Some(parent) = std::path::Path::new(&path).parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(&path, format!("{json}\n")).expect("write vector");
    println!("wrote {path}");
}
