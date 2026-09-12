//! Node-level behaviour: storage, blind collection, and what the verifier catches.

use std::path::PathBuf;

use ed25519_dalek::SigningKey;
use popcorn_core::constants::{BLOB_ROUND_HORIZON, FEE_TX, MAX_BLOB_SIZE, NATIVE_TOKEN};
use popcorn_core::crypto::{account_id_from_pubkey, sign_payload, verify_signature};
use popcorn_core::genesis::GenesisConfig;
use popcorn_core::state::Journal;
use popcorn_core::types::{Action, SignedTx, TxPayload};
use popcorn_node::chain::Chain;
use popcorn_node::mempool::{Mempool, SubmitError};
use popcorn_node::storage::Storage;
use popcorn_node::verify::{audit_collection, verify_chain_assuming_beacons};

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "popcorn-test-{tag}-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn chain_file(&self) -> PathBuf {
        self.0.join("popcorn.redb")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn config(node: &SigningKey, foundation: &SigningKey) -> GenesisConfig {
    GenesisConfig {
        genesis_drand_round: 1_000,
        node_pubkey: node.verifying_key().to_bytes(),
        foundation_pubkey: foundation.verifying_key().to_bytes(),
    }
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}

fn beacon_bytes(round: u64) -> Vec<u8> {
    let mut signature = vec![0u8; 48];
    signature[..8].copy_from_slice(&round.to_le_bytes());
    signature
}

fn tx(key: &SigningKey, nonce: u64, round: u64, action: Action) -> SignedTx {
    let payload = TxPayload {
        nonce,
        target_round: round,
        action,
    };
    SignedTx {
        signature: sign_payload(key, &payload),
        payload,
        signer_pubkey: key.verifying_key().to_bytes(),
    }
}

/// Genesis allocates nothing, and the first native units appear only when block 1 closes.
#[test]
fn genesis_is_empty_and_block_one_mints_the_foundation_share() {
    let dir = TempDir::new("genesis");
    let node_key = SigningKey::from_bytes(&[1u8; 32]);
    let foundation_key = SigningKey::from_bytes(&[2u8; 32]);
    let config = config(&node_key, &foundation_key);
    let foundation = config.foundation_account();

    let mut chain = Chain::initialize(&dir.chain_file(), config).unwrap();
    assert_eq!(chain.head().height, 0);
    assert_eq!(chain.state().global.native_emitted, 0);
    assert!(chain.state().accounts.is_empty());

    chain
        .produce(
            beacon_bytes(1_000),
            vec![],
            vec![],
            vec![],
            vec![],
            &node_key,
        )
        .unwrap();

    // 15% of the first emission, and nothing else: with nothing staked the staker share is
    // never born (§7.2).
    assert_eq!(chain.state().global.native_emitted, 150_000_000);
    assert_eq!(
        chain.state().balance_of(&foundation, &NATIVE_TOKEN),
        150_000_000
    );
    assert_eq!(chain.state().global.staking_reserved, 0);
}

/// Reopening rebuilds the same state from the blocks, which are the source of truth.
#[test]
fn reopening_rebuilds_the_same_state() {
    let dir = TempDir::new("reopen");
    let node_key = SigningKey::from_bytes(&[1u8; 32]);
    let foundation_key = SigningKey::from_bytes(&[2u8; 32]);
    let config = config(&node_key, &foundation_key);

    let expected_root = {
        let mut chain = Chain::initialize(&dir.chain_file(), config.clone()).unwrap();
        for height in 1..=5u64 {
            chain
                .produce(
                    beacon_bytes(999 + height),
                    vec![],
                    vec![],
                    vec![],
                    vec![],
                    &node_key,
                )
                .unwrap();
        }
        chain.state().state_root()
    };

    let reopened = Chain::open(&dir.chain_file()).unwrap();
    assert_eq!(reopened.head().height, 5);
    assert_eq!(reopened.state().state_root(), expected_root);
    assert_eq!(reopened.head().state_root, expected_root);
    // Round mapping continues where it left off; rounds are never skipped (§3.4).
    assert_eq!(reopened.next_round(), 1_005);
}

