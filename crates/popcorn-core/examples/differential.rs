//! Generate differential scenarios for the independent reference executor (SPEC.md §10).
//!
//! Two implementations of one specification that diverge mean a bug in the spec or a bug in
//! the code. This example builds random pre-states and random batches — deliberately
//! including transactions that must be rejected and actions that must fail — executes them
//! here, and writes everything the Python executor in `reference/` needs to do the same and
//! compare.
//!
//! Determinism is the point: a scenario is identified by its seed, so a divergence can be
//! reproduced exactly rather than described.
//!
//!     cargo run -p popcorn-core --example differential -- <count> <out.json>

use ed25519_dalek::SigningKey;
use popcorn_core::constants::{FEE_TIERS, FEE_TX, NATIVE_TOKEN};
use popcorn_core::crypto::{account_id_from_pubkey, sign_payload};
use popcorn_core::execute::{execute_batch, BatchInput};
use popcorn_core::ids::{lp_token_id, pair_id, token_id};
use popcorn_core::state::{Journal, State};
use popcorn_core::types::{AccountId, Action, ExecStatus, Htlc, Pair, SignedTx, Token, TxPayload};
use serde_json::{json, Value};

/// xorshift64*, so a seed reproduces a scenario byte for byte.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            0
        } else {
            self.next() % n
        }
    }

    fn amount(&mut self, max: u128) -> u128 {
        if max == 0 {
            return 0;
        }
        (self.next() as u128) % max
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let count: u64 = args.first().and_then(|v| v.parse().ok()).unwrap_or(50);
    let path = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "vectors/differential.json".to_string());

    let scenarios: Vec<Value> = (1..=count).map(scenario).collect();
    let document = json!({
        "note": "Differential scenarios for the reference executor (SPEC.md §10). \
                 Amounts are decimal strings: u128 does not survive JSON numbers.",
        "count": scenarios.len(),
        "scenarios": scenarios,
    });

    if let Some(parent) = std::path::Path::new(&path).parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(
        &path,
        format!("{}\n", serde_json::to_string(&document).unwrap()),
    )
    .expect("write scenarios");
    println!("wrote {count} scenarios to {path}");
}

