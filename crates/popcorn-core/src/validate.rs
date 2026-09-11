//! Static validation (SPEC.md §5.2): the ordered pipeline that splits a decrypted batch
//! into what will execute and what is rejected.
//!
//! Order is normative. A transaction dropped at one step does not take part in the
//! following ones, and the reason recorded in the block is the reason of the step that
//! dropped it — `rejected_root` commits to it.

use std::collections::BTreeMap;

use crate::constants::{
    is_valid_fee_tier, HTLC_MAX_LIFETIME_ROUNDS, MAX_PATH_LEN, MAX_PUBLISH_SIZE, MAX_SUPPLY,
    MAX_TX_PER_ACCOUNT_PER_BATCH, NATIVE_TOKEN,
};
use crate::crypto::{signing_hash, verify_signature};
use crate::fees::tx_fee;
use crate::state::State;
use crate::types::{Action, AccountId, RejectReason, SignedTx};

/// Outcome of static validation.
pub struct ValidationOutcome {
    /// Transactions that will execute, sorted by ascending `tx_id` — the shuffle's input.
    pub valid: Vec<SignedTx>,
    /// Rejected transactions with their reason, sorted by `tx_id`.
    pub rejected: Vec<([u8; 32], RejectReason)>,
}

/// Run the full pipeline for round `round` against the pre-batch `state`.
pub fn validate_batch(state: &State, txs: Vec<SignedTx>, round: u64) -> ValidationOutcome {
    let mut rejected: Vec<([u8; 32], RejectReason)> = Vec::new();
    let mut survivors: Vec<(SignedTx, [u8; 32], AccountId)> = Vec::new();

    // Steps 2-5: per-transaction checks, in order.
    for tx in txs {
        let tx_id = tx.tx_id();

        // 2. signature, and with it the signer's identity
        if !verify_signature(
            &tx.signer_pubkey,
            &signing_hash(&tx.payload),
            &tx.signature,
        ) {
            rejected.push((tx_id, RejectReason::BadSignature));
            continue;
        }
        let signer = tx.signer();

        // 3. the transaction must target this round
        if tx.payload.target_round != round {
            rejected.push((tx_id, RejectReason::WrongRound));
            continue;
        }

        // 4. account existence, then pubkey coherence, then terminal nonce — the sub-order
        //    is pinned, so PubkeyMismatch wins over NonceExhausted (§5.2)
        let Some(account) = state.account(&signer) else {
            rejected.push((tx_id, RejectReason::UnknownAccount));
            continue;
        };
        if let Some(pk) = account.pubkey {
            if pk != tx.signer_pubkey {
                rejected.push((tx_id, RejectReason::PubkeyMismatch));
                continue;
            }
        }
        if account.nonce == u64::MAX {
            rejected.push((tx_id, RejectReason::NonceExhausted));
            continue;
        }

        // 5. field ranges
        if !fields_in_range(&tx, round) {
            rejected.push((tx_id, RejectReason::FieldOutOfRange));
            continue;
        }

        survivors.push((tx, tx_id, signer));
    }

    // Deterministic grouping: accounts in key order, transactions in tx_id order.
    survivors.sort_by(|a, b| a.1.cmp(&b.1));
    let mut by_account: BTreeMap<AccountId, Vec<(SignedTx, [u8; 32])>> = BTreeMap::new();
    for (tx, tx_id, signer) in survivors {
        by_account.entry(signer).or_default().push((tx, tx_id));
    }

    let mut valid: Vec<SignedTx> = Vec::new();

    for (signer, entries) in by_account {
        // 6. nonce dedup: for equal (signer, nonce) the smallest tx_id survives. Entries are
        //    already tx_id-ordered, so the first occurrence of a nonce is the winner.
        let mut kept: Vec<(SignedTx, [u8; 32])> = Vec::new();
        let mut seen_nonces: BTreeMap<u64, ()> = BTreeMap::new();
        for (tx, tx_id) in entries {
            if seen_nonces.insert(tx.payload.nonce, ()).is_some() {
                rejected.push((tx_id, RejectReason::DuplicateNonce));
            } else {
                kept.push((tx, tx_id));
            }
        }

        // 7. contiguity from account.nonce + 1; everything from the first gap on is dropped
        kept.sort_by_key(|(tx, _)| tx.payload.nonce);
        let account = state.account(&signer).expect("checked at step 4");
        let mut expected = account.nonce + 1;
        let mut contiguous: Vec<(SignedTx, [u8; 32])> = Vec::new();
        let mut gap_reached = false;
        for (tx, tx_id) in kept {
            if gap_reached || tx.payload.nonce != expected {
                gap_reached = true;
                rejected.push((tx_id, RejectReason::NonceGap));
                continue;
            }
            expected += 1;
            contiguous.push((tx, tx_id));
        }

        // 8. per-account budget, keeping the lowest nonces
        if contiguous.len() > MAX_TX_PER_ACCOUNT_PER_BATCH {
            for (_, tx_id) in contiguous.drain(MAX_TX_PER_ACCOUNT_PER_BATCH..) {
                rejected.push((tx_id, RejectReason::OverBudget));
            }
        }

        // 9. fee solvency against the PRE-BATCH balance, dropping from the highest nonce
        //    down until the account can pay for everything that remains
        let balance = state.balance_of(&signer, &NATIVE_TOKEN);
        let mut owed: u128 = contiguous.iter().map(|(tx, _)| tx_fee(tx)).sum();
        while owed > balance {
            let Some((tx, tx_id)) = contiguous.pop() else {
                break;
            };
            owed -= tx_fee(&tx);
            rejected.push((tx_id, RejectReason::FeeInsolvent));
        }

        valid.extend(contiguous.into_iter().map(|(tx, _)| tx));
    }

    // The shuffle is defined over a list sorted by ascending tx_id (§3.7), and the block
    // records rejections in the same canonical order.
    valid.sort_by_key(|tx| tx.tx_id());
    rejected.sort_by(|a, b| a.0.cmp(&b.0));

    ValidationOutcome { valid, rejected }
}

