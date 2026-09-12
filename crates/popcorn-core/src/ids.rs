//! Domain-separated identifier derivation (SPEC.md §4.1).
//!
//! Every id is 32 bytes and every preimage starts with a distinct domain tag, so no two
//! namespaces can collide.

use crate::constants::NATIVE_TOKEN;

/// Domain tag bytes. They are part of consensus: changing one changes every derived id.
const TAG_TOKEN: u8 = 0x01;
const TAG_LP_TOKEN: u8 = 0x02;
const TAG_PAIR: u8 = 0x03;
const TAG_HTLC: u8 = 0x04;

/// `TokenId = blake3(0x01 || creator || LE64(nonce))`.
pub fn token_id(creator: &[u8; 32], nonce: u64) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    h.update(&[TAG_TOKEN]);
    h.update(creator);
    h.update(&nonce.to_le_bytes());
    *h.finalize().as_bytes()
}

/// `LpTokenId = blake3(0x02 || PairId)`.
pub fn lp_token_id(pair: &[u8; 32]) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    h.update(&[TAG_LP_TOKEN]);
    h.update(pair);
    *h.finalize().as_bytes()
}

/// `PairId = blake3(0x03 || token0 || token1 || LE16(fee_bps))` with `token0 < token1`.
///
/// The caller may pass the two sides in any order: canonical ordering happens here, so the
/// same unordered couple always yields the same pair.
pub fn pair_id(token_a: &[u8; 32], token_b: &[u8; 32], fee_bps: u16) -> [u8; 32] {
    let (token0, token1) = sort_pair(token_a, token_b);
    let mut h = blake3::Hasher::new();
    h.update(&[TAG_PAIR]);
    h.update(&token0);
    h.update(&token1);
    h.update(&fee_bps.to_le_bytes());
    *h.finalize().as_bytes()
}

/// `HtlcId = blake3(0x04 || sender || LE64(nonce))`.
pub fn htlc_id(sender: &[u8; 32], nonce: u64) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    h.update(&[TAG_HTLC]);
    h.update(sender);
    h.update(&nonce.to_le_bytes());
    *h.finalize().as_bytes()
}

/// Canonical side ordering for a pair: lexicographically smaller token first (§4.1).
pub fn sort_pair(token_a: &[u8; 32], token_b: &[u8; 32]) -> ([u8; 32], [u8; 32]) {
    if token_a <= token_b {
        (*token_a, *token_b)
    } else {
        (*token_b, *token_a)
    }
}

/// True for the native token id.
pub fn is_native(token: &[u8; 32]) -> bool {
    *token == NATIVE_TOKEN
}
