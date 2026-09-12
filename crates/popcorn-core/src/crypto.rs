//! Hashing and signature semantics (SPEC.md §3.1).
//!
//! Signature verification is a consensus rule, not an implementation detail: a replaying
//! verifier must accept and reject exactly the same signatures as the node. The frozen
//! semantics is `ed25519-dalek`'s `verify_strict` at the pinned version, which rejects
//! small-order points and non-canonical `s`.

use borsh::BorshSerialize;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};

use crate::constants::{CONSENSUS_LOCK, LOCK_DOMAIN, SIGN_DOMAIN};

/// BLAKE3-256 over a byte slice.
pub fn blake3_hash(bytes: &[u8]) -> [u8; 32] {
    *blake3::hash(bytes).as_bytes()
}

/// SHA-256, used only for HTLC hashlocks (§7.6) — the single non-blake3 point in the
/// protocol, chosen for cross-chain interoperability.
pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

/// Digest of the pinned dependency list of §13, stamped into genesis.
///
/// ```text
/// blake3( LOCK_DOMAIN
///         || LE32(count)
///         || for each entry, in the frozen order of CONSENSUS_LOCK:
///              LE32(len(name)) || name || LE32(len(version)) || version )
/// ```
///
/// Length prefixes throughout, so no pair of a name and a version can be re-cut into a
/// different pair with the same digest.
pub fn consensus_lock_digest() -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    h.update(LOCK_DOMAIN);
    h.update(&(CONSENSUS_LOCK.len() as u32).to_le_bytes());
    for (name, version) in CONSENSUS_LOCK {
        h.update(&(name.len() as u32).to_le_bytes());
        h.update(name.as_bytes());
        h.update(&(version.len() as u32).to_le_bytes());
        h.update(version.as_bytes());
    }
    *h.finalize().as_bytes()
}

/// `AccountId = blake3(verifying_key)` (§3.1).
pub fn account_id_from_pubkey(pubkey: &[u8; 32]) -> [u8; 32] {
    blake3_hash(pubkey)
}

/// The message actually signed: `blake3(SIGN_DOMAIN || borsh(payload))`.
///
/// The domain prefix is what stops a signature produced for a Solana wallet from being
/// replayed here, and vice versa.
pub fn signing_hash<T: BorshSerialize>(payload: &T) -> [u8; 32] {
    let encoded = borsh::to_vec(payload).expect("borsh serialization of a consensus payload");
    let mut h = blake3::Hasher::new();
    h.update(SIGN_DOMAIN);
    h.update(&encoded);
    *h.finalize().as_bytes()
}

/// Verify an ed25519 signature over `message` under the frozen `verify_strict` semantics.
///
/// A malformed key or signature is a verification failure, never a panic: blobs are
/// attacker-controlled.
pub fn verify_signature(pubkey: &[u8; 32], message: &[u8; 32], signature: &[u8; 64]) -> bool {
    let Ok(vk) = VerifyingKey::from_bytes(pubkey) else {
        return false;
    };
    let sig = Signature::from_bytes(signature);
    vk.verify_strict(message, &sig).is_ok()
}

/// Sign a 32-byte message with an ed25519 signing key.
pub fn sign(key: &SigningKey, message: &[u8; 32]) -> [u8; 64] {
    key.sign(message).to_bytes()
}

/// Sign a Borsh payload under the POPCORN signing domain.
pub fn sign_payload<T: BorshSerialize>(key: &SigningKey, payload: &T) -> [u8; 64] {
    sign(key, &signing_hash(payload))
}
