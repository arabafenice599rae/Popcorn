//! Blob encryption and decryption as pure functions (SPEC.md §3.6).
//!
//! Neither needs the network: encryption needs the chain's public key, decryption needs only
//! the round's signature — which a block already carries. That is what lets a verifier audit
//! a collection offline from `/chain/export` plus a blob mirror.
//!
//! Decryption is **total** (§3.6, §5.1): every `(blob, beacon)` maps either to a valid
//! plaintext or to `unusable`. The pinned `tlock` primitive does not honour that on its own —
//! `ibe.rs` reaches an `assert_eq!` on a crafted-but-profile-valid ciphertext and terminates
//! abnormally rather than returning an error — so [`decrypt`] contains that termination at the
//! per-blob boundary and reports it as an ordinary decryption failure. Node and verifier share
//! this one function, so they reach the same verdict on the same bytes, which is the property
//! the acceptance policy requires. This containment depends on `panic = "unwind"` (pinned in
//! the release profile and recorded in CONSENSUS-LOCK.md): under `abort` it is inert.

use std::cell::Cell;
use std::panic::{self, AssertUnwindSafe};
use std::sync::Once;

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

thread_local! {
    /// Set only for the duration of the contained call below, so the panic hook can stay
    /// silent for a termination we are about to convert into a verdict — without hiding a
    /// genuine bug panicking anywhere else.
    static CONTAINING: Cell<bool> = const { Cell::new(false) };
}

static HOOK: Once = Once::new();

/// Install, once, a panic hook that suppresses output only while a decryption is being
/// contained on this thread. Everything else panics as loudly as before.
fn install_quiet_hook() {
    HOOK.call_once(|| {
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            let contained = CONTAINING.with(|c| c.get());
            if !contained {
                previous(info);
            }
        }));
    });
}

/// Validate the profile, then decrypt with a verified beacon.
///
/// The profile check runs first and on attacker-chosen bytes: node and verifiers must reject
/// the same blobs for the same reason, or they disagree about which transactions exist. What
/// survives the profile is then decrypted inside a boundary that turns an abnormal termination
/// of the timelock primitive into [`TimelockError::Aborted`] — a decryption failure like any
/// other, resolving to `unusable`, rather than a panic that would take a decryption worker
/// (and, under `panic = "abort"`, the whole node) down with it.
pub fn decrypt(
    blob: &[u8],
    chain_hash: &[u8; 32],
    beacon: &Beacon,
) -> Result<Vec<u8>, TimelockError> {
    profile::validate(blob, beacon.round, chain_hash).map_err(TimelockError::Profile)?;

    install_quiet_hook();
    CONTAINING.with(|c| c.set(true));
    let outcome = panic::catch_unwind(AssertUnwindSafe(|| {
        let mut out = Vec::new();
        tlock_age::decrypt(&mut out, blob, chain_hash, &beacon.signature).map(|_| out)
    }));
    CONTAINING.with(|c| c.set(false));

    match outcome {
        Ok(Ok(out)) => Ok(out),
        Ok(Err(error)) => Err(TimelockError::Decrypt(error.to_string())),
        // Abnormal termination inside the primitive: a defined `unusable` outcome (§3.6), not a
        // crash. The bytes that reached here already passed the profile and earned a receipt,
        // so the blob is still manifested and the verdict is still checkable by anyone.
        Err(_) => Err(TimelockError::Aborted),
    }
}
