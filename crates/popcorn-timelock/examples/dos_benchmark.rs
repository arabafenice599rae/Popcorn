//! The mandatory pre-genesis worst-case benchmark (SPEC.md §11).
//!
//! The collection phase is blind: no fee can be charged before decryption, because the signer
//! is not known until then. So the only thing standing between the node and a flood is CPU,
//! and the specification is explicit that the CPU worst case "may not be the obvious one".
//! This measures it instead of assuming it.
//!
//! Three populations, as §11 prescribes — valid ciphertexts, ciphertexts whose tlock stanza
//! is corrupt, and outright garbage — plus two the specification does not name but an
//! attacker would find on their own: blobs that decrypt but do not decode, and headers padded
//! to the profile limit.
//!
//! This is an *availability* measurement, not a correctness one. Nothing here can change a
//! state root.
//!
//!     cargo run --release -p popcorn-timelock --example dos_benchmark -- [count]

use std::time::Instant;

use popcorn_timelock::{blob, profile, Beacon};

const CHAIN_HASH_HEX: &str = "52db9ba70e0cc0f6eaf7803dd07447a1f5477735fd3f661792ba94600c84e971";
const PUBLIC_KEY_HEX: &str = "83cf0f2896adee7eb8b5f01fcad3912212c437e0073e911fb90022d3e760183c8c4b450b6a0a6c3ac6a5776a2d1064510d1fec758c921cc22b0e17e63aaf4bcb5ed66304de9cf809bd274ca73bab4af5a6e9c76a4bc09e76eae8991ef5ece45a";
/// A real quicknet beacon: round 1000 and its signature.
const ROUND: u64 = 1_000;
const SIGNATURE_HEX: &str = "b44679b9a59af2ec876b1a6b1ad52ea9b1615fc3982b19576350f93447cb1125e342b73a8dd2bacbe47e4b6b63ed5e39";

/// The batch ceiling of §11. One round's worth of admitted blobs.
const DEFAULT_COUNT: usize = 10_000;
/// A round is 3 seconds. Decryption that cannot finish inside it makes the chain wait.
const ROUND_BUDGET_MS: f64 = 3_000.0;

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}

struct Population {
    name: &'static str,
    note: &'static str,
    blobs: Vec<Vec<u8>>,
}