/// A chain produced honestly verifies clean, end to end.
#[test]
fn an_honest_chain_verifies() {
    let dir = TempDir::new("verify-ok");
    let node_key = SigningKey::from_bytes(&[1u8; 32]);
    let foundation_key = SigningKey::from_bytes(&[2u8; 32]);
    let config = config(&node_key, &foundation_key);
    let foundation = config.foundation_account();

    let mut chain = Chain::initialize(&dir.chain_file(), config.clone()).unwrap();
    for height in 1..=3u64 {
        chain
            .produce(
                beacon_bytes(999 + height),
                vec![],
                vec![],
                vec![],
                vec![],
                &node_key,
            )
            .unwrap();
    }

    // The foundation now holds funds, so it can transact.
    let transfer = tx(
        &foundation_key,
        1,
        1_003,
        Action::Transfer {
            token: NATIVE_TOKEN,
            to: [9u8; 32],
            amount: 1_000,
        },
    );
    let blob_hash = [7u8; 32];
    chain
        .produce(
            beacon_bytes(1_003),
            vec![blob_hash],
            vec![],
            vec![transfer],
            vec![(blob_hash, b"not a real blob".to_vec())],
            &node_key,
        )
        .unwrap();

    let blocks = chain.storage().blocks_from(0).unwrap();
    let report = verify_chain_assuming_beacons(&config, &blocks);
    assert!(
        report.is_clean(),
        "honest chain reported divergences: {:?}",
        report.divergences
    );
    assert_eq!(report.blocks_checked, 5);
    assert_eq!(
        report.final_state.balance_of(&foundation, &NATIVE_TOKEN),
        chain.state().balance_of(&foundation, &NATIVE_TOKEN)
    );
}

/// Tampering is what the verifier exists for: each of these is a distinct lie, and each one
/// must be caught.
#[test]
fn the_verifier_catches_tampering() {
    let dir = TempDir::new("verify-bad");
    let node_key = SigningKey::from_bytes(&[1u8; 32]);
    let foundation_key = SigningKey::from_bytes(&[2u8; 32]);
    let config = config(&node_key, &foundation_key);

    let mut chain = Chain::initialize(&dir.chain_file(), config.clone()).unwrap();
    for height in 1..=3u64 {
        chain
            .produce(
                beacon_bytes(999 + height),
                vec![],
                vec![],
                vec![],
                vec![],
                &node_key,
            )
            .unwrap();
    }
    let honest = chain.storage().blocks_from(0).unwrap();
    assert!(verify_chain_assuming_beacons(&config, &honest).is_clean());

    // 1. a state root that claims more supply than the formula allows
    let mut forged = honest.clone();
    forged[2].header.state_root = [0xaa; 32];
    assert!(!verify_chain_assuming_beacons(&config, &forged).is_clean());

    // 2. a block signed by someone who is not the node
    let mut forged = honest.clone();
    let impostor = SigningKey::from_bytes(&[42u8; 32]);
    forged[2].node_signature =
        popcorn_core::crypto::sign(&impostor, &forged[2].header.block_hash());
    let report = verify_chain_assuming_beacons(&config, &forged);
    assert!(report
        .divergences
        .iter()
        .any(|d| d.what.contains("node signature")));

    // 3. a skipped drand round
    let mut forged = honest.clone();
    forged[2].header.drand_round += 5;
    let report = verify_chain_assuming_beacons(&config, &forged);
    assert!(report
        .divergences
        .iter()
        .any(|d| d.what.contains("mapping")));

    // 4. a broken header chain
    let mut forged = honest.clone();
    forged[2].header.prev_hash = [0u8; 32];
    let report = verify_chain_assuming_beacons(&config, &forged);
    assert!(report
        .divergences
        .iter()
        .any(|d| d.what.contains("prev_hash")));

    // 5. an unusable entry that is not even in the manifest
    let mut forged = honest.clone();
    forged[2].unusable = vec![[3u8; 32]];
    let report = verify_chain_assuming_beacons(&config, &forged);
    assert!(report
        .divergences
        .iter()
        .any(|d| d.what.contains("not in the manifest")));

    // 6. a beacon signature swapped out from under its own commitment
    let mut forged = honest.clone();
    forged[2].drand_signature = vec![0xff; 48];
    let report = verify_chain_assuming_beacons(&config, &forged);
    assert!(report
        .divergences
        .iter()
        .any(|d| d.what.contains("drand_sig_hash")));
}

