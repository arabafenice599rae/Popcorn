//! POPCORN-TLOCK-AGE-V1 acceptance and rejection (SPEC.md §3.6).
//!
//! The acceptance policy is normative, so the rejection cases matter as much as the
//! round trip: two implementations that disagree about which blobs are `unusable` disagree
//! about which transactions exist.

use popcorn_timelock::profile::{self, ProfileError};

const CHAIN: [u8; 32] = [
    0x52, 0xdb, 0x9b, 0xa7, 0x0e, 0x0c, 0xc0, 0xf6, 0xea, 0xf7, 0x80, 0x3d, 0xd0, 0x74, 0x47, 0xa1,
    0xf5, 0x47, 0x77, 0x35, 0xfd, 0x3f, 0x66, 0x17, 0x92, 0xba, 0x94, 0x60, 0x0c, 0x84, 0xe9, 0x71,
];
const CHAIN_HEX: &str = "52db9ba70e0cc0f6eaf7803dd07447a1f5477735fd3f661792ba94600c84e971";

/// A minimal conforming header, with a payload byte after it.
fn conforming(round: u64) -> Vec<u8> {
    let mut blob = format!("age-encryption.org/v1\n-> tlock {round} {CHAIN_HEX}\nAAAA\n--- AAAA\n")
        .into_bytes();
    blob.push(0x00);
    blob
}

#[test]
fn a_conforming_header_is_accepted() {
    let header = profile::validate(&conforming(1000), 1000, &CHAIN).unwrap();
    assert_eq!(header.round, 1000);
    assert_eq!(header.chain_hash, CHAIN);
}

/// Armor is a second encoding of the same ciphertext, so the same transaction would have two
/// blob hashes and two manifest entries.
#[test]
fn armor_is_refused() {
    let armored = b"-----BEGIN AGE ENCRYPTED FILE-----\nYWJj\n-----END AGE ENCRYPTED FILE-----\n";
    assert_eq!(
        profile::validate(armored, 1000, &CHAIN),
        Err(ProfileError::Armored)
    );
}

#[test]
fn only_age_v1_is_accepted() {
    let blob = conforming(1000);
    let wrong = String::from_utf8(blob.clone())
        .unwrap()
        .replace("age-encryption.org/v1", "age-encryption.org/v2");
    assert_eq!(
        profile::validate(wrong.as_bytes(), 1000, &CHAIN),
        Err(ProfileError::BadIntroLine)
    );
}

/// A blob aimed at another round is not a late transaction: it is not a transaction here.
#[test]
fn the_round_must_match_the_batch() {
    assert_eq!(
        profile::validate(&conforming(999), 1000, &CHAIN),
        Err(ProfileError::RoundMismatch {
            expected: 1000,
            found: 999
        })
    );
}

/// "007" and "7" would be two encodings of one round, hence two hashes for one submission.
#[test]
fn round_numbers_must_be_canonical_decimals() {
    let blob = format!("age-encryption.org/v1\n-> tlock 01000 {CHAIN_HEX}\nAAAA\n--- AAAA\n\0");
    assert_eq!(
        profile::validate(blob.as_bytes(), 1000, &CHAIN),
        Err(ProfileError::NonCanonicalRound)
    );
}

/// The chain hash must be the pinned one, in lowercase hex.
#[test]
fn the_chain_hash_is_pinned_and_lowercase() {
    let other = "0000000000000000000000000000000000000000000000000000000000000000";
    let blob = format!("age-encryption.org/v1\n-> tlock 1000 {other}\nAAAA\n--- AAAA\n\0");
    assert_eq!(
        profile::validate(blob.as_bytes(), 1000, &CHAIN),
        Err(ProfileError::ChainHashMismatch)
    );

    let upper = CHAIN_HEX.to_uppercase();
    let blob = format!("age-encryption.org/v1\n-> tlock 1000 {upper}\nAAAA\n--- AAAA\n\0");
    assert_eq!(
        profile::validate(blob.as_bytes(), 1000, &CHAIN),
        Err(ProfileError::ChainHashMismatch)
    );
}

/// A second recipient stanza could be a decryption path for somebody other than the round.
#[test]
fn a_foreign_recipient_stanza_is_refused() {
    let blob = format!(
        "age-encryption.org/v1\n-> tlock 1000 {CHAIN_HEX}\nAAAA\n-> X25519 abc\nAAAA\n--- AAAA\n\0"
    );
    assert_eq!(
        profile::validate(blob.as_bytes(), 1000, &CHAIN),
        Err(ProfileError::ForeignStanza("X25519".to_string()))
    );
}

