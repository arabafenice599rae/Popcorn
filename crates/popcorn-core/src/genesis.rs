//! Genesis (SPEC.md §4.3): block 0, an empty state, and no allocation at all.
//!
//! Fair launch means exactly this: the genesis block mints nothing. The first native units
//! come into existence with the emission that closes block 1.

use crate::constants::GENESIS_SUPPLY;
use crate::execute::{collection_root, rejected_root, results_root, txs_root};
use crate::state::State;
use crate::types::{Block, Header};

/// Chain configuration stamped into genesis and served by `/params`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GenesisConfig {
    /// drand round mapped to block 1: `round(h) = genesis_drand_round + h − 1`.
    pub genesis_drand_round: u64,
    /// Public key of the block-signing key. Controls no funds.
    pub node_pubkey: [u8; 32],
    /// Public key of the foundation key. Signs no blocks.
    pub foundation_pubkey: [u8; 32],
}

impl GenesisConfig {
    /// The account receiving the foundation share of emission.
    pub fn foundation_account(&self) -> [u8; 32] {
        crate::crypto::account_id_from_pubkey(&self.foundation_pubkey)
    }
}

/// The genesis state: empty, with `GENESIS_SUPPLY` units in existence (which is zero).
pub fn genesis_state() -> State {
    debug_assert_eq!(GENESIS_SUPPLY, 0, "fair launch: nothing is allocated");
    State::new()
}

/// The genesis block. It carries no beacon: the round mapping starts at height 1.
pub fn genesis_block(state: &State, node_signature: [u8; 64]) -> Block {
    let header = Header {
        height: 0,
        prev_hash: [0u8; 32],
        drand_round: 0,
        drand_sig_hash: crate::crypto::blake3_hash(&[]),
        collection_root: collection_root(&[]),
        txs_root: txs_root(&[]),
        rejected_root: rejected_root(&[]),
        results_root: results_root(&[]),
        state_root: state.state_root(),
    };
    Block {
        header,
        drand_signature: Vec::new(),
        blob_manifest: Vec::new(),
        unusable: Vec::new(),
        txs: Vec::new(),
        results: Vec::new(),
        rejected: Vec::new(),
        node_signature,
    }
}

/// The drand round a block height maps to (§3.4). Never skipped: after downtime the node
/// catches up by producing the missing blocks in sequence.
pub fn round_for_height(genesis_drand_round: u64, height: u64) -> u64 {
    debug_assert!(height >= 1, "height 0 has no beacon");
    genesis_drand_round + height - 1
}