fn scenario(seed: u64) -> Value {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);

    // A cast of actors with deterministic keys.
    // Seven actors, but only the first six get accounts: the seventh exists so that
    // UnknownAccount is reachable (§4.2 — an account is born on first receipt of funds).
    let actors: Vec<SigningKey> = (0..7)
        .map(|index| SigningKey::from_bytes(&[(seed as u8).wrapping_add(index); 32]))
        .collect();
    let ids: Vec<AccountId> = actors
        .iter()
        .map(|key| account_id_from_pubkey(&key.verifying_key().to_bytes()))
        .collect();
    let foundation = account_id_from_pubkey(&[200u8; 32]);

    // ---------------------------------------------------------------------------------
    // A random but coherent pre-state
    // ---------------------------------------------------------------------------------
    let mut state = State::new();
    let mut journal = Journal::new();
    let mut emitted: u128 = 0;

    for (index, id) in ids.iter().enumerate().take(6) {
        let balance = 1_000_000 + rng.amount(50_000_000_000);
        state
            .credit(id, &NATIVE_TOKEN, balance, &mut journal)
            .unwrap();
        emitted += balance;

        // Some accounts arrive already staked, with an accumulator ahead of them.
        if rng.below(3) == 0 {
            let stake = 1 + rng.amount(balance / 4);
            state.debit(id, &NATIVE_TOKEN, stake, &mut journal).unwrap();
            let account = state.account_mut(id, &mut journal).unwrap();
            account.staked = stake;
            state.global_mut(&mut journal).total_staked += stake;
        }
        // Materialize some keys, leave others unset, so both paths of §4.2 are exercised.
        if index % 2 == 0 {
            state.account_mut(id, &mut journal).unwrap().pubkey =
                Some(actors[index].verifying_key().to_bytes());
        }
        // Advance some nonces so contiguity has something to test.
        state.account_mut(id, &mut journal).unwrap().nonce = rng.below(3);
    }

    if state.global.total_staked > 0 {
        let staker_pot = 1 + rng.amount(5_000_000_000);
        let increment =
            popcorn_core::staking::accumulator_increment(staker_pot, state.global.total_staked)
                .unwrap();
        let global = state.global_mut(&mut journal);
        global.acc_per_stake = increment;
        global.staking_reserved = staker_pot;
        emitted += staker_pot;
    }

    // Tokens, pairs and liquidity.
    let mut tokens: Vec<[u8; 32]> = Vec::new();
    for index in 0..2u64 {
        let creator = ids[(index as usize) % ids.len()];
        let id = token_id(&creator, 900 + index);
        let supply = 1_000_000_000_000u128;
        state.insert_token(
            Token {
                id,
                creator,
                name: *b"REFTOKEN\0\0\0\0\0\0\0\0",
                total_supply: supply,
            },
            &mut journal,
        );
        for holder in ids.iter().take(6) {
            state
                .credit(holder, &id, supply / 12, &mut journal)
                .unwrap();
        }
        tokens.push(id);
    }

    let mut pairs: Vec<[u8; 32]> = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        let fee_bps = FEE_TIERS[index % FEE_TIERS.len()];
        let id = pair_id(&NATIVE_TOKEN, token, fee_bps);
        let (token0, token1) = popcorn_core::ids::sort_pair(&NATIVE_TOKEN, token);
        let reserve = 1_000_000 + rng.amount(900_000_000);
        state.insert_pair(
            Pair {
                id,
                token0,
                token1,
                fee_bps,
                reserve0: reserve,
                reserve1: reserve + rng.amount(reserve),
                lp_supply: reserve,
            },
            &mut journal,
        );
        // LP tokens have to be held by someone for RemoveLiquidity to be reachable.
        state
            .credit(&ids[0], &lp_token_id(&id), reserve / 2, &mut journal)
            .unwrap();
        pairs.push(id);
    }

    // A deliberately lopsided pool: a tiny output reserve against a deep input reserve, so a
    // one-unit swap floors to zero and ZeroOutput is reachable (§6).
    let lopsided = pair_id(&NATIVE_TOKEN, &tokens[0], 100);
    {
        let (token0, token1) = popcorn_core::ids::sort_pair(&NATIVE_TOKEN, &tokens[0]);
        let native_is_zero = token0 == NATIVE_TOKEN;
        state.insert_pair(
            Pair {
                id: lopsided,
                token0,
                token1,
                fee_bps: 100,
                reserve0: if native_is_zero { 500_000_000 } else { 900 },
                reserve1: if native_is_zero { 900 } else { 500_000_000 },
                lp_supply: 900,
            },
            &mut journal,
        );
        pairs.push(lopsided);
    }

    // Accounts that exist to make specific reject reasons reachable. A differential that
    // never walks a path proves nothing about that path.
    //
    //   poor      → FeeInsolvent (a balance that cannot cover its own fees)
    //   terminal  → NonceExhausted (u64::MAX is the end of an account's life)
    //   mismatch  → PubkeyMismatch (a materialized key that is not the signer's)
    let poor = ids[3];
    {
        // Move the balance rather than destroy it: units debited into the void would break
        // the invariant in the pre-state, and then the differential would only be testing
        // the generator.
        let balance = state.balance_of(&poor, &NATIVE_TOKEN);
        let moved = balance.saturating_sub(FEE_TX + 1);
        state
            .debit(&poor, &NATIVE_TOKEN, moved, &mut journal)
            .unwrap();
        state
            .credit(&ids[0], &NATIVE_TOKEN, moved, &mut journal)
            .unwrap();
    }
    let terminal = ids[4];
    state.account_mut(&terminal, &mut journal).unwrap().nonce = u64::MAX;
    let mismatched = ids[5];
    state.account_mut(&mismatched, &mut journal).unwrap().pubkey = Some([0xab; 32]);

    // A stranded pair: zero reserves with LP still outstanding, which is what the re-genesis
    // guard exists to refuse.
    let stranded = pair_id(&tokens[0], &tokens[1], 100);
    {
        let (token0, token1) = popcorn_core::ids::sort_pair(&tokens[0], &tokens[1]);
        state.insert_pair(
            Pair {
                id: stranded,
                token0,
                token1,
                fee_bps: 100,
                reserve0: 0,
                reserve1: 0,
                lp_supply: 10_000,
            },
            &mut journal,
        );
    }

    // An HTLC that has already expired, so claim-too-late and refund are both reachable.
    let expired_preimage = [(seed as u8).wrapping_add(99); 32];
    let expired_escrow = 1 + rng.amount(100_000);
    state
        .debit(&ids[0], &NATIVE_TOKEN, expired_escrow, &mut journal)
        .unwrap();
    let expired_htlc = Htlc {
        id: popcorn_core::ids::htlc_id(&ids[0], 778),
        sender: ids[0],
        recipient: ids[3],
        token: NATIVE_TOKEN,
        amount: expired_escrow,
        hashlock: popcorn_core::crypto::sha256(&expired_preimage),
        expiry_round: 1,
    };
    state.insert_htlc(expired_htlc.clone(), &mut journal);

    // An open HTLC, so claim, refund and auto-settlement are all reachable.
    let preimage = [seed as u8; 32];
    let hashlock = popcorn_core::crypto::sha256(&preimage);
    let escrow = 1 + rng.amount(1_000_000);
    let htlc_owner = ids[1];
    state
        .debit(&htlc_owner, &NATIVE_TOKEN, escrow, &mut journal)
        .unwrap();
    let htlc = Htlc {
        id: popcorn_core::ids::htlc_id(&htlc_owner, 777),
        sender: htlc_owner,
        recipient: ids[2],
        token: NATIVE_TOKEN,
        amount: escrow,
        hashlock,
        expiry_round: 1_000_000,
    };
    state.insert_htlc(htlc.clone(), &mut journal);

    // Native units sitting in a pool are a bucket of the monetary invariant (§5.5), so the
    // pre-state has to account for them — after every pool exists, not partway through.
    // Building scenarios that violate the invariant before a single transaction runs would
    // test nothing except the generator.
    emitted += state.total_native_in_pools();
    state.global_mut(&mut journal).native_emitted = emitted;
    assert!(
        state.monetary_invariant_holds(0),
        "the generated pre-state must satisfy the invariant"
    );

    let height = 1 + rng.below(20);
    let round = 1_000 + height;
    let pre = state_json(&state);

    // ---------------------------------------------------------------------------------
    // A random batch, including transactions that must not execute
    // ---------------------------------------------------------------------------------
    let mut txs: Vec<SignedTx> = Vec::new();
    let transaction_count = 3 + rng.below(12);
    // Every so often, one account sends far more than its per-batch budget allows, so
    // OverBudget is reachable (§5.2, step 8).
    let burst_actor = if rng.below(4) == 0 {
        Some(rng.below(6) as usize)
    } else {
        None
    };
    let transaction_count = if burst_actor.is_some() {
        transaction_count + 10
    } else {
        transaction_count
    };
    let mut burst_nonce = 0u64;
    for _ in 0..transaction_count {
        let actor_index = match burst_actor {
            Some(index) => index,
            None => rng.below(ids.len() as u64) as usize,
        };
        let key = &actors[actor_index];
        let signer = ids[actor_index];
        let current_nonce = state.account(&signer).map(|a| a.nonce).unwrap_or(0);

        // Nonce choice: usually the next one, sometimes a gap or a repeat.
        let nonce = if burst_actor.is_some() {
            // A contiguous run, so the budget is what stops it rather than a nonce gap.
            burst_nonce += 1;
            current_nonce + burst_nonce
        } else {
            match rng.below(8) {
                0 => current_nonce + 2 + rng.below(3),
                1 => current_nonce,
                _ => current_nonce + 1,
            }
        };
        // Round choice: usually right, occasionally wrong, so WrongRound is reachable.
        let target_round = if rng.below(10) == 0 { round + 1 } else { round };

        let action = random_action(
            &mut rng,
            &ids,
            &tokens,
            &pairs,
            &htlc,
            &expired_htlc,
            &preimage,
            round,
            stranded,
            &expired_preimage,
            htlc.hashlock,
            lopsided,
        );
        let payload = TxPayload {
            nonce,
            target_round,
            action,
        };
        let mut tx = SignedTx {
            signature: sign_payload(key, &payload),
            payload,
            signer_pubkey: key.verifying_key().to_bytes(),
        };
        // Occasionally corrupt the signature, so BadSignature is reachable.
        if rng.below(12) == 0 {
            tx.signature[0] ^= 0xff;
        }
        txs.push(tx);
    }

    let manifest: Vec<[u8; 32]> = txs.iter().map(|tx| tx.tx_id()).collect();
    let drand_signature: Vec<u8> = (0..48u8).map(|i| i.wrapping_add(seed as u8)).collect();

    let mut executed = state;
    let output = execute_batch(
        &mut executed,
        BatchInput {
            height,
            prev_hash: [0u8; 32],
            drand_round: round,
            drand_signature: drand_signature.clone(),
            blob_manifest: manifest.clone(),
            unusable: Vec::new(),
            txs: txs.clone(),
            foundation,
        },
    );

    json!({
        "seed": seed,
        "height": height,
        "round": round,
        "drand_signature": hex(&drand_signature),
        "foundation": hex(&foundation),
        "pre": pre,
        "txs": txs.iter().map(|tx| hex(&borsh::to_vec(tx).unwrap())).collect::<Vec<_>>(),
        "manifest": manifest.iter().map(|h| hex(h)).collect::<Vec<_>>(),
        "expected": {
            "order": output.txs.iter().map(|tx| hex(&tx.tx_id())).collect::<Vec<_>>(),
            "results": output.results.iter().map(status_name).collect::<Vec<_>>(),
            "rejected": output.rejected.iter()
                .map(|(id, reason)| json!([hex(id), format!("{reason:?}")]))
                .collect::<Vec<_>>(),
            "collection_root": hex(&output.header.collection_root),
            "txs_root": hex(&output.header.txs_root),
            "rejected_root": hex(&output.header.rejected_root),
            "results_root": hex(&output.header.results_root),
            "state_root": hex(&output.header.state_root),
            "invariant_holds": executed.monetary_invariant_holds(0),
        },
    })
}

