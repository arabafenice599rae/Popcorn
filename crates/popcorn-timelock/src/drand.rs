//! The drand-backed [`TimelockProvider`](crate::TimelockProvider) (SPEC.md §3.3, §3.5).

use std::sync::Mutex;

use drand_core::HttpClient;
use popcorn_core::constants::{DRAND_CHAIN_HASH, DRAND_SCHEME};

use crate::beacon::{Beacon, BeaconOutcome, FetchError};
use crate::{TimelockError, TimelockProvider};

/// A timelock backed by drand quicknet over HTTP, with ordered remote fallback.
pub struct DrandTimelock {
    clients: Vec<HttpClient>,
    chain_hash: [u8; 32],
    public_key: Vec<u8>,
    genesis_time: u64,
    period: u64,
    /// Remote URLs, kept for diagnostics in the same order as `clients`.
    remotes: Vec<String>,
    cache: Mutex<Option<Beacon>>,
}

impl DrandTimelock {
    /// Connect to the pinned chain through `remotes`, in order.
    ///
    /// Chain info is fetched once and checked against the values frozen at genesis: a remote
    /// serving a different chain or scheme is rejected here rather than discovered later.
    pub fn connect(remotes: &[&str]) -> Result<Self, TimelockError> {
        let chain_hash = parse_hex32(DRAND_CHAIN_HASH).expect("pinned chain hash is valid hex");

        let mut clients = Vec::new();
        let mut urls = Vec::new();
        let mut info = None;

        for remote in remotes {
            let url = format!("{}/{}", remote.trim_end_matches('/'), DRAND_CHAIN_HASH);
            let Ok(client) = HttpClient::try_from(url.as_str()) else {
                continue;
            };
            if info.is_none() {
                match client.chain_info() {
                    Ok(fetched) => {
                        if fetched.hash() != chain_hash.to_vec() {
                            return Err(TimelockError::WrongChain);
                        }
                        if fetched.scheme_id() != DRAND_SCHEME {
                            return Err(TimelockError::WrongScheme(
                                fetched.scheme_id().to_string(),
                            ));
                        }
                        info = Some(fetched);
                    }
                    Err(_) => continue,
                }
            }
            clients.push(client);
            urls.push(url);
        }

        let info = info.ok_or(TimelockError::NoRemoteAvailable)?;
        Ok(Self {
            clients,
            chain_hash,
            public_key: info.public_key(),
            genesis_time: info.genesis_time(),
            period: info.period(),
            remotes: urls,
            cache: Mutex::new(None),
        })
    }

    /// The remotes this provider will try, in order.
    pub fn remotes(&self) -> &[String] {
        &self.remotes
    }

    /// The chain's public key, for building an offline provider from the same parameters.
    pub fn public_key(&self) -> &[u8] {
        &self.public_key
    }

    /// Chain genesis time and period, as pinned drand parameters.
    pub fn timing(&self) -> (u64, u64) {
        (self.genesis_time, self.period)
    }
}

impl TimelockProvider for DrandTimelock {
    fn chain_hash(&self) -> [u8; 32] {
        self.chain_hash
    }

    fn encrypt(&self, plaintext: &[u8], round: u64) -> Result<Vec<u8>, TimelockError> {
        crate::blob::encrypt(plaintext, &self.chain_hash, &self.public_key, round)
    }

    fn decrypt(&self, blob: &[u8], beacon: &Beacon) -> Result<Vec<u8>, TimelockError> {
        crate::blob::decrypt(blob, &self.chain_hash, beacon)
    }

    fn get_beacon(&self, round: u64) -> BeaconOutcome {
        if let Some(cached) = self.cache.lock().unwrap().clone() {
            if cached.round == round {
                return BeaconOutcome::Available(cached);
            }
        }

        let mut errors = Vec::new();
        for client in &self.clients {
            match client.get(round) {
                Ok(randomness) => {
                    if randomness.round() != round {
                        errors.push(FetchError::WrongRound {
                            expected: round,
                            received: randomness.round(),
                        });
                        continue;
                    }
                    // drand_core verifies the BLS signature against the cached chain info on
                    // fetch; a beacon that reaches here has been checked, and one that fails
                    // verification surfaces as an error below.
                    let beacon = Beacon {
                        round,
                        signature: randomness.signature(),
                    };
                    *self.cache.lock().unwrap() = Some(beacon.clone());
                    return BeaconOutcome::Available(beacon);
                }
                Err(error) => errors.push(classify_error(&error.to_string())),
            }
        }

        BeaconOutcome::classify(errors)
    }

    fn round_for_time(&self, unix_seconds: u64) -> u64 {
        if unix_seconds <= self.genesis_time || self.period == 0 {
            return 1;
        }
        (unix_seconds - self.genesis_time) / self.period + 1
    }
}

/// Map a client error string onto the taxonomy of §3.3.
///
/// Only a signature that does not verify indicts the round; everything else indicts the
/// remote, and the next one is tried.
fn classify_error(message: &str) -> FetchError {
    let lower = message.to_ascii_lowercase();
    if lower.contains("signature") || lower.contains("verif") {
        FetchError::InvalidSignature
    } else if lower.contains("chain") {
        FetchError::WrongChain
    } else if lower.contains("parse") || lower.contains("json") || lower.contains("decode") {
        FetchError::MalformedResponse(message.to_string())
    } else {
        FetchError::FetchFailure(message.to_string())
    }
}

fn parse_hex32(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 {
        return None;
    }
    let bytes = text.as_bytes();
    let mut out = [0u8; 32];
    for (index, slot) in out.iter_mut().enumerate() {
        let hi = (bytes[index * 2] as char).to_digit(16)? as u8;
        let lo = (bytes[index * 2 + 1] as char).to_digit(16)? as u8;
        *slot = (hi << 4) | lo;
    }
    Some(out)
}