/// Two tlock stanzas leave the recipient ambiguous.
#[test]
fn two_tlock_stanzas_are_refused() {
    let blob = format!(
        "age-encryption.org/v1\n-> tlock 1000 {CHAIN_HEX}\nAAAA\n-> tlock 1000 {CHAIN_HEX}\nAAAA\n--- AAAA\n\0"
    );
    assert_eq!(
        profile::validate(blob.as_bytes(), 1000, &CHAIN),
        Err(ProfileError::MultipleTlockStanzas(2))
    );
}

/// age writes a randomized grease stanza into every header, so refusing it would reject every
/// blob the Rust client produces. One is tolerated; two are not (§14.12).
#[test]
fn one_grease_stanza_is_tolerated_and_two_are_not() {
    let blob = format!(
        "age-encryption.org/v1\n-> tlock 1000 {CHAIN_HEX}\nAAAA\n-> Ab3-grease x y\nAAAA\n--- AAAA\n\0"
    );
    assert!(profile::validate(blob.as_bytes(), 1000, &CHAIN).is_ok());

    let blob = format!(
        "age-encryption.org/v1\n-> tlock 1000 {CHAIN_HEX}\nAAAA\n-> Ab3-grease\nAAAA\n-> Zz9-grease\nAAAA\n--- AAAA\n\0"
    );
    assert_eq!(
        profile::validate(blob.as_bytes(), 1000, &CHAIN),
        Err(ProfileError::MultipleGreaseStanzas(2))
    );
}

/// A header without a tlock stanza has no timelock at all.
#[test]
fn a_header_without_a_tlock_stanza_is_refused() {
    let blob = "age-encryption.org/v1\n-> Ab3-grease\nAAAA\n--- AAAA\n\0";
    assert_eq!(
        profile::validate(blob.as_bytes(), 1000, &CHAIN),
        Err(ProfileError::WrongStanzaType(String::new()))
    );
}

/// CRLF would be a second encoding of the same header.
#[test]
fn carriage_returns_are_refused() {
    let blob = format!("age-encryption.org/v1\r\n-> tlock 1000 {CHAIN_HEX}\nAAAA\n--- AAAA\n\0");
    assert_eq!(
        profile::validate(blob.as_bytes(), 1000, &CHAIN),
        Err(ProfileError::CarriageReturn)
    );
}

/// The header cap bounds how much attacker-chosen data rides along with a blob.
#[test]
fn oversized_headers_are_refused() {
    let padding = "A".repeat(profile::MAX_HEADER_SIZE);
    let blob = format!("age-encryption.org/v1\n-> tlock 1000 {CHAIN_HEX}\n{padding}\n--- AAAA\n\0");
    assert!(matches!(
        profile::validate(blob.as_bytes(), 1000, &CHAIN),
        Err(ProfileError::HeaderTooLarge(_))
    ));
}

/// Truncated and empty inputs are `unusable`, not panics: these bytes come from the wire.
#[test]
fn truncated_input_is_refused_without_panicking() {
    assert_eq!(
        profile::validate(b"", 1000, &CHAIN),
        Err(ProfileError::Empty)
    );
    assert_eq!(
        profile::validate(b"age-encryption.org/v1\n", 1000, &CHAIN),
        Err(ProfileError::MissingMac)
    );

    // Every prefix of a valid blob must be refused rather than crash.
    let blob = conforming(1000);
    for cut in 0..blob.len() {
        let _ = profile::validate(&blob[..cut], 1000, &CHAIN);
    }
    // Every single-byte corruption likewise.
    for index in 0..blob.len() {
        let mut corrupted = blob.clone();
        corrupted[index] ^= 0xff;
        let _ = profile::validate(&corrupted, 1000, &CHAIN);
    }
}

/// A header with nothing after it carries no payload, so it carries no transaction.
#[test]
fn a_payloadless_header_is_refused() {
    let blob = format!("age-encryption.org/v1\n-> tlock 1000 {CHAIN_HEX}\nAAAA\n--- AAAA\n");
    assert_eq!(
        profile::validate(blob.as_bytes(), 1000, &CHAIN),
        Err(ProfileError::NoPayload)
    );
}