/// A manifested blob that the node refuses to serve is visible obstruction, and a false
/// `unusable` claim is refutable by anyone holding the blob (§9.2).
#[test]
fn the_collection_audit_refutes_a_false_unusable_claim() {
    let dir = TempDir::new("audit");
    let node_key = SigningKey::from_bytes(&[1u8; 32]);
    let foundation_key = SigningKey::from_bytes(&[2u8; 32]);
    let config = config(&node_key, &foundation_key);

    let mut chain = Chain::initialize(&dir.chain_file(), config).unwrap();
    let manifested = [5u8; 32];
    chain
        .produce(
            beacon_bytes(1_000),
            vec![manifested],
            vec![manifested],
            vec![],
            vec![],
            &node_key,
        )
        .unwrap();
    let block = chain.storage().block(1).unwrap().unwrap();
    let chain_hash = [0u8; 32];

    // Withholding the blob is itself a finding: the audit cannot be performed, and saying so
    // is the honest outcome rather than passing by default.
    let withheld = audit_collection(&block, &chain_hash, &|_| None);
    assert!(withheld.iter().any(|d| d.what.contains("not served")));

    // Serving bytes that genuinely do not decrypt supports the claim.
    let garbage = audit_collection(&block, &chain_hash, &|_| Some(b"garbage".to_vec()));
    assert!(garbage.is_empty(), "{garbage:?}");
}

/// Collection is blind, deduplicated by hash, and bounded at the wire (§5.1, §11).
#[test]
fn the_mempool_collects_blindly_within_its_limits() {
    let node_key = SigningKey::from_bytes(&[1u8; 32]);
    let mempool = Mempool::new();

    // A receipt is evidence the node cannot take back.
    let receipt = mempool
        .submit(b"blob one".to_vec(), 100, 100, &node_key, 1_700_000_000_000)
        .unwrap();
    assert!(verify_signature(
        &receipt.node_pubkey,
        &receipt.payload.receipt_hash(),
        &receipt.signature
    ));
    assert_eq!(receipt.payload.target_round, 100);

    // The same blob twice is one manifest entry but two receipts (§5.1).
    let again = mempool
        .submit(b"blob one".to_vec(), 100, 100, &node_key, 1_700_000_000_001)
        .unwrap();
    assert_eq!(receipt.payload.blob_hash, again.payload.blob_hash);
    assert_ne!(receipt.signature, again.signature);
    assert_eq!(mempool.queued(100), 1);

    // Wire limits, none of which are consensus.
    assert_eq!(
        mempool.submit(vec![0u8; MAX_BLOB_SIZE + 1], 100, 100, &node_key, 0),
        Err(SubmitError::TooLarge(MAX_BLOB_SIZE + 1))
    );
    assert_eq!(
        mempool.submit(b"late".to_vec(), 99, 100, &node_key, 0),
        Err(SubmitError::RoundClosed {
            target: 99,
            current: 100
        })
    );
    assert_eq!(
        mempool.submit(
            b"far future".to_vec(),
            100 + BLOB_ROUND_HORIZON + 1,
            100,
            &node_key,
            0
        ),
        Err(SubmitError::BeyondHorizon {
            target: 100 + BLOB_ROUND_HORIZON + 1,
            current: 100
        })
    );

    // Freezing the round yields the set in lexicographic order and empties the queue.
    mempool
        .submit(b"blob two".to_vec(), 100, 100, &node_key, 0)
        .unwrap();
    let frozen = mempool.take_round(100);
    assert_eq!(frozen.len(), 2);
    assert!(frozen[0].0 < frozen[1].0);
    assert_eq!(mempool.queued(100), 0);
}

