//! Offline BLS verification of a drand beacon (SPEC.md §3.3, §10).
//!
//! The producer only ever acts on a beacon that `drand_core` verified on fetch. But a
//! *verifier* replaying an exported block has no network and, until now, no way to check that
//! the `drand_signature` in a block is the real BLS signature of its round: it saw only
//! `drand_sig_hash == blake3(signature)`, which a malicious operator with the node key can
//! satisfy for a signature of their choosing — and so choose the shuffle seed. This closes
//! that: the quicknet chain info is pinned here, and verification runs against it with no
//! network, so "the ordering is a function of the beacon, never an operator choice" becomes
//! checkable by anyone holding only the blocks.
//!
//! The BLS check itself is `drand_core`'s audited path, reached through its public API; this
//! module only pins the chain info and adapts POPCORN's `(round, signature)` to it.

use sha2::{Digest, Sha256};

/// The pinned drand **quicknet** chain info (§2.4). These are the values `verify` needs — the
/// public key it checks the signature against and the scheme that fixes the hash-to-curve
/// domain — plus the identity fields, frozen so the document is self-contained. `hash` and
/// `public_key` match `DRAND_CHAIN_HASH` / the pinned key used everywhere else.
const QUICKNET_CHAIN_INFO: &str = r#"{
  "public_key": "83cf0f2896adee7eb8b5f01fcad3912212c437e0073e911fb90022d3e760183c8c4b450b6a0a6c3ac6a5776a2d1064510d1fec758c921cc22b0e17e63aaf4bcb5ed66304de9cf809bd274ca73bab4af5a6e9c76a4bc09e76eae8991ef5ece45a",
  "period": 3,
  "genesis_time": 1692803367,
  "hash": "52db9ba70e0cc0f6eaf7803dd07447a1f5477735fd3f661792ba94600c84e971",
  "groupHash": "f477d5c89f21a17c863a7f937c6a6d15859414d2be09cd448d4279af331c5d3e",
  "schemeID": "bls-unchained-g1-rfc9380",
  "metadata": { "beaconID": "quicknet" }
}"#;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Verify that `signature` is the genuine drand quicknet BLS signature for `round`, offline.
///
/// Returns `false` for any failure — a signature that does not verify, a malformed one, or a
/// chain info that will not parse — because to a verifier they are the same fact: this is not
/// the beacon of that round. There is no panic path: the input is attacker-controlled (it is
/// exactly the field a malicious operator would forge).
pub fn verify_beacon(round: u64, signature: &[u8]) -> bool {
    let Ok(info) = serde_json::from_str::<drand_core::chain::ChainInfo>(QUICKNET_CHAIN_INFO) else {
        return false;
    };

    // drand defines a round's randomness as sha256(signature); `verify` checks that too, so it
    // must be supplied. The security is the BLS check against the pinned public key underneath.
    let randomness = {
        let mut h = Sha256::new();
        h.update(signature);
        h.finalize().to_vec()
    };

    // The only public constructor for a beacon is deserialization; build the unchained shape
    // `verify` expects. `ApiBeacon` is untagged, so with no `previous_signature` it parses as
    // the unchained variant.
    let json = format!(
        r#"{{"round":{round},"randomness":"{}","signature":"{}"}}"#,
        hex(&randomness),
        hex(signature)
    );
    let Ok(beacon) = serde_json::from_str::<drand_core::beacon::ApiBeacon>(&json) else {
        return false;
    };
    if beacon.round() != round {
        return false;
    }
    beacon.verify(info).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The real quicknet beacon for round 1000, used throughout the interop and DoS vectors.
    const ROUND: u64 = 1000;
    const SIG: &str = "b44679b9a59af2ec876b1a6b1ad52ea9b1615fc3982b19576350f93447cb1125e342b73a8dd2bacbe47e4b6b63ed5e39";

    fn unhex(t: &str) -> Vec<u8> {
        (0..t.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&t[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn the_real_round_1000_beacon_verifies() {
        assert!(verify_beacon(ROUND, &unhex(SIG)));
    }

    #[test]
    fn a_forged_signature_is_rejected() {
        // A well-formed 48-byte G1 point that is not the round's signature.
        let mut forged = unhex(SIG);
        forged[0] ^= 0x01;
        assert!(!verify_beacon(ROUND, &forged));
    }

    #[test]
    fn the_right_signature_for_the_wrong_round_is_rejected() {
        // The seed an operator would want: a valid signature, but attributed to another round.
        assert!(!verify_beacon(ROUND + 1, &unhex(SIG)));
    }

    #[test]
    fn garbage_never_panics_and_is_rejected() {
        assert!(!verify_beacon(ROUND, &[]));
        assert!(!verify_beacon(ROUND, &[0u8; 48]));
        assert!(!verify_beacon(ROUND, &[7u8; 96]));
    }
}
