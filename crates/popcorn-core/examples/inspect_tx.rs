//! Parse a Borsh `SignedTx` from hex and print what consensus would make of it.
//!
//! This is the Rust half of the browser-path gate (`web/test/browser-path.sh`): the page
//! builds and signs a transaction with its own Borsh and BLAKE3, and this tool says whether
//! the node agrees about the bytes, the identity and the signature. A browser that encodes a
//! payload differently does not fail visibly — it produces a signature over bytes nobody
//! asked for — so the disagreement has to be made loud somewhere, and this is where.
//!
//!     cargo run -p popcorn-core --example inspect_tx -- <signed-tx-hex>

use popcorn_core::crypto::{signing_hash, verify_signature};
use popcorn_core::types::SignedTx;

fn main() {
    let Some(hex) = std::env::args().nth(1) else {
        eprintln!("usage: inspect_tx <signed-tx-hex>");
        std::process::exit(1);
    };
    let bytes = decode_hex(hex.trim()).unwrap_or_else(|| {
        eprintln!("argument is not hexadecimal");
        std::process::exit(1);
    });

    let tx: SignedTx = match borsh::from_slice(&bytes) {
        Ok(tx) => tx,
        Err(error) => {
            eprintln!("not a SignedTx: {error}");
            std::process::exit(2);
        }
    };

    // Re-encoding must reproduce the input exactly: Borsh is canonical here, and a payload
    // that round-trips to different bytes would have two tx ids.
    let reencoded = borsh::to_vec(&tx).expect("a parsed transaction re-encodes");
    let canonical = reencoded == bytes;

    println!("{{");
    println!("  \"canonical\": {canonical},");
    println!("  \"nonce\": {},", tx.payload.nonce);
    println!("  \"target_round\": {},", tx.payload.target_round);
    println!("  \"action\": \"{}\",", action_name(&tx.payload.action));
    println!(
        "  \"signer_pubkey\": \"{}\",",
        encode_hex(&tx.signer_pubkey)
    );
    println!("  \"account\": \"{}\",", encode_hex(&tx.signer()));
    println!("  \"tx_id\": \"{}\",", encode_hex(&tx.tx_id()));
    println!(
        "  \"signing_hash\": \"{}\",",
        encode_hex(&signing_hash(&tx.payload))
    );
    println!(
        "  \"signature_valid\": {}",
        verify_signature(&tx.signer_pubkey, &signing_hash(&tx.payload), &tx.signature)
    );
    println!("}}");

    if !canonical {
        std::process::exit(3);
    }
}

fn action_name(action: &popcorn_core::types::Action) -> &'static str {
    use popcorn_core::types::Action::*;
    match action {
        Transfer { .. } => "Transfer",
        CreateToken { .. } => "CreateToken",
        CreatePair { .. } => "CreatePair",
        AddLiquidity { .. } => "AddLiquidity",
        RemoveLiquidity { .. } => "RemoveLiquidity",
        SwapExactIn { .. } => "SwapExactIn",
        SwapExactOut { .. } => "SwapExactOut",
        Publish { .. } => "Publish",
        HtlcLock { .. } => "HtlcLock",
        HtlcClaim { .. } => "HtlcClaim",
        HtlcRefund { .. } => "HtlcRefund",
        Stake { .. } => "Stake",
        Unstake { .. } => "Unstake",
        ClaimRewards {} => "ClaimRewards",
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn decode_hex(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 {
        return None;
    }
    (0..text.len() / 2)
        .map(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).ok())
        .collect()
}
