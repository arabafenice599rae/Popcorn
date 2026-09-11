//! A network-free [`TimelockProvider`](crate::TimelockProvider) over known beacons.
//!
//! Replay is not a second live node: every block carries the BLS signature of its own round,
//! so a verifier can re-derive the ordering and re-decrypt the collection with what it
//! already holds. This provider is that path — and the one tests use, so the suite does not
//! depend on drand being up.

use std::collections::BTreeMap;

use crate::beacon::{Beacon, BeaconOutcome, FetchError};
use crate::{blob, TimelockError, TimelockProvider};

pub struct StaticTimelock {
    chain_hash: [u8; 32],
    public_key: Vec<u8>,
    beacons: BTreeMap<u64, Beacon>,
    genesis_time: u64,
    period: u64,
}

impl StaticTimelock {
    /// Build from the pinned chain parameters. `public_key` may be empty if the provider is
    /// only used to decrypt.
    pub fn new(chain_hash: [u8; 32], public_key: Vec<u8>, genesis_time: u64, period: u64) -> Self {
        Self {
            chain_hash,
            public_key,
            beacons: BTreeMap::new(),
            genesis_time,
            period,
        }
    }

    /// Record a beacon this provider can serve.
    pub fn with_beacon(mut self, beacon: Beacon) -> Self {
        self.beacons.insert(beacon.round, beacon);
        self
    }

    pub fn insert_beacon(&mut self, beacon: Beacon) {
        self.beacons.insert(beacon.round, beacon);
    }
}

impl TimelockProvider for StaticTimelock {
    fn chain_hash(&self) -> [u8; 32] {
        self.chain_hash
    }

    fn encrypt(&self, plaintext: &[u8], round: u64) -> Result<Vec<u8>, TimelockError> {
        blob::encrypt(plaintext, &self.chain_hash, &self.public_key, round)
    }

    fn decrypt(&self, blob_bytes: &[u8], beacon: &Beacon) -> Result<Vec<u8>, TimelockError> {
        blob::decrypt(blob_bytes, &self.chain_hash, beacon)
    }

    fn get_beacon(&self, round: u64) -> BeaconOutcome {
        match self.beacons.get(&round) {
            Some(beacon) => BeaconOutcome::Available(beacon.clone()),
            // Absent, not forged: the chain waits rather than halting (§3.3).
            None => BeaconOutcome::NotAvailable(vec![FetchError::FetchFailure(
                "round not held by this provider".to_string(),
            )]),
        }
    }

    fn round_for_time(&self, unix_seconds: u64) -> u64 {
        if unix_seconds <= self.genesis_time || self.period == 0 {
            return 1;
        }
        (unix_seconds - self.genesis_time) / self.period + 1
    }
}