fn main() {
    let count: usize = std::env::args()
        .nth(1)
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_COUNT);

    let chain_hash: [u8; 32] = unhex(CHAIN_HASH_HEX).try_into().unwrap();
    let public_key = unhex(PUBLIC_KEY_HEX);
    let beacon = Beacon {
        round: ROUND,
        signature: unhex(SIGNATURE_HEX),
    };

    println!("POPCORN blind-collection worst case — {count} blobs per population");
    println!("(a round is 3 s; anything above that budget means the chain waits)\n");

    // Build one real ciphertext and derive the hostile populations from it, so the shapes
    // differ only where the attack does.
    let payload = borsh::to_vec(&sample_transaction()).expect("borsh");
    let valid = blob::encrypt(&payload, &chain_hash, &public_key, ROUND).expect("encrypt");

    let mut corrupt_tlock = valid.clone();
    // Corrupt the stanza body while keeping it valid base64, so the profile still accepts it
    // and the cost lands where it belongs: on the timelock unwrap. Flipping a bit instead
    // would usually produce a non-base64 character, and the profile would refuse it in
    // microseconds — measuring the parser rather than the attack.
    let body_offset = find_body_offset(&corrupt_tlock);
    corrupt_tlock[body_offset] = if corrupt_tlock[body_offset] == b'A' {
        b'B'
    } else {
        b'A'
    };

    let mut corrupt_payload = valid.clone();
    // Corrupt the STREAM payload instead: tlock succeeds, the AEAD fails.
    let last = corrupt_payload.len() - 1;
    corrupt_payload[last] ^= 0xff;

    let undecodable = {
        // Decrypts cleanly and is not a transaction: the `unusable` case that costs the most,
        // because every stage runs before the decode refuses it (§5.1).
        blob::encrypt(b"not a transaction", &chain_hash, &public_key, ROUND).expect("encrypt")
    };

    let padded = {
        // A header padded to the profile ceiling: maximum parsing work per blob. Note this
        // one is accepted by the profile and then fails in the timelock, so it measures
        // parsing *plus* a failed unwrap.
        let mut bytes = Vec::from(&b"age-encryption.org/v1\n"[..]);
        bytes.extend_from_slice(format!("-> tlock {ROUND} {CHAIN_HASH_HEX}\n").as_bytes());
        while bytes.len() < profile::MAX_HEADER_SIZE - 80 {
            bytes.extend_from_slice(
                b"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\n",
            );
        }
        bytes.extend_from_slice(b"--- AAAA\n\x00");
        bytes
    };

    let garbage: Vec<u8> = (0..2048u32).map(|i| (i * 7 + 13) as u8).collect();

    let populations = vec![
        Population {
            name: "valid ciphertexts",
            note: "the honest case: full tlock unwrap, AEAD, and Borsh decode",
            blobs: vec![valid.clone(); count],
        },
        Population {
            name: "corrupt tlock stanza",
            note: "profile passes, the timelock unwrap fails",
            blobs: vec![corrupt_tlock; count],
        },
        Population {
            name: "corrupt payload",
            note: "timelock succeeds, the AEAD fails",
            blobs: vec![corrupt_payload; count],
        },
        Population {
            name: "decrypts, does not decode",
            note: "every stage runs, then Borsh refuses it — `unusable` at full price",
            blobs: vec![undecodable; count],
        },
        Population {
            name: "profile-limit headers",
            note: "1 KiB of header to parse before the profile refuses it",
            blobs: vec![padded; count],
        },
        Population {
            name: "garbage",
            note: "refused by the profile on the first bytes",
            blobs: vec![garbage; count],
        },
    ];

    let mut worst: (f64, &str) = (0.0, "");
    println!(
        "{:<28} {:>10} {:>12} {:>10}  what it costs",
        "population", "total", "per blob", "of budget"
    );
    println!("{}", "-".repeat(100));

    for population in &populations {
        let started = Instant::now();
        let mut decoded = 0usize;
        for bytes in &population.blobs {
            // Exactly the node's path: profile, then decrypt, then decode (§5.1).
            if let Ok(plaintext) = blob::decrypt(bytes, &chain_hash, &beacon) {
                if borsh::from_slice::<popcorn_core::SignedTx>(&plaintext).is_ok() {
                    decoded += 1;
                }
            }
        }
        let elapsed = started.elapsed().as_secs_f64() * 1_000.0;
        let per_blob = elapsed / population.blobs.len() as f64;
        let budget = elapsed / ROUND_BUDGET_MS * 100.0;
        if elapsed > worst.0 {
            worst = (elapsed, population.name);
        }
        println!(
            "{:<28} {:>8.0} ms {:>9.3} ms {:>9.1}%  {}",
            population.name, elapsed, per_blob, budget, population.note
        );
        debug_assert!(decoded == 0 || population.name == "valid ciphertexts");
    }

    // The same worst population across cores. Decryption parallelises cleanly — the derived
    // set does not depend on the order it is computed in — so this is the number that decides
    // whether a ceiling is reachable, not the single-core one.
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let hostile = &populations
        .iter()
        .find(|p| p.name == worst.1)
        .expect("worst population")
        .blobs;
    let started = Instant::now();
    let chunk_size = hostile.len().div_ceil(cores);
    std::thread::scope(|scope| {
        for chunk in hostile.chunks(chunk_size) {
            let beacon = beacon.clone();
            scope.spawn(move || {
                for bytes in chunk {
                    let _ = blob::decrypt(bytes, &chain_hash, &beacon);
                }
            });
        }
    });
    let parallel_ms = started.elapsed().as_secs_f64() * 1_000.0;

    println!(
        "\nworst population: {} at {:.0} ms for {count} blobs",
        worst.1, worst.0
    );
    println!(
        "  across {cores} cores: {parallel_ms:.0} ms ({:.1}x of a round)",
        parallel_ms / ROUND_BUDGET_MS
    );
    println!(
        "  cores needed to clear {count} of them inside one round: {:.0}",
        (worst.0 / ROUND_BUDGET_MS).ceil()
    );
    if worst.0 > ROUND_BUDGET_MS {
        println!(
            "OVER BUDGET: a full round of this population takes {:.1}x the 3 s round time.\n\
             This is the case MAX_TLOCK_DECRYPT_WORK_PER_ROUND has to bound (§11): the chain\n\
             waits rather than skipping manifested blobs.",
            worst.0 / ROUND_BUDGET_MS
        );
    } else {
        println!(
            "within budget: the worst population uses {:.1}% of a round.",
            worst.0 / ROUND_BUDGET_MS * 100.0
        );
    }
}

/// Offset of the first stanza body byte, so corruption lands where it is meant to.
fn find_body_offset(blob: &[u8]) -> usize {
    let text = String::from_utf8_lossy(&blob[..blob.len().min(1024)]);
    let mut offset = 0;
    for line in text.lines() {
        offset += line.len() + 1;
        if line.starts_with("-> tlock ") {
            return offset;
        }
    }
    0
}

fn sample_transaction() -> popcorn_core::SignedTx {
    use popcorn_core::types::{Action, SignedTx, TxPayload};
    let payload = TxPayload {
        nonce: 1,
        target_round: ROUND,
        action: Action::Transfer {
            token: popcorn_core::NATIVE_TOKEN,
            to: [9u8; 32],
            amount: 1_000,
        },
    };
    SignedTx {
        payload,
        signer_pubkey: [7u8; 32],
        signature: [0u8; 64],
    }
}
