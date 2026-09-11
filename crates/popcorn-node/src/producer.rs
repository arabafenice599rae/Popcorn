//! Block production (SPEC.md §5.1).
//!
//! The loop is deliberately dull, and its dullness is the point. It closes collection, asks
//! for exactly one round's beacon, decrypts what it committed to, executes, and commits. It
//! never skips a round, never substitutes randomness for a missing beacon, and never
//! produces a block on a beacon it could not verify.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ed25519_dalek::SigningKey;
use popcorn_core::types::{Block, SignedTx};
use popcorn_timelock::{Beacon, BeaconOutcome, TimelockProvider};
use tokio::sync::{broadcast, Mutex};

use crate::chain::Chain;
use crate::mempool::Mempool;

/// What happened to one production attempt.
#[derive(Debug)]
pub enum Tick {
    Produced(Box<Block>),
    /// The beacon for the expected round has not arrived. The chain waits (§3.3).
    Waiting(u64),
    /// Every trusted source served an unverifiable signature for this round. Stop.
    Halt(u64, String),
    Storage(String),
}

pub struct Producer {
    pub chain: Arc<Mutex<Chain>>,
    pub mempool: Arc<Mempool>,
    pub timelock: Arc<dyn TimelockProvider>,
    pub node_key: SigningKey,
    pub blocks: broadcast::Sender<Block>,
}

impl Producer {
    /// Attempt to produce the next block.
    pub async fn tick(&self) -> Tick {
        let round = {
            let chain = self.chain.lock().await;
            chain.next_round()
        };

        // Freeze the collection BEFORE asking for the beacon. Consensus does not force this
        // order (§1.2 says so plainly), which is exactly why the node should commit first and
        // publish the root: the honest sequence is what the receipts are evidence about.
        let collected = self.mempool.take_round(round);
        let manifest: Vec<[u8; 32]> = collected.iter().map(|(hash, _)| *hash).collect();

        let timelock = Arc::clone(&self.timelock);
        let beacon = match tokio::task::spawn_blocking(move || timelock.get_beacon(round))
            .await
            .unwrap_or(BeaconOutcome::NotAvailable(vec![]))
        {
            BeaconOutcome::Available(beacon) => beacon,
            BeaconOutcome::NotAvailable(_) => {
                // Put the collection back: the round is not lost, it is not ready.
                self.restore(round, collected);
                return Tick::Waiting(round);
            }
            BeaconOutcome::Invalid(errors) => {
                self.restore(round, collected);
                return Tick::Halt(round, format!("{errors:?}"));
            }
        };

        let (txs, unusable) = self.decrypt_collection(&collected, &beacon);

        let mut chain = self.chain.lock().await;
        match chain.produce(
            beacon.signature.clone(),
            manifest,
            unusable,
            txs,
            collected,
            &self.node_key,
        ) {
            Ok(block) => {
                let _ = self.blocks.send(block.clone());
                Tick::Produced(Box::new(block))
            }
            Err(error) => Tick::Storage(error.to_string()),
        }
    }

    /// Decrypt everything committed to, and classify what does not yield a transaction.
    ///
    /// `unusable` is derived here, never asserted: a failed timelock, a failed AEAD and a
    /// payload that decrypts but does not decode are one outcome, because in all three cases
    /// no transaction — and therefore no `tx_id` — exists (§5.1).
    fn decrypt_collection(
        &self,
        collected: &[([u8; 32], Vec<u8>)],
        beacon: &Beacon,
    ) -> (Vec<SignedTx>, Vec<[u8; 32]>) {
        let mut txs = Vec::new();
        let mut unusable = Vec::new();
        for (hash, blob) in collected {
            match popcorn_timelock::decrypt_transaction(self.timelock.as_ref(), blob, beacon) {
                Some(tx) => txs.push(tx),
                None => unusable.push(*hash),
            }
        }
        unusable.sort_unstable();
        (txs, unusable)
    }

    fn restore(&self, round: u64, collected: Vec<([u8; 32], Vec<u8>)>) {
        // Re-queue verbatim: these blobs were receipted, so they must still reach a manifest.
        for (_, blob) in collected {
            let _ = self
                .mempool
                .submit(blob, round, round, &self.node_key, now_ms());
        }
    }

    /// Run until the chain halts or the process stops.
    ///
    /// Pacing is wall-clock, but wall-clock never enters consensus (§3.4): it decides only
    /// how often to try, while what a block contains comes from its round alone.
    pub async fn run(self: Arc<Self>, period: Duration) {
        let mut ticker = tokio::time::interval(period);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            // Catch up one block at a time: after downtime, missing rounds are produced in
            // sequence, as empty blocks if nothing was collected. Rounds are never skipped,
            // so HTLC windows stay whole.
            loop {
                match self.tick().await {
                    Tick::Produced(block) => {
                        if block.header.drand_round + 1
                            >= self.timelock.round_for_time(now_seconds())
                        {
                            break;
                        }
                    }
                    Tick::Waiting(_) => break,
                    Tick::Halt(round, reason) => {
                        eprintln!(
                            "halt: every trusted source served an unverifiable beacon for round {round}: {reason}"
                        );
                        return;
                    }
                    Tick::Storage(error) => {
                        eprintln!("halt: storage failure: {error}");
                        return;
                    }
                }
            }
        }
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
