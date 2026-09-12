//! Generate the "abnormal termination" regression vector (SPEC.md §3.6, §5.1).
//!
//! Searches base64-clean corruptions of a valid blob's tlock stanza for one that passes
//! POPCORN-TLOCK-AGE-V1 yet drives the pinned tlock primitive to terminate abnormally. With
//! the containment in `blob::decrypt`, that outcome is now `TimelockError::Aborted` — a
//! defined `unusable`, not a crash. The blob is frozen as a cross-language rejection vector.
//!
//!     cargo run -p popcorn-timelock --example make_halt_vector -- vectors/halt.json
use popcorn_timelock::{blob, Beacon};

const CHAIN: &str = "52db9ba70e0cc0f6eaf7803dd07447a1f5477735fd3f661792ba94600c84e971";
const PK: &str = "83cf0f2896adee7eb8b5f01fcad3912212c437e0073e911fb90022d3e760183c8c4b450b6a0a6c3ac6a5776a2d1064510d1fec758c921cc22b0e17e63aaf4bcb5ed66304de9cf809bd274ca73bab4af5a6e9c76a4bc09e76eae8991ef5ece45a";
const SIG: &str = "b44679b9a59af2ec876b1a6b1ad52ea9b1615fc3982b19576350f93447cb1125e342b73a8dd2bacbe47e4b6b63ed5e39";
const ROUND: u64 = 1000;

fn unhex(t: &str) -> Vec<u8> {
    (0..t.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&t[i..i + 2], 16).unwrap())
        .collect()
}
fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
const B64: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "vectors/halt.json".to_string());
    let chain: [u8; 32] = unhex(CHAIN).try_into().unwrap();
    let pk = unhex(PK);
    let beacon = Beacon {
        round: ROUND,
        signature: unhex(SIG),
    };
    let valid = blob::encrypt(b"popcorn-halt-vector", &chain, &pk, ROUND).unwrap();

    let text = String::from_utf8_lossy(&valid).to_string();
    let lines: Vec<String> = text.lines().map(String::from).collect();
    let idx = lines
        .iter()
        .position(|l| l.starts_with("-> tlock"))
        .unwrap()
        + 1;

    // Deterministic scan: first corruption (lowest position, then B64 order) that is
    // profile-valid and makes decrypt report Aborted. With the fix in place this never panics.
    for pos in 0..lines[idx].len() {
        for &c in B64 {
            let mut ls = lines.clone();
            let bytes = unsafe { ls[idx].as_bytes_mut() };
            if bytes[pos] == c {
                continue;
            }
            bytes[pos] = c;
            let crafted = ls.join("\n").into_bytes();
            if popcorn_timelock::profile::validate(&crafted, ROUND, &chain).is_err() {
                continue;
            }
            if let Err(popcorn_timelock::TimelockError::Aborted) =
                blob::decrypt(&crafted, &chain, &beacon)
            {
                let json = format!(
                    "{{\n  \"note\": \"profile-valid blob whose decryption terminates abnormally in the pinned tlock; a total decryptor classifies it unusable (SPEC.md 3.6, 5.1)\",\n  \"round\": {ROUND},\n  \"chain_hash\": \"{CHAIN}\",\n  \"beacon_signature\": \"{SIG}\",\n  \"blob\": \"{}\",\n  \"blob_hash\": \"{}\",\n  \"expected\": \"unusable\"\n}}\n",
                    hex(&crafted), hex(blake3::hash(&crafted).as_bytes()));
                std::fs::write(&out, json).unwrap();
                println!(
                    "wrote {out}: {}-byte blob, hash {}",
                    crafted.len(),
                    hex(blake3::hash(&crafted).as_bytes())
                );
                return;
            }
        }
    }
    eprintln!("no abnormal-termination corruption found in the scan window");
    std::process::exit(1);
}
