//! Batch execution (SPEC.md §5.1, §5.2, §7.6) — the pipeline that turns a decrypted
//! collection into a block and a new state root.
//!
//! The phase order is normative and every phase boundary in it exists because of a concrete
//! failure it prevents:
//!
//! * fees are burned in a single phase **before** any action runs, so no transaction can
//!   spend the units a later transaction needs for its own fee;
//! * HTLC auto-settlement runs **after** execution, so a claim in the same batch wins and
//!   the publish carrying the same preimage is a deterministic no-op;
//! * emission is applied **last**, so nothing minted in a batch is spendable within it.

use std::collections::BTreeMap;

use crate::amm;
use crate::constants::{FEE_TX, MINIMUM_LIQUIDITY, NATIVE_TOKEN};
use crate::crypto::sha256;
use crate::emission;
use crate::fees::tx_fee;
use crate::ids::{htlc_id, lp_token_id, pair_id, sort_pair};
use crate::shuffle::{normalize_nonces, shuffle, BeaconRng};
use crate::staking::{accumulator_increment, pending, settle_amount};
use crate::state::{Journal, State};
use crate::types::{
    Account, AccountId, Action, Amount, Block, ExecStatus, FailReason, Header, Htlc, Pair,
    RejectReason, SignedTx, Token, TokenId,
};
use crate::validate::validate_batch;

/// Everything a batch needs that does not come from state.
pub struct BatchInput {
    pub height: u64,
    pub prev_hash: [u8; 32],
    pub drand_round: u64,
    /// Raw BLS signature of the round's beacon: it seeds the shuffle and is committed to.
    pub drand_signature: Vec<u8>,
    /// blake3 of every blob received for this round. A set: duplicates collapse.
    pub blob_manifest: Vec<[u8; 32]>,
    /// Manifest entries that yielded no decodable transaction (§5.1).
    pub unusable: Vec<[u8; 32]>,
    /// Transactions decoded from the collection, unvalidated and unordered.
    pub txs: Vec<SignedTx>,
    /// The account receiving the foundation share of emission.
    pub foundation: AccountId,
}

/// The block produced by a batch, minus the node signature.
pub struct BatchOutput {
    pub header: Header,
    pub txs: Vec<SignedTx>,
    pub results: Vec<ExecStatus>,
    pub rejected: Vec<([u8; 32], RejectReason)>,
    pub blob_manifest: Vec<[u8; 32]>,
    pub unusable: Vec<[u8; 32]>,
    pub drand_signature: Vec<u8>,
}

impl BatchOutput {
    /// Assemble the signed block. The signature covers the header, which binds every root.
    pub fn into_block(self, node_signature: [u8; 64]) -> Block {
        Block {
            header: self.header,
            drand_signature: self.drand_signature,
            blob_manifest: self.blob_manifest,
            unusable: self.unusable,
            txs: self.txs,
            results: self.results,
            rejected: self.rejected,
            node_signature,
        }
    }
}

