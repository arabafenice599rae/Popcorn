//! Shared test scaffolding: deterministic keys, funded accounts, and a batch runner.

#![allow(dead_code)]

use ed25519_dalek::SigningKey;
use popcorn_core::constants::NATIVE_TOKEN;
use popcorn_core::crypto::{account_id_from_pubkey, sign_payload};
use popcorn_core::execute::{execute_batch, BatchInput, BatchOutput};
use popcorn_core::state::{Journal, State};
use popcorn_core::types::{AccountId, Action, Amount, ExecStatus, SignedTx, TxPayload};

/// A deterministic keypair. Seeded by a single byte so tests read as "actor 1", "actor 2".
pub struct Actor {
    pub key: SigningKey,
    pub id: AccountId,
}

impl Actor {
    pub fn new(seed: u8) -> Self {
        let key = SigningKey::from_bytes(&[seed; 32]);
        let pubkey = key.verifying_key().to_bytes();
        Self {
            id: account_id_from_pubkey(&pubkey),
            key,
        }
    }

    pub fn pubkey(&self) -> [u8; 32] {
        self.key.verifying_key().to_bytes()
    }

    /// Sign an action into a transaction targeting `round`.
    pub fn tx(&self, nonce: u64, round: u64, action: Action) -> SignedTx {
        let payload = TxPayload {
            nonce,
            target_round: round,
            action,
        };
        let signature = sign_payload(&self.key, &payload);
        SignedTx {
            payload,
            signer_pubkey: self.pubkey(),
            signature,
        }
    }
}

/// Credit native units to an account as if they had been emitted.
///
/// Tests need funded accounts without mining a year of blocks; accounting for the credit in
/// `native_emitted` keeps the four-bucket invariant true, so invariant assertions stay
/// meaningful.
pub fn fund(state: &mut State, id: &AccountId, amount: Amount) {
    let mut journal = Journal::new();
    state.credit(id, &NATIVE_TOKEN, amount, &mut journal).unwrap();
    state.global_mut(&mut journal).native_emitted += amount;
}

/// Credit an arbitrary token, bypassing `CreateToken`. Non-native tokens are outside the
/// monetary invariant, so nothing else needs adjusting.
pub fn fund_token(state: &mut State, id: &AccountId, token: &[u8; 32], amount: Amount) {
    let mut journal = Journal::new();
    state.credit(id, token, amount, &mut journal).unwrap();
}

/// A fixed beacon signature. Any bytes work: the shuffle only needs them to be committed to.
pub fn beacon(round: u64) -> Vec<u8> {
    let mut sig = vec![0u8; 48];
    sig[..8].copy_from_slice(&round.to_le_bytes());
    sig
}

/// Run one batch at `height` (round = height, for readability) and return its output.
pub fn run_batch(
    state: &mut State,
    height: u64,
    txs: Vec<SignedTx>,
    foundation: AccountId,
) -> BatchOutput {
    let manifest: Vec<[u8; 32]> = txs.iter().map(|tx| tx.tx_id()).collect();
    let input = BatchInput {
        height,
        prev_hash: [0u8; 32],
        drand_round: height,
        drand_signature: beacon(height),
        blob_manifest: manifest,
        unusable: Vec::new(),
        txs,
        foundation,
    };
    execute_batch(state, input)
}

/// The result of the transaction with this id, if it executed.
pub fn status_of(output: &BatchOutput, tx_id: &[u8; 32]) -> Option<ExecStatus> {
    output
        .txs
        .iter()
        .position(|tx| tx.tx_id() == *tx_id)
        .map(|index| output.results[index])
}

/// Whether a transaction was rejected, and why.
pub fn reject_of(
    output: &BatchOutput,
    tx_id: &[u8; 32],
) -> Option<popcorn_core::types::RejectReason> {
    output
        .rejected
        .iter()
        .find(|(id, _)| id == tx_id)
        .map(|(_, reason)| *reason)
}
