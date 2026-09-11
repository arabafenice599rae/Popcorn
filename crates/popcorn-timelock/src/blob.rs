//! Blob encryption and decryption as pure functions (SPEC.md §3.6).
//!
//! Neither needs the network: encryption needs the chain's public key, decryption needs only
//! the round's signature — which a block already carries. That is what lets a verifier audit
//! a collection offline from `/chain/export` plus a blob mirror.

use crate::beacon::Beacon;
use crate::profile;
use crate::TimelockError;

/// Encrypt a payload toward `round` in the POPCORN-TLOCK-AGE-V1 profile.
pub fn encrypt(
    plaintext: &[u8],
    chain_hash: &[u8; 32],
    public_key: &[u8],
    round: u64,
) -> Result<Vec<u8>, TimelockError> {
    let mut out = Vec::new();
    tlock_age::encrypt(&mut out, plaintext, chain_hash, public_key, round)
        .map_err(|e| TimelockError::Encrypt(e.to_string()))?;
    Ok(out)
}

/// Validate the profile, then decrypt with a verified beacon.
///
/// The profile check runs first and on attacker-chosen bytes: node and verifiers must reject
/// the same blobs for the same reason, or they disagree about which transactions exist.
pub fn decrypt(
    blob: &[u8],
    chain_hash: &[u8; 32],
    beacon: &Beacon,
) -> Result<Vec<u8>, TimelockError> {
    profile::validate(blob, beacon.round, chain_hash).map_err(TimelockError::Profile)?;
    let mut out = Vec::new();
    tlock_age::decrypt(&mut out, blob, chain_hash, &beacon.signature)
        .map_err(|e| TimelockError::Decrypt(e.to_string()))?;
    Ok(out)
}