fn status_name(status: &ExecStatus) -> String {
    match status {
        ExecStatus::Ok => "Ok".to_string(),
        ExecStatus::Failed(reason) => format!("Failed:{reason:?}"),
    }
}

#[allow(clippy::too_many_arguments)]
fn random_action(
    rng: &mut Rng,
    ids: &[AccountId],
    tokens: &[[u8; 32]],
    pairs: &[[u8; 32]],
    htlc: &Htlc,
    expired: &Htlc,
    preimage: &[u8; 32],
    round: u64,
    stranded: [u8; 32],
    expired_preimage: &[u8; 32],
    open_hashlock: [u8; 32],
    lopsided: [u8; 32],
) -> Action {
    // One draw in six is a deliberately invalid action: the failure paths are exactly the
    // part of the specification that never gets exercised by accident.
    if rng.below(6) == 0 {
        return hostile_action(
            rng,
            ids,
            tokens,
            pairs,
            expired,
            round,
            stranded,
            expired_preimage,
            open_hashlock,
            lopsided,
        );
    }
    let pick = rng.below(14);
    let recipient = ids[rng.below(ids.len() as u64) as usize];
    let token = if rng.below(2) == 0 {
        NATIVE_TOKEN
    } else {
        tokens[rng.below(tokens.len() as u64) as usize]
    };
    let pair = pairs[rng.below(pairs.len() as u64) as usize];

    match pick {
        0 | 1 => Action::Transfer {
            token,
            to: recipient,
            amount: 1 + rng.amount(5_000_000),
        },
        2 => Action::CreateToken {
            name: *b"DIFFTOKEN\0\0\0\0\0\0\0",
            supply: 1 + rng.amount(1_000_000_000),
        },
        3 => Action::CreatePair {
            token_a: NATIVE_TOKEN,
            token_b: tokens[rng.below(tokens.len() as u64) as usize],
            fee_bps: FEE_TIERS[rng.below(FEE_TIERS.len() as u64) as usize],
        },
        4 => Action::AddLiquidity {
            pair,
            amount0_desired: 1 + rng.amount(10_000_000),
            amount1_desired: 1 + rng.amount(10_000_000),
            amount0_min: 0,
            amount1_min: rng.amount(20_000_000),
        },
        5 => Action::RemoveLiquidity {
            pair,
            lp_amount: 1 + rng.amount(100_000),
            amount0_min: 0,
            amount1_min: 0,
        },
        6 | 7 => Action::SwapExactIn {
            path: vec![pair],
            token_in: NATIVE_TOKEN,
            amount_in: 1 + rng.amount(5_000_000),
            min_amount_out: rng.amount(5_000_000),
        },
        8 => Action::SwapExactOut {
            path: vec![pair],
            token_in: NATIVE_TOKEN,
            amount_out: 1 + rng.amount(1_000_000),
            max_amount_in: rng.amount(50_000_000),
        },
        9 => Action::Publish {
            topic: [7u8; 32],
            // Half the time the exact preimage, so auto-settlement is reachable.
            data: if rng.below(2) == 0 {
                preimage.to_vec()
            } else {
                vec![1u8; 1 + rng.below(300) as usize]
            },
        },
        10 => Action::HtlcLock {
            to: recipient,
            token,
            amount: 1 + rng.amount(1_000_000),
            hashlock: [rng.next() as u8; 32],
            expiry_round: round + 1 + rng.below(1_000),
        },
        11 => {
            if rng.below(2) == 0 {
                Action::HtlcClaim {
                    htlc_id: htlc.id,
                    preimage: *preimage,
                }
            } else {
                Action::HtlcRefund { htlc_id: htlc.id }
            }
        }
        12 => Action::Stake {
            amount: 1 + rng.amount(1_000_000),
        },
        _ => {
            if rng.below(2) == 0 {
                Action::Unstake {
                    amount: 1 + rng.amount(1_000_000),
                }
            } else {
                Action::ClaimRewards {}
            }
        }
    }
}