/// Blobs are stored so that `GET /blob/{hash}` can answer: without them the collection audit
/// is not practicable by third parties (§10).
#[test]
fn manifested_blobs_are_retained_for_audit() {
    let dir = TempDir::new("blobs");
    let node_key = SigningKey::from_bytes(&[1u8; 32]);
    let foundation_key = SigningKey::from_bytes(&[2u8; 32]);
    let config = config(&node_key, &foundation_key);

    let mut chain = Chain::initialize(&dir.chain_file(), config).unwrap();
    let hash = [4u8; 32];
    chain
        .produce(
            beacon_bytes(1_000),
            vec![hash],
            vec![hash],
            vec![],
            vec![(hash, b"an encrypted blob".to_vec())],
            &node_key,
        )
        .unwrap();

    let stored = chain.storage().blob(&hash).unwrap();
    assert_eq!(stored.as_deref(), Some(b"an encrypted blob".as_slice()));
}

/// A chain cannot be initialized twice over an existing one.
#[test]
fn initializing_over_an_existing_chain_is_refused() {
    let dir = TempDir::new("double-init");
    let node_key = SigningKey::from_bytes(&[1u8; 32]);
    let foundation_key = SigningKey::from_bytes(&[2u8; 32]);
    let config = config(&node_key, &foundation_key);

    Chain::initialize(&dir.chain_file(), config.clone()).unwrap();
    assert!(Chain::initialize(&dir.chain_file(), config).is_err());
}

/// Fees are burned, not collected: the node key ends up owning nothing at all.
#[test]
fn the_node_key_never_accumulates_value() {
    let dir = TempDir::new("node-key");
    let node_key = SigningKey::from_bytes(&[1u8; 32]);
    let foundation_key = SigningKey::from_bytes(&[2u8; 32]);
    let config = config(&node_key, &foundation_key);
    let node_account = account_id_from_pubkey(&node_key.verifying_key().to_bytes());

    let mut chain = Chain::initialize(&dir.chain_file(), config).unwrap();
    for height in 1..=3u64 {
        chain
            .produce(
                beacon_bytes(999 + height),
                vec![],
                vec![],
                vec![],
                vec![],
                &node_key,
            )
            .unwrap();
    }

    let transfer = tx(
        &foundation_key,
        1,
        1_003,
        Action::Transfer {
            token: NATIVE_TOKEN,
            to: [9u8; 32],
            amount: 1_000,
        },
    );
    chain
        .produce(
            beacon_bytes(1_003),
            vec![],
            vec![],
            vec![transfer],
            vec![],
            &node_key,
        )
        .unwrap();

    assert!(chain.state().account(&node_account).is_none());
    assert_eq!(chain.state().global.native_burned, FEE_TX);
}

/// Storage keeps the blocks it is given and can stream them back for replay.
#[test]
fn storage_streams_blocks_for_export() {
    let dir = TempDir::new("export");
    let node_key = SigningKey::from_bytes(&[1u8; 32]);
    let foundation_key = SigningKey::from_bytes(&[2u8; 32]);
    let config = config(&node_key, &foundation_key);

    {
        let mut chain = Chain::initialize(&dir.chain_file(), config).unwrap();
        for height in 1..=4u64 {
            chain
                .produce(
                    beacon_bytes(999 + height),
                    vec![],
                    vec![],
                    vec![],
                    vec![],
                    &node_key,
                )
                .unwrap();
        }
    }

    let storage = Storage::open(&dir.chain_file()).unwrap();
    assert_eq!(storage.head_height().unwrap(), 4);
    assert_eq!(storage.blocks_from(0).unwrap().len(), 5);
    assert_eq!(storage.blocks_from(3).unwrap().len(), 2);

    // A journal is only needed to mutate state; reads never allocate one.
    let mut journal = Journal::new();
    assert!(journal.is_empty());
    let _ = &mut journal;
}

