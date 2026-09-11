//! Borderline signature vectors (SPEC.md §3.1).
//!
//! RFC 8032 is under-specified around the edges and implementations genuinely disagree there.
//! POPCORN pins one semantics — `ed25519-dalek`'s `verify_strict` — and the pinning only
//! means something if the awkward cases are written down and checked: a verifier that accepts
//! a signature the node rejected disagrees about which transactions exist.
//!
//! This emits each case with the verdict the pinned implementation gives it. The reference
//! executor's pure-Python verifier must return the same verdict for every one.
//!
//!     cargo run -p popcorn-core --example signature_vectors -- vectors/signatures.json

use ed25519_dalek::SigningKey;
use popcorn_core::crypto::{sign, verify_signature};
use serde_json::{json, Value};

/// The eight points of order dividing 8. `verify_strict` refuses them as A and as R: a
/// small-order key verifies signatures it never produced.
const SMALL_ORDER_POINTS: [&str; 8] = [
    "0100000000000000000000000000000000000000000000000000000000000000",
    "ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    "0000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000080",
    "26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc05",
    "c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac03fa",
    "26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc85",
    "c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac037a",
];

/// The group order L, little-endian.
const L_LE: [u8; 32] = [
    0xed, 0xd3, 0xf5, 0x5c, 0x1a, 0x63, 0x12, 0x58, 0xd6, 0x9c, 0xf7, 0xa2, 0xde, 0xf9, 0xde, 0x14,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10,
];

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "vectors/signatures.json".to_string());

    let key = SigningKey::from_bytes(&[11u8; 32]);
    let pubkey = key.verifying_key().to_bytes();
    let message = [42u8; 32];
    let good = sign(&key, &message);

    let mut cases: Vec<Value> = Vec::new();
    let mut add = |name: &str, note: &str, pk: [u8; 32], msg: [u8; 32], sig: [u8; 64]| {
        cases.push(json!({
            "name": name,
            "note": note,
            "public_key": hex(&pk),
            "message": hex(&msg),
            "signature": hex(&sig),
            // The verdict of the pinned implementation. This is the contract.
            "accepted": verify_signature(&pk, &msg, &sig),
        }));
    };

    add("valid", "a well-formed signature", pubkey, message, good);

    let mut wrong_message = message;
    wrong_message[0] ^= 1;
    add(
        "wrong_message",
        "one bit flipped in the message",
        pubkey,
        wrong_message,
        good,
    );

    let mut flipped = good;
    flipped[0] ^= 1;
    add(
        "corrupted_r",
        "one bit flipped in R",
        pubkey,
        message,
        flipped,
    );

    let mut flipped_s = good;
    flipped_s[63] ^= 1;
    add(
        "corrupted_s",
        "one bit flipped in s",
        pubkey,
        message,
        flipped_s,
    );

    // s == L and s == L + 1: non-canonical scalars. Accepting them would give one signature
    // several encodings, and tx_id covers the signature (§4.1).
    let mut s_equals_l = good;
    s_equals_l[32..].copy_from_slice(&L_LE);
    add(
        "s_equals_group_order",
        "s == L, non-canonical",
        pubkey,
        message,
        s_equals_l,
    );

    let mut s_above_l = good;
    let mut l_plus_one = L_LE;
    l_plus_one[0] = l_plus_one[0].wrapping_add(1);
    s_above_l[32..].copy_from_slice(&l_plus_one);
    add(
        "s_above_group_order",
        "s == L + 1",
        pubkey,
        message,
        s_above_l,
    );

    let mut s_max = good;
    s_max[32..].copy_from_slice(&[0xff; 32]);
    add("s_all_ones", "s is every bit set", pubkey, message, s_max);

    // Small-order public keys and commitments.
    for (index, point) in SMALL_ORDER_POINTS.iter().enumerate() {
        let bytes: [u8; 32] = unhex(point).try_into().unwrap();
        add(
            &format!("small_order_public_key_{index}"),
            "A is a point of order dividing 8",
            bytes,
            message,
            good,
        );

        let mut sig = good;
        sig[..32].copy_from_slice(&bytes);
        add(
            &format!("small_order_r_{index}"),
            "R is a point of order dividing 8",
            pubkey,
            message,
            sig,
        );
    }

    // A y coordinate at or above p: a non-canonical field encoding.
    let mut non_canonical_a = [0xffu8; 32];
    non_canonical_a[31] = 0x7f;
    add(
        "non_canonical_public_key",
        "A encodes y >= p",
        non_canonical_a,
        message,
        good,
    );

    let mut non_canonical_r = good;
    non_canonical_r[..32].copy_from_slice(&non_canonical_a);
    add(
        "non_canonical_r",
        "R encodes y >= p",
        pubkey,
        message,
        non_canonical_r,
    );

    add(
        "zero_everything",
        "all zeroes",
        [0u8; 32],
        message,
        [0u8; 64],
    );

    let accepted = cases
        .iter()
        .filter(|c| c["accepted"] == json!(true))
        .count();
    let document = json!({
        "note": "Borderline Ed25519 cases (SPEC.md §3.1). `accepted` is the verdict of the \
                 pinned ed25519-dalek verify_strict; any implementation claiming to replay \
                 POPCORN must agree on every one.",
        "semantics": "ed25519-dalek verify_strict: canonical s, no small-order A or R, \
                      cofactorless equation",
        "accepted_count": accepted,
        "rejected_count": cases.len() - accepted,
        "cases": cases,
    });

    if let Some(parent) = std::path::Path::new(&path).parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(
        &path,
        format!("{}\n", serde_json::to_string_pretty(&document).unwrap()),
    )
    .expect("write signature vectors");
    println!(
        "wrote {} cases to {path} ({accepted} accepted, {} rejected)",
        document["cases"].as_array().unwrap().len(),
        document["rejected_count"]
    );
}