/// Actions chosen to reach the failure and rejection paths on purpose.
fn hostile_action(
    rng: &mut Rng,
    ids: &[AccountId],
    tokens: &[[u8; 32]],
    pairs: &[[u8; 32]],
    expired: &Htlc,
    round: u64,
    stranded: [u8; 32],
    expired_preimage: &[u8; 32],
    open_hashlock: [u8; 32],
    lopsided: [u8; 32],
) -> Action {
    let unknown = [0xEEu8; 32];
    match rng.below(20) {
        // Static range violations → FieldOutOfRange
        0 => Action::Transfer {
            token: NATIVE_TOKEN,
            to: ids[0],
            amount: 0,
        },
        1 => Action::Stake { amount: 0 },
        2 => Action::CreatePair {
            token_a: NATIVE_TOKEN,
            token_b: tokens[0],
            fee_bps: 7, // not a tier
        },
        3 => Action::SwapExactIn {
            path: vec![pairs[0]; 9], // longer than MAX_PATH_LEN
            token_in: NATIVE_TOKEN,
            amount_in: 1_000,
            min_amount_out: 0,
        },
        4 => Action::Publish {
            topic: [1u8; 32],
            data: vec![0u8; 600], // over MAX_PUBLISH_SIZE
        },
        5 => Action::HtlcLock {
            to: ids[0],
            token: NATIVE_TOKEN,
            amount: 1_000,
            hashlock: [1u8; 32],
            expiry_round: round, // not strictly in the future
        },
        // Runtime failures
        6 => Action::Transfer {
            token: unknown,
            to: ids[0],
            amount: 1, // a token nobody holds → InsufficientBalance
        },
        7 => Action::CreatePair {
            token_a: NATIVE_TOKEN,
            token_b: unknown, // → UnknownToken
            fee_bps: 30,
        },
        8 => Action::CreatePair {
            token_a: NATIVE_TOKEN,
            token_b: lp_token_id(&pairs[0]), // → LpTokenAsPairSide
            fee_bps: 30,
        },
        9 => Action::SwapExactIn {
            path: vec![unknown], // → UnknownPair
            token_in: NATIVE_TOKEN,
            amount_in: 1_000,
            min_amount_out: 0,
        },
        10 => Action::SwapExactIn {
            path: vec![pairs[0]],
            token_in: tokens[1], // not a side of this pair → BadPath
            amount_in: 1_000,
            min_amount_out: 0,
        },
        11 => Action::SwapExactIn {
            path: vec![lopsided], // deep in, shallow out: floors to nothing → ZeroOutput
            token_in: NATIVE_TOKEN,
            amount_in: 1,
            min_amount_out: 0,
        },
        12 => Action::AddLiquidity {
            pair: stranded, // zero reserves with LP outstanding → ReGenesisGuard
            amount0_desired: 1_000_000,
            amount1_desired: 1_000_000,
            amount0_min: 0,
            amount1_min: 0,
        },
        13 => Action::Stake {
            amount: u128::MAX / 4, // nothing would stay liquid → StakeLiquidityGuard
        },
        14 => Action::HtlcClaim {
            htlc_id: expired.id,
            preimage: [0u8; 32], // → HtlcBadPreimage
        },
        15 => Action::HtlcRefund { htlc_id: unknown }, // → HtlcNotFound
        16 => Action::HtlcClaim {
            htlc_id: expired.id,
            preimage: *expired_preimage, // right secret, too late → HtlcExpired
        },
        17 => Action::HtlcLock {
            to: ids[0],
            token: NATIVE_TOKEN,
            amount: 1_000,
            hashlock: open_hashlock, // already locked → HtlcDuplicateHashlock
            expiry_round: round + 10,
        },
        18 => Action::AddLiquidity {
            pair: pairs[0],
            amount0_desired: 0, // → LiquidityTooSmall
            amount1_desired: 1_000,
            amount0_min: 0,
            amount1_min: 0,
        },
        _ => Action::SwapExactOut {
            path: vec![pairs[0]],
            token_in: NATIVE_TOKEN,
            amount_out: u128::MAX / 8, // more than the whole reserve → SlippageExceeded
            max_amount_in: u128::MAX / 2,
        },
    }
}

