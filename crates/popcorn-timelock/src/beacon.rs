//! Beacon types and the consensus-defined policy for a missing or invalid round
//! (SPEC.md §3.3).
//!
//! The point of enumerating outcomes is that nothing is left to the runtime: a node that
//! cannot get a verified beacon waits, and a node that gets a *forged* one stops. Neither
//! ever invents randomness, and neither ever skips a round.

/// A verified randomness beacon for one round.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Beacon {
    pub round: u64,
    /// Raw BLS signature bytes. These seed the shuffle and are committed to in the header.
    pub signature: Vec<u8>,
}

/// Why one remote failed to provide a usable beacon.
///
/// Everything except `InvalidSignature` is a fault of that *remote*, not of the round.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FetchError {
    /// HTTP failure or timeout.
    FetchFailure(String),
    /// The response did not parse as a beacon.
    MalformedResponse(String),
    /// The remote answered for a different round than the one requested.
    WrongRound { expected: u64, received: u64 },
    /// The remote serves a different drand chain than the one pinned at genesis.
    WrongChain,
    /// The BLS signature does not verify against the pinned chain info. This is the only
    /// error that can halt the chain — and only if every trusted source agrees on it.
    InvalidSignature,
}

impl FetchError {
    /// Whether this error indicts the round rather than the remote.
    pub fn is_forgery(&self) -> bool {
        matches!(self, FetchError::InvalidSignature)
    }
}

/// The frozen outcomes of asking for round `R`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BeaconOutcome {
    /// A BLS-verified beacon: produce block `R`.
    Available(Beacon),
    /// No remote answered. Retry with backoff — the chain waits, it never skips.
    NotAvailable(Vec<FetchError>),
    /// Every trusted source returned a signature that does not verify. Halt explicitly:
    /// a block with an unverified beacon is never produced.
    Invalid(Vec<FetchError>),
}

impl BeaconOutcome {
    /// Classify a round's per-remote results under the policy of §3.3.
    ///
    /// Lateness is deliberately absent: it is liveness and telemetry, not a ledger category.
    pub fn classify(errors: Vec<FetchError>) -> Self {
        if !errors.is_empty() && errors.iter().all(FetchError::is_forgery) {
            BeaconOutcome::Invalid(errors)
        } else {
            BeaconOutcome::NotAvailable(errors)
        }
    }
}