/// Run a full batch against `state`, mutating it into the post-batch state.
pub fn execute_batch(state: &mut State, input: BatchInput) -> BatchOutput {
    let round = input.drand_round;

    // The manifest commits to the SET of distinct blobs received, in lexicographic order
    // (§5.1): the same blob submitted ten times is one entry. Normalizing here rather than
    // trusting the caller is what makes a multiset manifest unrepresentable instead of
    // merely forbidden.
    let mut blob_manifest = input.blob_manifest;
    blob_manifest.sort_unstable();
    blob_manifest.dedup();
    let mut unusable = input.unusable;
    unusable.sort_unstable();
    unusable.dedup();

    // 5. static validation
    let outcome = validate_batch(state, input.txs, round);
    let mut ordered = outcome.valid;

    // 6. ordering: shuffle by the beacon, then per-account nonce normalization
    let mut rng = BeaconRng::new(&input.drand_signature, input.height);
    shuffle(&mut ordered, &mut rng);
    normalize_nonces(&mut ordered);

    // 7. fee collection, single phase. Static solvency guarantees this cannot fail.
    for tx in &ordered {
        let mut journal = Journal::new();
        let fee = tx_fee(tx);
        state
            .burn_native(&tx.signer(), fee, &mut journal)
            .expect("fee solvency was established over the pre-batch balance (§5.2 step 9)");
    }

    // 8. sequential execution
    let mut results: Vec<ExecStatus> = Vec::with_capacity(ordered.len());
    for tx in &ordered {
        let signer = tx.signer();
        let mut journal = Journal::new();
        let status = match execute_action(state, &signer, tx, round, &mut journal) {
            Ok(()) => ExecStatus::Ok,
            Err(reason) => {
                state.rollback(journal);
                ExecStatus::Failed(reason)
            }
        };

        // The nonce is consumed and the pubkey materializes on every EXECUTED transaction,
        // Ok or Failed alike, and neither is subject to the rollback above (§4.2, §5.2).
        let mut bookkeeping = Journal::new();
        let account = state
            .account_mut(&signer, &mut bookkeeping)
            .expect("signer exists: static validation checked it");
        account.nonce = tx.payload.nonce;
        if account.pubkey.is_none() {
            account.pubkey = Some(tx.signer_pubkey);
        }

        results.push(status);
    }

    // 9a. HTLC auto-settlement via Publish (§7.6)
    auto_settle_htlcs(state, &ordered, &results, round);

    // 9b. emission (§7.2)
    apply_emission(state, input.height, &input.foundation);

    // 9c. close
    let mut journal = Journal::new();
    state.global_mut(&mut journal).height = input.height;

    let header = Header {
        height: input.height,
        prev_hash: input.prev_hash,
        drand_round: round,
        drand_sig_hash: crate::crypto::blake3_hash(&input.drand_signature),
        collection_root: collection_root(&blob_manifest),
        txs_root: txs_root(&ordered),
        rejected_root: rejected_root(&outcome.rejected),
        results_root: results_root(&results),
        state_root: state.state_root(),
    };

    BatchOutput {
        header,
        txs: ordered,
        results,
        rejected: outcome.rejected,
        blob_manifest,
        unusable,
        drand_signature: input.drand_signature,
    }
}

// ---------------------------------------------------------------------------------------
// Roots
// ---------------------------------------------------------------------------------------

/// `collection_root` over the sorted manifest. Empty list → blake3 of the empty input.
pub fn collection_root(manifest: &[[u8; 32]]) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    for entry in manifest {
        h.update(entry);
    }
    *h.finalize().as_bytes()
}

/// `txs_root` over transaction ids in execution order.
pub fn txs_root(txs: &[SignedTx]) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    for tx in txs {
        h.update(&tx.tx_id());
    }
    *h.finalize().as_bytes()
}

/// `rejected_root` over `(tx_id ‖ borsh(reason))` pairs, sorted by tx_id.
///
/// The reason is committed to as well: a node cannot restate why it dropped a transaction
/// after the fact.
pub fn rejected_root(rejected: &[([u8; 32], RejectReason)]) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    for (tx_id, reason) in rejected {
        h.update(tx_id);
        let encoded = borsh::to_vec(reason).expect("borsh serialization of a RejectReason");
        h.update(&encoded);
    }
    *h.finalize().as_bytes()
}

/// `results_root = blake3(borsh(Vec<ExecStatus>))`.
///
/// Note this one hashes the Borsh encoding of the whole vector, length prefix included —
/// unlike the other roots, which concatenate fixed-size elements.
pub fn results_root(results: &[ExecStatus]) -> [u8; 32] {
    let encoded = borsh::to_vec(results).expect("borsh serialization of results");
    crate::crypto::blake3_hash(&encoded)
}

// ---------------------------------------------------------------------------------------
// Close-of-batch phases
// ---------------------------------------------------------------------------------------

/// Scan `Ok` publishes in execution order and settle any HTLC whose hashlock they open.
///
/// The preimage is carrier-independent: whoever publishes it settles the lock, the recipient
/// may be offline, and censoring settlement would mean discarding blobs blindly and en masse.
fn auto_settle_htlcs(state: &mut State, txs: &[SignedTx], results: &[ExecStatus], round: u64) {
    // Derived index, rebuilt here and never committed (§7.6, §14.6).
    let mut index: BTreeMap<[u8; 32], [u8; 32]> = state.hashlock_index();

    for (tx, status) in txs.iter().zip(results) {
        if *status != ExecStatus::Ok {
            continue;
        }
        let Action::Publish { data, .. } = &tx.payload.action else {
            continue;
        };
        if data.len() != 32 {
            continue;
        }
        let hash = sha256(data);
        let Some(id) = index.get(&hash).copied() else {
            continue;
        };
        let Some(htlc) = state.htlcs.get(&id).cloned() else {
            index.remove(&hash);
            continue;
        };
        if htlc.expiry_round < round {
            continue;
        }

        let mut journal = Journal::new();
        state.remove_htlc(&id, &mut journal);
        if state
            .credit(&htlc.recipient, &htlc.token, htlc.amount, &mut journal)
            .is_err()
        {
            // Unreachable: the escrowed amount was debited from a balance that held it.
            state.rollback(journal);
            continue;
        }
        index.remove(&hash);
    }
}