/// A chain that funds a native pool must verify, and must survive a reopen.
///
/// This is the path the four-bucket invariant broke: §10 checks the invariant at every block,
/// so before the fifth bucket existed, an honest chain reported divergences the moment
/// somebody added native liquidity — and the verifier calls a divergence "cryptographic proof
/// of incorrectness". Driving it here keeps it from coming back.
#[test]
fn a_chain_with_native_pools_verifies_and_reopens() {
    use popcorn_core::ids::{pair_id, token_id};

    let dir = TempDir::new("native-pools");
    let node_key = SigningKey::from_bytes(&[1u8; 32]);
    let foundation_key = SigningKey::from_bytes(&[2u8; 32]);
    let config = config(&node_key, &foundation_key);
    let foundation = config.foundation_account();

    let mut chain = Chain::initialize(&dir.chain_file(), config.clone()).unwrap();

    // Let the foundation accrue enough to provide liquidity: 150M native per block.
    for height in 1..=24u64 {
        chain
            .produce(
                beacon_bytes(999 + height),
                vec![],
                vec![],
                vec![],
                vec![],
                &node_key,
            )
            .unwrap();
    }
    assert!(chain.state().balance_of(&foundation, &NATIVE_TOKEN) > 3_000_000_000);

    let token = token_id(&foundation, 1);
    let pair = pair_id(&NATIVE_TOKEN, &token, 30);

    // One action per block, because nonces must be contiguous and each needs the previous
    // one's effects.
    let actions = vec![
        Action::CreateToken {
            name: *b"POOLTEST\0\0\0\0\0\0\0\0",
            supply: 1_000_000_000_000,
        },
        Action::CreatePair {
            token_a: NATIVE_TOKEN,
            token_b: token,
            fee_bps: 30,
        },
        Action::AddLiquidity {
            pair,
            amount0_desired: 1_000_000_000,
            amount1_desired: 50_000_000_000,
            amount0_min: 0,
            amount1_min: 0,
        },
        Action::SwapExactIn {
            path: vec![pair],
            token_in: NATIVE_TOKEN,
            amount_in: 100_000_000,
            min_amount_out: 1,
        },
        Action::SwapExactOut {
            path: vec![pair],
            token_in: token,
            amount_out: 50_000_000,
            max_amount_in: u128::MAX / 2,
        },
        Action::RemoveLiquidity {
            pair,
            lp_amount: 2_000_000_000,
            amount0_min: 0,
            amount1_min: 0,
        },
    ];

    for (index, action) in actions.into_iter().enumerate() {
        let height = 25 + index as u64;
        let round = 999 + height;
        let nonce = 1 + index as u64;
        let signed = tx(&foundation_key, nonce, round, action);
        let block = chain
            .produce(
                beacon_bytes(round),
                vec![],
                vec![],
                vec![signed],
                vec![],
                &node_key,
            )
            .unwrap();
        assert_eq!(
            block.results,
            vec![popcorn_core::types::ExecStatus::Ok],
            "action {index} did not execute: {:?}",
            block.results
        );
        assert!(
            chain.state().monetary_invariant_holds(0),
            "the invariant broke after action {index}"
        );
    }

    // The pool really does hold native, or this test proves nothing.
    let in_pools = chain.state().total_native_in_pools();
    assert!(in_pools > 0, "the pool holds no native");
    assert_ne!(
        chain.state().total_native_balances() + chain.state().global.staking_reserved,
        chain.state().global.native_emitted - chain.state().global.native_burned,
        "the four-bucket sum should NOT balance here — that is the whole point"
    );

    // Full verification, which checks the invariant at every block.
    let blocks = chain.storage().blocks_from(0).unwrap();
    let report = verify_chain_assuming_beacons(&config, &blocks);
    assert!(
        report.is_clean(),
        "a chain with native pools failed verification: {:?}",
        report.divergences
    );

    // And it survives a reopen: replay rebuilds the same state, pools included.
    let expected_root = chain.state().state_root();
    drop(chain);
    let reopened = Chain::open(&dir.chain_file()).unwrap();
    assert_eq!(reopened.state().state_root(), expected_root);
    assert_eq!(reopened.state().total_native_in_pools(), in_pools);
}

