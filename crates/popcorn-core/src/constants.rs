//! Protocol parameters (SPEC.md §11) and consensus identifiers (§13).
//!
//! Everything in this module is frozen at genesis. Changing any of it after genesis is
//! consensus-breaking per §13.3 and produces, in effect, a different chain.

/// Normative consensus version, three 16-bit fields `0x{reserved}_{minor}_{patch}`.
pub const CONSENSUS_VERSION: u64 = 0x0000_0009_0002;

/// Signing domain, 10 ASCII bytes (§3.1). Prefixes every signed transaction preimage.
pub const SIGN_DOMAIN: &[u8] = b"popcorn-v1";

/// Receipt domain (§9.2). Lives inside the Borsh-encoded receipt payload.
pub const RECEIPT_DOMAIN: &str = "popcorn-receipt-v1";

/// The native token id: the all-zero identifier (§4.1).
pub const NATIVE_TOKEN: [u8; 32] = [0u8; 32];

// ---------------------------------------------------------------------------------------
// Batch limits
// ---------------------------------------------------------------------------------------

/// Maximum transactions per account per batch (§5.2, step 8).
pub const MAX_TX_PER_ACCOUNT_PER_BATCH: usize = 8;
/// Admission ceiling per batch (§9.2, resource bound).
pub const MAX_TX_PER_BATCH: usize = 10_000;
/// Maximum number of hops in a swap path (§4.3).
pub const MAX_PATH_LEN: usize = 4;
/// Wire-level ceiling on an encrypted blob (§4.1).
pub const MAX_BLOB_SIZE: usize = 2 * 1024;
/// Ceiling on the cleartext `Publish.data` field (§4.1).
pub const MAX_PUBLISH_SIZE: usize = 512;
/// Bytes of `Publish.data` included in the flat fee (§5.2).
pub const PUBLISH_FREE_BYTES: usize = 128;
/// Per-byte surcharge above `PUBLISH_FREE_BYTES` (§5.2).
pub const PUBLISH_BYTE_FEE: u128 = 50;
/// Flat fee per executed transaction, burned in full (§7.4).
pub const FEE_TX: u128 = 5_000;

/// The only admitted AMM fee tiers, in basis points (§11).
pub const FEE_TIERS: [u16; 3] = [5, 30, 100];
/// Supply ceiling for a user-created token (§7.3).
pub const MAX_SUPPLY: u128 = 1_000_000_000_000_000_000_000_000_000_000; // 10^30
/// Liquidity burned at a pair's first mint (§6).
pub const MINIMUM_LIQUIDITY: u128 = 1_000;

// ---------------------------------------------------------------------------------------
// Emission and staking
// ---------------------------------------------------------------------------------------

/// Fair launch: nothing is allocated at genesis (§7.1).
pub const GENESIS_SUPPLY: u128 = 0;
/// Emission of the first epoch, per batch (§7.2). 1 native, at 9 decimals.
pub const EMISSION_0: u128 = 1_000_000_000;
/// Batches per halving epoch (§7.2). ≈ 1 year at 3 s per batch.
pub const HALVING_INTERVAL: u64 = 10_512_000;
/// Staker share of emission, in basis points; the remainder goes to the foundation (§7.2).
pub const EMISSION_STAKER_BPS: u128 = 8_500;
/// Fixed-point precision of the staking accumulator (§8).
pub const PRECISION: u128 = 1_000_000_000_000_000_000; // 10^18

// ---------------------------------------------------------------------------------------
// Timelock and HTLC windows
// ---------------------------------------------------------------------------------------

/// Maximum lifetime of an HTLC lock, in rounds (§7.6). ≈ 30 days.
pub const HTLC_MAX_LIFETIME_ROUNDS: u64 = 864_000;
/// How far ahead a blob may target, in rounds (§3.5). ≈ 10 minutes.
pub const BLOB_ROUND_HORIZON: u64 = 200;

/// drand quicknet scheme identifier, pinned at genesis (§2.4).
pub const DRAND_SCHEME: &str = "bls-unchained-g1-rfc9380";
/// drand quicknet chain hash, pinned at genesis (§2.4).
pub const DRAND_CHAIN_HASH: &str =
    "52db9ba70e0cc0f6eaf7803dd07447a1f5477735fd3f661792ba94600c84e971";
/// Beacon sources tried in order (§11).
pub const DRAND_REMOTES: [&str; 2] = ["https://api.drand.sh", "https://drand.cloudflare.com"];

/// Returns true if `fee_bps` is one of the admitted tiers.
pub fn is_valid_fee_tier(fee_bps: u16) -> bool {
    let mut i = 0;
    while i < FEE_TIERS.len() {
        if FEE_TIERS[i] == fee_bps {
            return true;
        }
        i += 1;
    }
    false
}