/// Serialize the pre-state so the reference executor starts from exactly the same place.
fn state_json(state: &State) -> Value {
    json!({
        "accounts": state.accounts.iter().map(|(id, account)| json!({
            "id": hex(id),
            "pubkey": account.pubkey.map(|k| hex(&k)),
            "nonce": account.nonce,
            "balances": account.balances.iter()
                .map(|(token, amount)| json!([hex(token), amount.to_string()]))
                .collect::<Vec<_>>(),
            "staked": account.staked.to_string(),
            "paid_acc": account.paid_acc.to_string(),
        })).collect::<Vec<_>>(),
        "tokens": state.tokens.values().map(|token| json!({
            "id": hex(&token.id),
            "creator": hex(&token.creator),
            "name": hex(&token.name),
            "total_supply": token.total_supply.to_string(),
        })).collect::<Vec<_>>(),
        "pairs": state.pairs.values().map(|pair| json!({
            "id": hex(&pair.id),
            "token0": hex(&pair.token0),
            "token1": hex(&pair.token1),
            "fee_bps": pair.fee_bps,
            "reserve0": pair.reserve0.to_string(),
            "reserve1": pair.reserve1.to_string(),
            "lp_supply": pair.lp_supply.to_string(),
        })).collect::<Vec<_>>(),
        "htlcs": state.htlcs.values().map(|htlc| json!({
            "id": hex(&htlc.id),
            "sender": hex(&htlc.sender),
            "recipient": hex(&htlc.recipient),
            "token": hex(&htlc.token),
            "amount": htlc.amount.to_string(),
            "hashlock": hex(&htlc.hashlock),
            "expiry_round": htlc.expiry_round,
        })).collect::<Vec<_>>(),
        "global": {
            "height": state.global.height,
            "total_staked": state.global.total_staked.to_string(),
            "acc_per_stake": state.global.acc_per_stake.to_string(),
            "staking_reserved": state.global.staking_reserved.to_string(),
            "native_emitted": state.global.native_emitted.to_string(),
            "native_burned": state.global.native_burned.to_string(),
            "account_count": state.global.account_count,
        },
    })
}