/// All five buckets of §5.5 non-empty at the same time, on one chain.
///
/// Pools, stake, HTLC escrow and the staking reserve each came from a different part of the
/// protocol and each was added at a different time; this drives them together, because an
/// invariant that only holds when the buckets are exercised one at a time is not an
/// invariant. It also settles the HTLC the carrier-independent way — a `Publish` from someone
/// who is neither sender nor recipient — while staking rewards are accruing underneath.
#[test]
fn all_five_buckets_hold_together() {
    use popcorn_core::crypto::sha256;
    use popcorn_core::ids::{pair_id, token_id};
    use popcorn_core::types::ExecStatus;

    let dir = TempDir::new("five-buckets");
    let node_key = SigningKey::from_bytes(&[1u8; 32]);
    let foundation_key = SigningKey::from_bytes(&[2u8; 32]);
    let carrier_key = SigningKey::from_bytes(&[3u8; 32]);
    let config = config(&node_key, &foundation_key);
    let foundation = config.foundation_account();
    let carrier = account_id_from_pubkey(&carrier_key.verifying_key().to_bytes());
    let recipient = [0xAAu8; 32];

    let mut chain = Chain::initialize(&dir.chain_file(), config.clone()).unwrap();
    let mut height = 0u64;
    let produce = |chain: &mut Chain, height: &mut u64, txs: Vec<SignedTx>| {
        *height += 1;
        let round = 999 + *height;
        let block = chain
            .produce(beacon_bytes(round), vec![], vec![], txs, vec![], &node_key)
            .unwrap();
        assert!(
            block.results.iter().all(|result| *result == ExecStatus::Ok),
            "block {} did not execute cleanly: {:?}",
            block.header.height,
            block.results
        );
        assert!(
            chain.state().monetary_invariant_holds(0),
            "the invariant broke at height {}",
            block.header.height
        );
        block
    };

    // Emission funds the foundation, which then funds the carrier: a third party needs its
    // own balance to pay a fee, and an account is born on first receipt (§4.2).
    for _ in 0..24 {
        produce(&mut chain, &mut height, vec![]);
    }
    let mut nonce = 0u64;
    let mut next = |action: Action, height: u64| {
        nonce += 1;
        tx(&foundation_key, nonce, 999 + height, action)
    };

    let fund = next(
        Action::Transfer {
            token: NATIVE_TOKEN,
            to: carrier,
            amount: 10 * FEE_TX,
        },
        height + 1,
    );
    produce(&mut chain, &mut height, vec![fund]);

    // Bucket 4: a pool holding native.
    let token = token_id(&foundation, 2);
    let pair = pair_id(&NATIVE_TOKEN, &token, 30);
    let create_token = next(
        Action::CreateToken {
            name: *b"FIVEBUCKET\0\0\0\0\0\0",
            supply: 1_000_000_000_000,
        },
        height + 1,
    );
    produce(&mut chain, &mut height, vec![create_token]);
    let create_pair = next(
        Action::CreatePair {
            token_a: NATIVE_TOKEN,
            token_b: token,
            fee_bps: 30,
        },
        height + 1,
    );
    produce(&mut chain, &mut height, vec![create_pair]);
    let add = next(
        Action::AddLiquidity {
            pair,
            amount0_desired: 500_000_000,
            amount1_desired: 20_000_000_000,
            amount0_min: 0,
            amount1_min: 0,
        },
        height + 1,
    );
    produce(&mut chain, &mut height, vec![add]);

    // Bucket 2: stake. From the next block's close the staker share starts accruing, which
    // fills bucket 5.
    let stake = next(
        Action::Stake {
            amount: 400_000_000,
        },
        height + 1,
    );
    produce(&mut chain, &mut height, vec![stake]);
    produce(&mut chain, &mut height, vec![]);

    // Bucket 3: an HTLC escrow, locked to someone who will never send anything.
    let preimage = [0x42u8; 32];
    let lock = next(
        Action::HtlcLock {
            to: recipient,
            token: NATIVE_TOKEN,
            amount: 250_000_000,
            hashlock: sha256(&preimage),
            expiry_round: 999 + height + 500,
        },
        height + 1,
    );
    produce(&mut chain, &mut height, vec![lock]);

    // Every bucket is now occupied at once.
    let state = chain.state();
    let liquid = state.total_native_balances();
    let staked = state.total_staked_sum();
    let escrow = state.total_native_in_htlcs();
    let pools = state.total_native_in_pools();
    let reserved = state.global.staking_reserved;
    for (name, value) in [
        ("liquid", liquid),
        ("staked", staked),
        ("escrow", escrow),
        ("pools", pools),
        ("reserve", reserved),
    ] {
        assert!(
            value > 0,
            "bucket `{name}` is empty; this test proves nothing"
        );
    }
    assert_eq!(
        liquid + staked + escrow + pools + reserved,
        state.global.native_emitted - state.global.native_burned,
        "five buckets must account for every native unit"
    );

    // Carrier-independent settlement: a third party publishes the 32-byte secret, and the
    // recipient — who has done nothing at all — is paid (§7.6).
    assert_eq!(chain.state().balance_of(&recipient, &NATIVE_TOKEN), 0);
    let publish = tx(
        &carrier_key,
        1,
        999 + height + 1,
        Action::Publish {
            topic: [7u8; 32],
            data: preimage.to_vec(),
        },
    );
    produce(&mut chain, &mut height, vec![publish]);
    assert_eq!(
        chain.state().balance_of(&recipient, &NATIVE_TOKEN),
        250_000_000,
        "the published preimage did not settle the lock"
    );
    assert!(chain.state().htlcs.is_empty());

    // Drain the staking buckets and check the reserve covered every claim.
    let claim = next(Action::ClaimRewards {}, height + 1);
    produce(&mut chain, &mut height, vec![claim]);
    let unstake = next(
        Action::Unstake {
            amount: 400_000_000,
        },
        height + 1,
    );
    produce(&mut chain, &mut height, vec![unstake]);

    assert_eq!(chain.state().global.total_staked, 0);
    assert_eq!(chain.state().total_native_in_htlcs(), 0);
    assert!(
        chain.state().total_native_in_pools() > 0,
        "the pool is still funded"
    );
    assert!(
        chain.state().global.staking_reserved >= chain.state().total_pending(),
        "the reserve must still cover every outstanding claim"
    );

    // And the whole chain verifies, invariant checked at every block.
    let blocks = chain.storage().blocks_from(0).unwrap();
    let report = verify_chain_assuming_beacons(&config, &blocks);
    assert!(
        report.is_clean(),
        "verification failed: {:?}",
        report.divergences
    );
}