/// Apply the batch's emission (§7.2).
///
/// With nothing staked, the staker share is not born at all — not credited elsewhere, not
/// burned. That is the fair-launch bootstrap: the foundation still receives only its nominal
/// 15%, which happens to be 100% of that batch's effective emission.
fn apply_emission(state: &mut State, height: u64, foundation: &AccountId) {
    let nominal = emission::emission_at(height);
    if nominal == 0 {
        return;
    }
    let (staker_share, foundation_share) = emission::split(nominal);
    let total_staked = state.global.total_staked;

    let mut journal = Journal::new();
    if total_staked == 0 {
        let global = state.global_mut(&mut journal);
        global.native_emitted += foundation_share;
    } else {
        let increment = accumulator_increment(staker_share, total_staked)
            .expect("total_staked is non-zero in this branch");
        let global = state.global_mut(&mut journal);
        global.acc_per_stake += increment;
        global.staking_reserved += staker_share;
        global.native_emitted += nominal;
    }

    state
        .credit(foundation, &NATIVE_TOKEN, foundation_share, &mut journal)
        .expect("foundation credit is bounded by finite supply");
}

// ---------------------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------------------

fn execute_action(
    state: &mut State,
    signer: &AccountId,
    tx: &SignedTx,
    round: u64,
    journal: &mut Journal,
) -> Result<(), FailReason> {
    match &tx.payload.action {
        Action::Transfer { token, to, amount } => {
            if to == signer {
                // A self-transfer would pay a fee for an accounting-ambiguous no-op.
                return Err(FailReason::SelfTransferNoop);
            }
            state
                .debit(signer, token, *amount, journal)
                .map_err(|_| FailReason::InsufficientBalance)?;
            state
                .credit(to, token, *amount, journal)
                .map_err(|_| FailReason::Overflow)
        }

        Action::CreateToken { name, supply } => {
            let id = crate::ids::token_id(signer, tx.payload.nonce);
            if state.tokens.contains_key(&id) {
                // Unreachable: a nonce executes at most once per account (§14.3).
                debug_assert!(false, "token id collision");
                return Err(FailReason::Overflow);
            }
            state.insert_token(
                Token {
                    id,
                    creator: *signer,
                    name: *name,
                    total_supply: *supply,
                },
                journal,
            );
            state
                .credit(signer, &id, *supply, journal)
                .map_err(|_| FailReason::Overflow)
        }

        Action::CreatePair {
            token_a,
            token_b,
            fee_bps,
        } => execute_create_pair(state, token_a, token_b, *fee_bps, journal),

        Action::AddLiquidity {
            pair,
            amount0_desired,
            amount1_desired,
            amount0_min,
            amount1_min,
        } => execute_add_liquidity(
            state,
            signer,
            pair,
            *amount0_desired,
            *amount1_desired,
            *amount0_min,
            *amount1_min,
            journal,
        ),

        Action::RemoveLiquidity {
            pair,
            lp_amount,
            amount0_min,
            amount1_min,
        } => execute_remove_liquidity(
            state,
            signer,
            pair,
            *lp_amount,
            *amount0_min,
            *amount1_min,
            journal,
        ),

        Action::SwapExactIn {
            path,
            token_in,
            amount_in,
            min_amount_out,
        } => execute_swap_exact_in(
            state,
            signer,
            path,
            token_in,
            *amount_in,
            *min_amount_out,
            journal,
        ),

        Action::SwapExactOut {
            path,
            token_in,
            amount_out,
            max_amount_in,
        } => execute_swap_exact_out(
            state,
            signer,
            path,
            token_in,
            *amount_out,
            *max_amount_in,
            journal,
        ),

        // The data lives in the block and nowhere else: state does not grow by a byte.
        Action::Publish { .. } => Ok(()),

        Action::HtlcLock {
            to,
            token,
            amount,
            hashlock,
            expiry_round,
        } => {
            if state.htlcs.values().any(|h| h.hashlock == *hashlock) {
                // One hashlock, one HTLC: the index must stay injective for settlement to
                // be unambiguous (§7.6).
                return Err(FailReason::HtlcDuplicateHashlock);
            }
            let id = htlc_id(signer, tx.payload.nonce);
            if state.htlcs.contains_key(&id) {
                debug_assert!(false, "htlc id collision");
                return Err(FailReason::Overflow);
            }
            state
                .debit(signer, token, *amount, journal)
                .map_err(|_| FailReason::InsufficientBalance)?;
            state.insert_htlc(
                Htlc {
                    id,
                    sender: *signer,
                    recipient: *to,
                    token: *token,
                    amount: *amount,
                    hashlock: *hashlock,
                    expiry_round: *expiry_round,
                },
                journal,
            );
            Ok(())
        }

        Action::HtlcClaim { htlc_id, preimage } => {
            let htlc = state
                .htlcs
                .get(htlc_id)
                .cloned()
                .ok_or(FailReason::HtlcNotFound)?;
            if sha256(preimage) != htlc.hashlock {
                return Err(FailReason::HtlcBadPreimage);
            }
            if round > htlc.expiry_round {
                return Err(FailReason::HtlcExpired);
            }
            state.remove_htlc(htlc_id, journal);
            state
                .credit(&htlc.recipient, &htlc.token, htlc.amount, journal)
                .map_err(|_| FailReason::Overflow)
        }

        Action::HtlcRefund { htlc_id } => {
            let htlc = state
                .htlcs
                .get(htlc_id)
                .cloned()
                .ok_or(FailReason::HtlcNotFound)?;
            if round <= htlc.expiry_round {
                return Err(FailReason::HtlcNotExpired);
            }
            state.remove_htlc(htlc_id, journal);
            state
                .credit(&htlc.sender, &htlc.token, htlc.amount, journal)
                .map_err(|_| FailReason::Overflow)
        }

        Action::Stake { amount } => execute_stake(state, signer, *amount, journal),
        Action::Unstake { amount } => execute_unstake(state, signer, *amount, journal),
        Action::ClaimRewards {} => {
            settle_rewards(state, signer, journal)?;
            Ok(())
        }
    }
}