/// Step 5: the static field ranges, per action.
fn fields_in_range(tx: &SignedTx, round: u64) -> bool {
    match &tx.payload.action {
        Action::Transfer { amount, .. } => *amount > 0,
        Action::CreateToken { name, supply } => {
            *supply >= 1 && *supply <= MAX_SUPPLY && is_printable_ascii(name)
        }
        Action::CreatePair { fee_bps, .. } => is_valid_fee_tier(*fee_bps),
        Action::AddLiquidity { .. } | Action::RemoveLiquidity { .. } => true,
        Action::SwapExactIn { path, .. } | Action::SwapExactOut { path, .. } => {
            !path.is_empty() && path.len() <= MAX_PATH_LEN
        }
        Action::Publish { data, .. } => data.len() <= MAX_PUBLISH_SIZE,
        Action::HtlcLock {
            amount,
            expiry_round,
            ..
        } => {
            *amount > 0
                && *expiry_round > round
                && *expiry_round <= round.saturating_add(HTLC_MAX_LIFETIME_ROUNDS)
        }
        Action::HtlcClaim { .. } | Action::HtlcRefund { .. } => true,
        Action::Stake { amount } | Action::Unstake { amount } => *amount > 0,
        Action::ClaimRewards {} => true,
    }
}

/// Token names are 16 bytes of printable ASCII, right zero-padded (§7.3).
fn is_printable_ascii(name: &[u8; 16]) -> bool {
    let mut padding = false;
    for &byte in name {
        match byte {
            0x00 => padding = true,
            0x20..=0x7E => {
                if padding {
                    // A printable byte after padding would make two different encodings of
                    // the same visible name; the canonical form is right-padded only.
                    return false;
                }
            }
            _ => return false,
        }
    }
    true
}