/// A chain created under different consensus rules is refused, not extended.
///
/// The binary cannot fake being a different version of itself, so the test builds the chain
/// the other way round: a genesis whose stamped identity is not this binary's. Opening it
/// must fail — continuing would mean producing blocks nobody replaying under the stamped
/// rules could reproduce, and leaving the discovery to whoever verifies later.
#[test]
fn a_chain_stamped_with_foreign_rules_is_refused() {
    use popcorn_core::genesis::{genesis_block, genesis_state};
    use popcorn_node::storage::StorageError;

    let dir = TempDir::new("foreign");
    let node = SigningKey::from_bytes(&[1u8; 32]);
    let foundation = SigningKey::from_bytes(&[2u8; 32]);

    // Genesis as some other implementation would write it: same everything, different rules.
    let mut state = genesis_state();
    state.global.consensus_version = popcorn_core::constants::CONSENSUS_VERSION + 1;
    let block = genesis_block(&state, [0u8; 64]);

    let storage = Storage::open(&dir.chain_file()).unwrap();
    storage
        .initialize(&config(&node, &foundation), &block)
        .unwrap();
    drop(storage);

    // The stored metadata says this binary's version — `initialize` writes it from the
    // constants — so what catches this is the commitment: block 0's state root is not the
    // one this binary computes for an empty chain.
    match Chain::open(&dir.chain_file()) {
        Err(StorageError::Database(message)) => {
            assert!(
                message.contains("different rules"),
                "unexpected message: {message}"
            );
        }
        Err(other) => panic!("wrong error: {other}"),
        Ok(_) => panic!("a chain stamped with foreign rules was opened"),
    }
}