fn execute_create_pair(
    state: &mut State,
    token_a: &TokenId,
    token_b: &TokenId,
    fee_bps: u16,
    journal: &mut Journal,
) -> Result<(), FailReason> {
    if token_a == token_b {
        return Err(FailReason::BadPath);
    }
    for token in [token_a, token_b] {
        // The LP check comes first, and the order matters: an LP token has no `Token`
        // record, so checking existence first would report UnknownToken for every LP side
        // and leave LpTokenAsPairSide unreachable — a dead discriminant in a committed enum
        // (§14.7). LP tokens are only recognizable by deriving them from an existing pair,
        // hence the scan.
        if state.pairs.keys().any(|p| lp_token_id(p) == *token) {
            return Err(FailReason::LpTokenAsPairSide);
        }
        if *token != NATIVE_TOKEN && !state.tokens.contains_key(token) {
            return Err(FailReason::UnknownToken);
        }
    }

    let id = pair_id(token_a, token_b, fee_bps);
    if state.pairs.contains_key(&id) {
        return Err(FailReason::PairAlreadyExists);
    }
    let (token0, token1) = sort_pair(token_a, token_b);
    state.insert_pair(
        Pair {
            id,
            token0,
            token1,
            fee_bps,
            reserve0: 0,
            reserve1: 0,
            lp_supply: 0,
        },
        journal,
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn execute_add_liquidity(
    state: &mut State,
    signer: &AccountId,
    pair_key: &[u8; 32],
    amount0_desired: Amount,
    amount1_desired: Amount,
    amount0_min: Amount,
    amount1_min: Amount,
    journal: &mut Journal,
) -> Result<(), FailReason> {
    let pair = state
        .pairs
        .get(pair_key)
        .cloned()
        .ok_or(FailReason::UnknownPair)?;
    if amount0_desired == 0 || amount1_desired == 0 {
        return Err(FailReason::LiquidityTooSmall);
    }

    let empty = pair.reserve0 == 0 && pair.reserve1 == 0;
    let (actual0, actual1, minted, new_lp_supply) = if empty {
        // Genesis and re-genesis share the formula; what separates them is how much LP is
        // already outstanding against zero reserves.
        if pair.lp_supply != 0 && pair.lp_supply != MINIMUM_LIQUIDITY {
            return Err(FailReason::ReGenesisGuard);
        }
        let minted = amm::initial_liquidity(amount0_desired, amount1_desired, MINIMUM_LIQUIDITY)?;
        let new_supply = if pair.lp_supply == 0 {
            minted + MINIMUM_LIQUIDITY
        } else {
            pair.lp_supply + minted
        };
        (amount0_desired, amount1_desired, minted, new_supply)
    } else {
        if pair.reserve0 == 0 || pair.reserve1 == 0 {
            // A half-empty pool has no defined price: it must be drained and restarted.
            return Err(FailReason::ReGenesisGuard);
        }
        let (actual0, actual1) = amm::actual_deposit(
            amount0_desired,
            amount1_desired,
            pair.reserve0,
            pair.reserve1,
        )?;
        let minted = amm::subsequent_liquidity(
            actual0,
            actual1,
            pair.reserve0,
            pair.reserve1,
            pair.lp_supply,
        )?;
        (actual0, actual1, minted, pair.lp_supply + minted)
    };

    if actual0 < amount0_min || actual1 < amount1_min {
        return Err(FailReason::SlippageExceeded);
    }

    // Only the actual amounts are ever debited: the excess a depositor offered is untouched.
    state
        .debit(signer, &pair.token0, actual0, journal)
        .map_err(|_| FailReason::InsufficientBalance)?;
    state
        .debit(signer, &pair.token1, actual1, journal)
        .map_err(|_| FailReason::InsufficientBalance)?;

    let lp_token = lp_token_id(&pair.id);
    state
        .credit(signer, &lp_token, minted, journal)
        .map_err(|_| FailReason::Overflow)?;

    let pair_mut = state.pair_mut(pair_key, journal).expect("pair exists");
    pair_mut.reserve0 += actual0;
    pair_mut.reserve1 += actual1;
    pair_mut.lp_supply = new_lp_supply;
    Ok(())
}

fn execute_remove_liquidity(
    state: &mut State,
    signer: &AccountId,
    pair_key: &[u8; 32],
    lp_amount: Amount,
    amount0_min: Amount,
    amount1_min: Amount,
    journal: &mut Journal,
) -> Result<(), FailReason> {
    let pair = state
        .pairs
        .get(pair_key)
        .cloned()
        .ok_or(FailReason::UnknownPair)?;
    if lp_amount == 0 {
        return Err(FailReason::LiquidityTooSmall);
    }

    let amount0 = amm::withdrawal_amount(lp_amount, pair.reserve0, pair.lp_supply)?;
    let amount1 = amm::withdrawal_amount(lp_amount, pair.reserve1, pair.lp_supply)?;
    if amount0 < amount0_min || amount1 < amount1_min {
        return Err(FailReason::SlippageExceeded);
    }

    let lp_token = lp_token_id(&pair.id);
    state
        .debit(signer, &lp_token, lp_amount, journal)
        .map_err(|_| FailReason::InsufficientBalance)?;
    state
        .credit(signer, &pair.token0, amount0, journal)
        .map_err(|_| FailReason::Overflow)?;
    state
        .credit(signer, &pair.token1, amount1, journal)
        .map_err(|_| FailReason::Overflow)?;

    let pair_mut = state.pair_mut(pair_key, journal).expect("pair exists");
    pair_mut.reserve0 -= amount0;
    pair_mut.reserve1 -= amount1;
    pair_mut.lp_supply -= lp_amount;
    Ok(())
}

/// Resolve a path into `(pair_key, token_in, token_out)` hops, following the token as it
/// moves. A pair that does not contain the current token breaks the path.
fn resolve_path(
    state: &State,
    path: &[[u8; 32]],
    token_in: &TokenId,
) -> Result<Vec<([u8; 32], TokenId, TokenId)>, FailReason> {
    let mut hops = Vec::with_capacity(path.len());
    let mut current = *token_in;
    for pair_key in path {
        let pair = state.pairs.get(pair_key).ok_or(FailReason::UnknownPair)?;
        let next = if pair.token0 == current {
            pair.token1
        } else if pair.token1 == current {
            pair.token0
        } else {
            return Err(FailReason::BadPath);
        };
        hops.push((*pair_key, current, next));
        current = next;
    }
    Ok(hops)
}

/// Apply one hop's reserve movement, in the pair's own orientation.
fn apply_hop(
    state: &mut State,
    pair_key: &[u8; 32],
    token_in: &TokenId,
    amount_in: Amount,
    amount_out: Amount,
    journal: &mut Journal,
) -> Result<(), FailReason> {
    let pair = state
        .pair_mut(pair_key, journal)
        .ok_or(FailReason::UnknownPair)?;
    if pair.token0 == *token_in {
        pair.reserve0 = pair
            .reserve0
            .checked_add(amount_in)
            .ok_or(FailReason::Overflow)?;
        pair.reserve1 = pair
            .reserve1
            .checked_sub(amount_out)
            .ok_or(FailReason::Overflow)?;
    } else {
        pair.reserve1 = pair
            .reserve1
            .checked_add(amount_in)
            .ok_or(FailReason::Overflow)?;
        pair.reserve0 = pair
            .reserve0
            .checked_sub(amount_out)
            .ok_or(FailReason::Overflow)?;
    }
    Ok(())
}

/// Reserves of a hop, oriented as (in, out).
fn oriented_reserves(pair: &Pair, token_in: &TokenId) -> (Amount, Amount) {
    if pair.token0 == *token_in {
        (pair.reserve0, pair.reserve1)
    } else {
        (pair.reserve1, pair.reserve0)
    }
}

fn execute_swap_exact_in(
    state: &mut State,
    signer: &AccountId,
    path: &[[u8; 32]],
    token_in: &TokenId,
    amount_in: Amount,
    min_amount_out: Amount,
    journal: &mut Journal,
) -> Result<(), FailReason> {
    if amount_in == 0 {
        return Err(FailReason::ZeroOutput);
    }
    let hops = resolve_path(state, path, token_in)?;

    state
        .debit(signer, token_in, amount_in, journal)
        .map_err(|_| FailReason::InsufficientBalance)?;

    let mut current_amount = amount_in;
    let mut current_token = *token_in;
    for (pair_key, hop_in, hop_out) in &hops {
        let pair = state.pairs.get(pair_key).ok_or(FailReason::UnknownPair)?;
        let (reserve_in, reserve_out) = oriented_reserves(pair, hop_in);
        let out = amm::amount_out_exact_in(current_amount, reserve_in, reserve_out, pair.fee_bps)?;
        if out == 0 {
            // No zero-yield swaps: they would only pay a fee to move dust.
            return Err(FailReason::ZeroOutput);
        }
        apply_hop(state, pair_key, hop_in, current_amount, out, journal)?;
        current_amount = out;
        current_token = *hop_out;
    }

    if current_amount < min_amount_out {
        return Err(FailReason::SlippageExceeded);
    }
    state
        .credit(signer, &current_token, current_amount, journal)
        .map_err(|_| FailReason::Overflow)
}

fn execute_swap_exact_out(
    state: &mut State,
    signer: &AccountId,
    path: &[[u8; 32]],
    token_in: &TokenId,
    amount_out: Amount,
    max_amount_in: Amount,
    journal: &mut Journal,
) -> Result<(), FailReason> {
    if amount_out == 0 {
        return Err(FailReason::ZeroOutput);
    }
    let hops = resolve_path(state, path, token_in)?;

    // Backward pass: required input per hop, last hop first.
    let mut required = vec![0u128; hops.len()];
    let mut needed = amount_out;
    for (index, (pair_key, hop_in, _)) in hops.iter().enumerate().rev() {
        let pair = state.pairs.get(pair_key).ok_or(FailReason::UnknownPair)?;
        let (reserve_in, reserve_out) = oriented_reserves(pair, hop_in);
        let input = amm::amount_in_exact_out(needed, reserve_in, reserve_out, pair.fee_bps)?;
        required[index] = input;
        needed = input;
    }

    let total_in = required[0];
    if total_in > max_amount_in {
        return Err(FailReason::SlippageExceeded);
    }

    state
        .debit(signer, token_in, total_in, journal)
        .map_err(|_| FailReason::InsufficientBalance)?;

    // Forward execution with exactly the amounts from the backward pass — never recomputed.
    for (index, (pair_key, hop_in, _)) in hops.iter().enumerate() {
        let hop_out_amount = if index + 1 < required.len() {
            required[index + 1]
        } else {
            amount_out
        };
        if hop_out_amount == 0 {
            return Err(FailReason::ZeroOutput);
        }
        apply_hop(
            state,
            pair_key,
            hop_in,
            required[index],
            hop_out_amount,
            journal,
        )?;
    }

    let final_token = hops.last().expect("path is non-empty").2;
    state
        .credit(signer, &final_token, amount_out, journal)
        .map_err(|_| FailReason::Overflow)
}

// ---------------------------------------------------------------------------------------
// Staking actions
// ---------------------------------------------------------------------------------------

/// Pay out pending rewards and snapshot the accumulator. Returns the settled amount.
///
/// This is a transfer out of the reserve, never an emission: the four-bucket total is
/// unchanged by it.
fn settle_rewards(
    state: &mut State,
    signer: &AccountId,
    journal: &mut Journal,
) -> Result<Amount, FailReason> {
    let acc = state.global.acc_per_stake;
    let reserved = state.global.staking_reserved;
    let account = state
        .account(signer)
        .cloned()
        .unwrap_or_else(Account::default);
    let payout = settle_amount(&account, acc, reserved)?;

    if payout > 0 {
        state.global_mut(journal).staking_reserved -= payout;
        state
            .credit(signer, &NATIVE_TOKEN, payout, journal)
            .map_err(|_| FailReason::Overflow)?;
    }
    if let Some(account) = state.account_mut(signer, journal) {
        account.paid_acc = acc;
    }
    Ok(payout)
}

fn execute_stake(
    state: &mut State,
    signer: &AccountId,
    amount: Amount,
    journal: &mut Journal,
) -> Result<(), FailReason> {
    // Settle first (§8): the payout lands in the same action, and the guard below is
    // evaluated over the balance that includes it (§14.2).
    settle_rewards(state, signer, journal)?;

    let balance = state.balance_of(signer, &NATIVE_TOKEN);
    let required = amount.checked_add(FEE_TX).ok_or(FailReason::Overflow)?;
    if balance < required {
        // Without this guard an account staking its whole balance could never pay the fee
        // for its own Unstake: the value would be locked forever.
        return Err(FailReason::StakeLiquidityGuard);
    }

    state
        .debit(signer, &NATIVE_TOKEN, amount, journal)
        .map_err(|_| FailReason::InsufficientBalance)?;

    let acc = state.global.acc_per_stake;
    {
        let account = state
            .account_mut(signer, journal)
            .ok_or(FailReason::InsufficientBalance)?;
        account.staked = account
            .staked
            .checked_add(amount)
            .ok_or(FailReason::Overflow)?;
        account.paid_acc = acc;
    }
    let global = state.global_mut(journal);
    global.total_staked = global
        .total_staked
        .checked_add(amount)
        .ok_or(FailReason::Overflow)?;
    Ok(())
}

fn execute_unstake(
    state: &mut State,
    signer: &AccountId,
    amount: Amount,
    journal: &mut Journal,
) -> Result<(), FailReason> {
    settle_rewards(state, signer, journal)?;

    let staked = state.account(signer).map(|a| a.staked).unwrap_or(0);
    if staked < amount {
        return Err(FailReason::InsufficientBalance);
    }

    let acc = state.global.acc_per_stake;
    {
        let account = state
            .account_mut(signer, journal)
            .ok_or(FailReason::InsufficientBalance)?;
        account.staked -= amount;
        account.paid_acc = acc;
    }
    state.global_mut(journal).total_staked -= amount;
    state
        .credit(signer, &NATIVE_TOKEN, amount, journal)
        .map_err(|_| FailReason::Overflow)
}

/// Claimable reward of an account, for API and audit use.
pub fn account_pending(state: &State, id: &AccountId) -> Amount {
    state
        .account(id)
        .map(|a| pending(a, state.global.acc_per_stake))
        .unwrap_or(0)
}
