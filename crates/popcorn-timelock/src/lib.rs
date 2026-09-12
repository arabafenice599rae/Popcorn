//! The timelock layer (SPEC.md §3.3–§3.6).
//!
//! Everything tlock and drand sits behind one trait. The state machine never learns which
//! implementation is underneath, which is what makes the dependency substitutable by
//! construction — tlock-rs today, a native implementation over audited BLS tomorrow — rather
//! than an inseparable part of the architecture.
//!
//! Two things this layer deliberately does not do: it does not decide *when* collection
//! closes (that is the node's, and §1.2 is explicit that consensus does not bind it), and it
//! never falls back to other randomness when a beacon is missing.

pub mod beacon;
pub mod blob;
pub mod drand;
pub mod profile;
pub mod static_provider;

pub use beacon::{Beacon, BeaconOutcome, FetchError};
pub use drand::DrandTimelock;
pub use profile::{BlobHeader, ProfileError};
pub use static_provider::StaticTimelock;

/// Failures of the timelock layer itself, as opposed to of a round.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TimelockError {
    /// The blob is outside POPCORN-TLOCK-AGE-V1 (§3.6). Resolves to `unusable`.
    Profile(ProfileError),
    Encrypt(String),
    /// Decryption failed with a valid-looking header. Also resolves to `unusable`.
    Decrypt(String),
    /// The timelock primitive terminated abnormally on a crafted-but-profile-valid ciphertext
    /// (a reachable assertion in the pinned `tlock`), contained at the per-blob boundary.
    /// Resolves to `unusable` exactly like `Decrypt`: the totality of §3.6/§5.1 realized.
    Aborted,
    /// A remote serves a different drand chain than the one pinned at genesis.
    WrongChain,
    WrongScheme(String),
    NoRemoteAvailable,
}

/// The single interface between POPCORN and timelock encryption.
pub trait TimelockProvider: Send + Sync {
    /// The drand chain this provider is pinned to.
    fn chain_hash(&self) -> [u8; 32];

    /// Encrypt a payload toward `round`, in the POPCORN-TLOCK-AGE-V1 profile.
    fn encrypt(&self, plaintext: &[u8], round: u64) -> Result<Vec<u8>, TimelockError>;

    /// Decrypt a blob with a verified beacon, rejecting anything outside the profile first.
    fn decrypt(&self, blob: &[u8], beacon: &Beacon) -> Result<Vec<u8>, TimelockError>;

    /// Fetch and verify the beacon for a round, under the policy of §3.3.
    fn get_beacon(&self, round: u64) -> BeaconOutcome;

    /// The round covering a wall-clock instant.
    ///
    /// This is used to decide what a *client* should target and to pace block production —
    /// never to decide what a block contains. Wall-clock time does not enter consensus
    /// (§3.4).
    fn round_for_time(&self, unix_seconds: u64) -> u64;
}

/// Decrypt a blob and decode the transaction inside it.
///
/// Returning `None` is the whole `unusable` set of §5.1 in one place: a blob outside the
/// profile, a failed AEAD, and a payload that decrypts but does not decode are all the same
/// outcome — no transaction exists, so no `tx_id` exists to reject.
pub fn decrypt_transaction<P: TimelockProvider + ?Sized>(
    provider: &P,
    blob: &[u8],
    beacon: &Beacon,
) -> Option<popcorn_core::SignedTx> {
    let plaintext = provider.decrypt(blob, beacon).ok()?;
    borsh::from_slice::<popcorn_core::SignedTx>(&plaintext).ok()
}
