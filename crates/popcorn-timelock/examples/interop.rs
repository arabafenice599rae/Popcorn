//! The Rust half of the cross-language gate (SPEC.md §2.4).
//!
//! Same command surface as the Go and JS tools in `interop/`, so the harness can run every
//! encrypt/decrypt direction between the three implementations and compare bytes. Offline:
//! encryption needs the chain's public key, decryption only the round's signature.
//!
//!     cargo run -p popcorn-timelock --example interop -- encrypt <round> <plaintext-hex>
//!     cargo run -p popcorn-timelock --example interop -- decrypt <blob-hex> <signature-hex>
//!     cargo run -p popcorn-timelock --example interop -- profile <blob-hex> <round> <chain-hex>

use popcorn_timelock::{blob, profile, Beacon};

/// quicknet, pinned at genesis (SPEC.md §2.4).
const CHAIN_HASH_HEX: &str = "52db9ba70e0cc0f6eaf7803dd07447a1f5477735fd3f661792ba94600c84e971";
const PUBLIC_KEY_HEX: &str = "83cf0f2896adee7eb8b5f01fcad3912212c437e0073e911fb90022d3e760183c8c4b450b6a0a6c3ac6a5776a2d1064510d1fec758c921cc22b0e17e63aaf4bcb5ed66304de9cf809bd274ca73bab4af5a6e9c76a4bc09e76eae8991ef5ece45a";

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex input"))
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn chain_hash() -> [u8; 32] {
    unhex(CHAIN_HASH_HEX).try_into().expect("32 bytes")
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("");

    match command {
        "encrypt" => {
            let round: u64 = args[1].parse().expect("round");
            let plaintext = unhex(&args[2]);
            let out = blob::encrypt(&plaintext, &chain_hash(), &unhex(PUBLIC_KEY_HEX), round)
                .expect("encryption");
            println!("{}", hex(&out));
        }
        "decrypt" => {
            let blob_bytes = unhex(&args[1]);
            let signature = unhex(&args[2]);
            // The round is read from the header the blob itself carries, exactly as a
            // verifier would when re-deriving `unusable` from a mirror.
            let round = header_round(&blob_bytes).expect("a tlock header");
            let beacon = Beacon { round, signature };
            match blob::decrypt(&blob_bytes, &chain_hash(), &beacon) {
                Ok(plaintext) => println!("{}", hex(&plaintext)),
                Err(error) => {
                    eprintln!("error: {error:?}");
                    std::process::exit(1);
                }
            }
        }
        "profile" => {
            let blob_bytes = unhex(&args[1]);
            let round: u64 = args[2].parse().expect("round");
            let expected: [u8; 32] = unhex(&args[3]).try_into().expect("32 bytes");
            match profile::validate(&blob_bytes, round, &expected) {
                Ok(_) => println!("OK"),
                Err(error) => {
                    // The error NAME is the contract across implementations, not its text.
                    println!("{}", error_name(&error));
                    std::process::exit(2);
                }
            }
        }
        other => {
            eprintln!("unknown command `{other}`");
            std::process::exit(1);
        }
    }
}

/// Read the round out of the tlock stanza without decrypting anything.
fn header_round(blob_bytes: &[u8]) -> Option<u64> {
    let text = String::from_utf8_lossy(&blob_bytes[..blob_bytes.len().min(1024)]);
    for line in text.lines() {
        if let Some(stanza) = line.strip_prefix("-> tlock ") {
            return stanza.split(' ').next()?.parse().ok();
        }
    }
    None
}

/// Stable names for the rejection cases, so the harness can compare verdicts across
/// languages rather than comparing prose.
fn error_name(error: &profile::ProfileError) -> &'static str {
    use profile::ProfileError::*;
    match error {
        Empty => "Empty",
        Armored => "Armored",
        BadIntroLine => "BadIntroLine",
        CarriageReturn => "CarriageReturn",
        HeaderTooLarge(_) => "HeaderTooLarge",
        NoStanza => "NoStanza",
        MultipleTlockStanzas(_) => "MultipleTlockStanzas",
        ForeignStanza(_) => "ForeignStanza",
        MultipleGreaseStanzas(_) => "MultipleGreaseStanzas",
        WrongStanzaType(_) => "WrongStanzaType",
        MalformedStanzaArgs => "MalformedStanzaArgs",
        NonCanonicalRound => "NonCanonicalRound",
        RoundMismatch { .. } => "RoundMismatch",
        ChainHashMismatch => "ChainHashMismatch",
        NonCanonicalBase64 => "NonCanonicalBase64",
        MissingMac => "MissingMac",
        NoPayload => "NoPayload",
    }
}