/// An honest chain records its identity and opens cleanly.
#[test]
fn an_honest_chain_records_its_consensus_identity() {
    let dir = TempDir::new("identity");
    let node = SigningKey::from_bytes(&[1u8; 32]);
    let foundation = SigningKey::from_bytes(&[2u8; 32]);

    let chain = Chain::initialize(&dir.chain_file(), config(&node, &foundation)).unwrap();
    assert_eq!(
        chain.state().global.consensus_version,
        popcorn_core::constants::CONSENSUS_VERSION
    );
    assert_eq!(
        chain.state().global.lock_digest,
        popcorn_core::crypto::consensus_lock_digest()
    );
    drop(chain);

    let reopened = Chain::open(&dir.chain_file()).unwrap();
    assert_eq!(
        reopened.state().global.consensus_version,
        popcorn_core::constants::CONSENSUS_VERSION,
        "the identity survives a reopen: it is state, not a constant read at startup"
    );
}

/// Test A (audit C-01): the verifier authenticates the beacon, not just its hash.
///
/// A block can carry a valid node signature and a valid `drand_sig_hash` over a beacon the
/// operator invented — the commitment check cannot tell the difference, and inventing the
/// beacon means choosing the shuffle seed. The strict verifier must reject it; the
/// replay-only path, which does not authenticate beacons, must still accept it (that is the
/// difference the split encodes).
#[test]
fn the_verifier_authenticates_the_beacon() {
    use popcorn_node::verify::verify_chain;

    // The real drand quicknet signature for round 1000, and genesis mapped so block 1 is
    // exactly that round.
    let real_1000 = unhex(
        "b44679b9a59af2ec876b1a6b1ad52ea9b1615fc3982b19576350f93447cb1125e342b73a8dd2bacbe47e4b6b63ed5e39",
    );
    let node_key = SigningKey::from_bytes(&[1u8; 32]);
    let foundation_key = SigningKey::from_bytes(&[2u8; 32]);
    let config = config(&node_key, &foundation_key);
    assert_eq!(config.genesis_drand_round, 1_000);

    // Honest: block 1 carries the genuine round-1000 beacon → the strict verifier is clean.
    let honest_dir = TempDir::new("beacon-honest");
    let mut honest = Chain::initialize(&honest_dir.chain_file(), config.clone()).unwrap();
    honest
        .produce(real_1000.clone(), vec![], vec![], vec![], vec![], &node_key)
        .unwrap();
    let honest_blocks = honest.storage().blocks_from(0).unwrap();
    assert!(
        verify_chain(&config, &honest_blocks).is_clean(),
        "a chain with the real beacon must verify strictly"
    );

    // Forged: block 1 carries an invented beacon. `drand_sig_hash` matches it (produce derives
    // it), and the node signature is valid — only the BLS check can catch this.
    let forged_dir = TempDir::new("beacon-forged");
    let mut forged = Chain::initialize(&forged_dir.chain_file(), config.clone()).unwrap();
    forged
        .produce(vec![0xab; 48], vec![], vec![], vec![], vec![], &node_key)
        .unwrap();
    let forged_blocks = forged.storage().blocks_from(0).unwrap();

    let strict = verify_chain(&config, &forged_blocks);
    assert!(
        !strict.is_clean(),
        "the strict verifier must reject a forged beacon"
    );
    assert!(
        strict
            .divergences
            .iter()
            .any(|d| d.what.contains("does not verify")),
        "the divergence must name the beacon: {:?}",
        strict.divergences
    );

    // And the difference is exactly the beacon authenticity: replay-only accepts it.
    assert!(
        verify_chain_assuming_beacons(&config, &forged_blocks).is_clean(),
        "replay-only verification does not authenticate beacons, so it accepts this chain"
    );
}
