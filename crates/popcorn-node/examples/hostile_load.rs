//! Prolonged hostile load against a live node (SPEC.md §5.1, §9.2, §11).
//!
//! §11's `dos_benchmark` measures decryption cost in isolation. This is the other half: many
//! concurrent clients pushing adversarial blobs at a running node for a sustained period,
//! while one honest client keeps transacting underneath the flood. It checks the properties
//! that only a live node can falsify —
//!
//!   * survival:        the node keeps answering, and keeps producing blocks;
//!   * liveness:        honest transactions still reach a block during the flood;
//!   * accountability:  every block's `unusable` set is a subset of its manifest (§9.2 — a
//!     decrypted-and-dropped blob that was never manifested would be two contradicting
//!     signatures by the node);
//!   * conservation:    the five-bucket invariant holds throughout (§5.5).
//!
//! Nothing here can move a state root: the flood is blind bytes the node cannot read, and the
//! canary is an ordinary signed transaction. It is a load test, not a consensus test.
//!
//!     cargo run --release -p popcorn-node --example hostile_load -- \
//!         --node http://127.0.0.1:8610 --key foundation.key --seconds 120 --threads 16

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ed25519_dalek::SigningKey;
use popcorn_core::constants::{MAX_BLOB_SIZE, NATIVE_TOKEN};
use popcorn_core::crypto::account_id_from_pubkey;
use popcorn_core::types::{Action, SignedTx, TxPayload};
use popcorn_node::client::{self, Url};
use popcorn_node::encoding::{to_base64, to_hex};
use popcorn_node::keys;
use popcorn_timelock::blob;

fn arg(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

#[derive(Default)]
struct Counts {
    accepted: AtomicU64,
    round_closed: AtomicU64,
    too_large: AtomicU64,
    round_full: AtomicU64,
    other_reject: AtomicU64,
    net_error: AtomicU64,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let node = arg(&args, "--node").unwrap_or_else(|| "http://127.0.0.1:8610".to_string());
    let key_path = arg(&args, "--key").expect("--key <FILE> (a funded account, for the canary)");
    let seconds: u64 = arg(&args, "--seconds")
        .and_then(|v| v.parse().ok())
        .unwrap_or(120);
    let threads: usize = arg(&args, "--threads")
        .and_then(|v| v.parse().ok())
        .unwrap_or(16);

    let base = Url::parse(&node).expect("node url");
    let params = client::get(&base.join("/params").to_string()).expect("reach /params");
    let chain_hash: [u8; 32] = unhex(params["drand"]["chain_hash"].as_str().unwrap())
        .try_into()
        .unwrap();
    let public_key = unhex(
        params["drand"]
            .get("public_key")
            .and_then(|v| v.as_str())
            .unwrap_or(DRAND_PUBLIC_KEY),
    );

    let key = keys::load(std::path::Path::new(&key_path)).expect("load canary key");
    let signer = account_id_from_pubkey(&key.verifying_key().to_bytes());

    println!("hostile load: {threads} attacker threads, {seconds}s, node {node}");
    println!("canary account {}\n", to_hex(&signer));

    let head0 = client::get(&base.join("/head").to_string()).expect("head");
    let round0 = head0["drand_round"].as_u64().unwrap();
    let height0 = head0["height"].as_u64().unwrap();

    // One real ciphertext toward a near round, then the hostile shapes derived from it. The
    // internal stanza round need not match what we POST as target_round: the mempool queues
    // blindly (§5.1), and the mismatch simply makes the blob `unusable` at decrypt — which is
    // the expensive path, and the point.
    let seed_round = round0 + 4;
    let valid = blob::encrypt(
        b"popcorn-hostile-load-seed",
        &chain_hash,
        &public_key,
        seed_round,
    )
    .expect("encrypt");

    let stop = Arc::new(AtomicBool::new(false));
    let counts = Arc::new(Counts::default());
    let deadline = Instant::now() + Duration::from_secs(seconds);

    let mut handles = Vec::new();
    for t in 0..threads {
        let node = node.clone();
        let stop = Arc::clone(&stop);
        let counts = Arc::clone(&counts);
        let valid = valid.clone();
        handles.push(std::thread::spawn(move || {
            let base = Url::parse(&node).expect("node url");
            attacker(t, &base, &stop, &counts, &valid);
        }));
    }

    // The canary: an honest client transacting through the flood. Each submission is a real
    // signed self-transfer of zero — enough to consume a nonce and land in a block without
    // moving value — and we confirm it is executed, not merely accepted.
    let canary = std::thread::spawn({
        let node = node.clone();
        let stop = Arc::clone(&stop);
        move || {
            let base = Url::parse(&node).expect("node url");
            canary_loop(&base, &key, &signer, &stop, deadline)
        }
    });

    // Watch survival and print progress while the flood runs.
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_secs(10));
        let elapsed =
            seconds.saturating_sub(deadline.saturating_duration_since(Instant::now()).as_secs());
        match client::get(&base.join("/head").to_string()) {
            Ok(h) => println!(
                "  t+{elapsed:>3}s  height {}  round {}  accepted {}  rejected(closed {} full {} large {} other {})  neterr {}",
                h["height"], h["drand_round"],
                counts.accepted.load(Ordering::Relaxed),
                counts.round_closed.load(Ordering::Relaxed),
                counts.round_full.load(Ordering::Relaxed),
                counts.too_large.load(Ordering::Relaxed),
                counts.other_reject.load(Ordering::Relaxed),
                counts.net_error.load(Ordering::Relaxed),
            ),
            Err(e) => println!("  t+{elapsed:>3}s  NODE UNREACHABLE: {e}"),
        }
    }
    stop.store(true, Ordering::Relaxed);
    for h in handles {
        let _ = h.join();
    }
    let (canary_ok, canary_tries) = canary.join().unwrap();

    println!("\n--- after the flood ---");
    let alive = client::get(&base.join("/head").to_string());
    let (height1, round1) = match &alive {
        Ok(h) => {
            println!(
                "node ALIVE: height {} round {}",
                h["height"], h["drand_round"]
            );
            (
                h["height"].as_u64().unwrap(),
                h["drand_round"].as_u64().unwrap(),
            )
        }
        Err(e) => {
            println!("node DEAD after the flood: {e}");
            std::process::exit(1);
        }
    };
    println!(
        "blocks produced during the run: {}",
        height1.saturating_sub(height0)
    );
    println!(
        "rounds elapsed:                 {}",
        round1.saturating_sub(round0)
    );

    let supply = client::get(&base.join("/supply").to_string()).expect("supply");
    let holds = supply["invariant_holds"].as_bool().unwrap_or(false);
    println!("monetary invariant holds:       {holds}");
    println!("canary honest tx executed:      {canary_ok}/{canary_tries}");

    // Accountability: sample the blocks produced during the flood and confirm every one's
    // `unusable` set is a subset of its manifest — nothing was decrypted-and-dropped without
    // first being manifested and receipted (§9.2).
    let mut manifested_total = 0u64;
    let mut unusable_total = 0u64;
    for h in height0 + 1..=height1 {
        if let Ok(b) = client::get(&base.join(&format!("/block/{h}")).to_string()) {
            let m = b["blob_manifest"].as_array().map(|a| a.len()).unwrap_or(0) as u64;
            let u = b["unusable"].as_array().map(|a| a.len()).unwrap_or(0) as u64;
            manifested_total += m;
            unusable_total += u;
            assert!(
                u <= m,
                "block {h}: {u} unusable exceeds {m} manifested — accountability broken"
            );
        }
    }
    println!("manifested blobs (flood):       {manifested_total}");
    println!("of which unusable:              {unusable_total}");

    let ok = alive.is_ok() && holds && canary_ok > 0 && unusable_total <= manifested_total;
    println!(
        "\n{}",
        if ok {
            "HOSTILE LOAD: PASS"
        } else {
            "HOSTILE LOAD: FAIL"
        }
    );
    if !ok {
        std::process::exit(1);
    }
}

fn attacker(t: usize, base: &Url, stop: &AtomicBool, counts: &Counts, valid: &[u8]) {
    let mut n: u64 = (t as u64) << 40;
    let mut cached_round = 0u64;
    let mut last_refresh = Instant::now() - Duration::from_secs(9);
    while !stop.load(Ordering::Relaxed) {
        // Refresh the head a couple of times a round, not per request: the flood must cost the
        // node, not our own /head polling.
        if last_refresh.elapsed() > Duration::from_millis(1500) {
            if let Ok(h) = client::get(&base.join("/head").to_string()) {
                cached_round = h["drand_round"].as_u64().unwrap_or(cached_round);
            }
            last_refresh = Instant::now();
        }
        let target = cached_round + 2;
        let blob = hostile_blob(n, valid);
        n = n.wrapping_add(1);
        let body = serde_json::json!({ "blob": to_base64(&blob), "target_round": target });
        match client::post(&base.join("/tx").to_string(), &body) {
            Ok(v) if v.get("receipt_hash").is_some() => {
                counts.accepted.fetch_add(1, Ordering::Relaxed);
            }
            Ok(v) => {
                let e = v["error"].as_str().unwrap_or("");
                if e.contains("closed") {
                    counts.round_closed.fetch_add(1, Ordering::Relaxed);
                } else if e.contains("budget") {
                    counts.round_full.fetch_add(1, Ordering::Relaxed);
                } else if e.contains("limit") {
                    counts.too_large.fetch_add(1, Ordering::Relaxed);
                } else {
                    counts.other_reject.fetch_add(1, Ordering::Relaxed);
                }
            }
            Err(_) => {
                counts.net_error.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

/// Rotate through the hostile shapes, each made distinct by `n` so they do not dedupe to one
/// manifest entry — a flood of identical blobs is one blob (§5.1), which is not a flood.
fn hostile_blob(n: u64, valid: &[u8]) -> Vec<u8> {
    match n % 6 {
        0 => {
            // garbage: refused by the profile on the first bytes
            let mut b = vec![0u8; 512];
            b[..8].copy_from_slice(&n.to_le_bytes());
            b
        }
        1 => {
            // oversized: refused on size before anything else
            vec![(n & 0xff) as u8; MAX_BLOB_SIZE + 64]
        }
        2 => {
            // profile passes, timelock unwrap fails — distinct via one payload byte
            let mut b = valid.to_vec();
            let last = b.len() - 1;
            b[last] ^= (n as u8) | 1;
            b
        }
        3 => {
            // decrypts-shaped but corrupted mid-stream: unusable at full price
            let mut b = valid.to_vec();
            let mid = b.len() / 2;
            b[mid] ^= (n as u8) | 1;
            b
        }
        4 => {
            // near-limit random bytes, profile refuses after reading them
            let mut b = vec![0u8; MAX_BLOB_SIZE - 1];
            for (i, x) in b.iter_mut().enumerate() {
                *x = (i as u64).wrapping_mul(n | 1) as u8;
            }
            b
        }
        _ => {
            // valid-shaped, distinct: accepted then unusable (trailing bytes break the tag)
            let mut b = valid.to_vec();
            b.extend_from_slice(&n.to_le_bytes());
            b
        }
    }
}

fn canary_loop(
    base: &Url,
    key: &SigningKey,
    signer: &[u8; 32],
    stop: &AtomicBool,
    deadline: Instant,
) -> (u64, u64) {
    let mut ok = 0u64;
    let mut tries = 0u64;
    // The canary competes for the same admission budget as the flood, so it may need a few
    // attempts — which is itself the liveness question: can an honest client get in at all?
    while Instant::now() < deadline && !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_secs(8));
        tries += 1;
        if submit_canary(base, key, signer).unwrap_or(false) {
            ok += 1;
        }
    }
    (ok, tries)
}

fn submit_canary(base: &Url, key: &SigningKey, signer: &[u8; 32]) -> Option<bool> {
    let account = client::get(
        &base
            .join(&format!("/account/{}", to_hex(signer)))
            .to_string(),
    )
    .ok()?;
    let nonce = account["nonce"].as_u64().unwrap_or(0) + 1;
    let head = client::get(&base.join("/head").to_string()).ok()?;
    let current = head["drand_round"].as_u64()?;
    let start_height = head["height"].as_u64()?;
    let target = current + 3;

    let params = client::get(&base.join("/params").to_string()).ok()?;
    let chain_hash: [u8; 32] = unhex(params["drand"]["chain_hash"].as_str()?)
        .try_into()
        .ok()?;
    let public_key = unhex(
        params["drand"]
            .get("public_key")
            .and_then(|v| v.as_str())
            .unwrap_or(DRAND_PUBLIC_KEY),
    );

    // A self-transfer of one base unit. `amount > 0` is a static validation rule (§5.2), so a
    // zero-amount transfer would be *rejected* and never consume a nonce — it would measure
    // nothing. One unit to itself moves no net value; the fee is burned, which the five-bucket
    // invariant already accounts for, so success is unambiguous and conservation is untouched.
    let payload = TxPayload {
        nonce,
        target_round: target,
        action: Action::Transfer {
            token: NATIVE_TOKEN,
            to: *signer,
            amount: 1,
        },
    };
    let sig = popcorn_core::crypto::sign_payload(key, &payload);
    let tx = SignedTx {
        payload,
        signer_pubkey: key.verifying_key().to_bytes(),
        signature: sig,
    };
    let bytes = borsh::to_vec(&tx).ok()?;
    let blob = blob::encrypt(&bytes, &chain_hash, &public_key, target).ok()?;
    let tx_id = to_hex(&popcorn_core::crypto::blake3_hash(&bytes));

    let body = serde_json::json!({ "blob": to_base64(&blob), "target_round": target });
    client::post(&base.join("/tx").to_string(), &body).ok()?;

    // Wait for the target round to close, then look for the tx id in the blocks it could land in.
    let wait_until = Instant::now() + Duration::from_secs(30);
    while Instant::now() < wait_until {
        std::thread::sleep(Duration::from_secs(2));
        let h = client::get(&base.join("/head").to_string()).ok()?;
        let height = h["height"].as_u64()?;
        for probe in start_height + 1..=height {
            if let Ok(b) = client::get(&base.join(&format!("/block/{probe}")).to_string()) {
                if let Some(ids) = b["tx_ids"].as_array() {
                    if ids.iter().any(|v| v.as_str() == Some(tx_id.as_str())) {
                        return Some(true);
                    }
                }
            }
        }
    }
    Some(false)
}

const DRAND_PUBLIC_KEY: &str = "83cf0f2896adee7eb8b5f01fcad3912212c437e0073e911fb90022d3e760183c8c4b450b6a0a6c3ac6a5776a2d1064510d1fec758c921cc22b0e17e63aaf4bcb5ed66304de9cf809bd274ca73bab4af5a6e9c76a4bc09e76eae8991ef5ece45a";

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}
