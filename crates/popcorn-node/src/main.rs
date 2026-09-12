//! The POPCORN binary: `genesis`, `node`, `verify`, `keygen`, `account`.
//!
//! Argument parsing is hand-rolled on purpose. §2 fixes the dependency list, and a CLI parser
//! is not a reason to widen a list that the specification treats as part of the audit surface.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use popcorn_core::constants::{DRAND_REMOTES, GENESIS_SUPPLY};
use popcorn_core::genesis::GenesisConfig;
use popcorn_node::api::{router, NodeApi};
use popcorn_node::chain::Chain;
use popcorn_node::cors::CorsPolicy;
use popcorn_node::encoding::to_hex;
use popcorn_node::mempool::Mempool;
use popcorn_node::producer::Producer;
use popcorn_node::verify::verify_chain;
use popcorn_node::{keys, storage::Storage};
use popcorn_timelock::{DrandTimelock, TimelockProvider};
use tokio::sync::{broadcast, Mutex};

const USAGE: &str = "\
popcorn — a single-operator deterministic/verifiable execution chain

USAGE:
    popcorn keygen --out <FILE>
    popcorn genesis --data <DIR> --node-key <FILE> --foundation-key <FILE> [--drand-round <N>]
    popcorn node    --data <DIR> --node-key <FILE> [--listen <ADDR>] [--no-web]
                    [--cors '*' | <ORIGIN>[,<ORIGIN>...]]
    popcorn verify  --data <DIR> | --node <URL> [--audit-collection]
    popcorn account --key <FILE>
    popcorn submit  --key <FILE> --node <URL> <ACTION>

ACTIONS for `submit`:
    transfer         --to <ACCOUNT_HEX> --amount <N> [--token <TOKEN_HEX>]
    stake            --amount <N>
    unstake          --amount <N>
    claim
    publish          --topic <TOPIC_HEX> --data <HEX>
    create-token     --name <NAME> --supply <N>
    create-pair      --token-a <HEX> --token-b <HEX> --fee-bps <5|30|100>
    add-liquidity    --pair <HEX> --amount0 <N> --amount1 <N> [--min0 <N>] [--min1 <N>]
    remove-liquidity --pair <HEX> --lp <N> [--min0 <N>] [--min1 <N>]
    swap-in          --path <HEX[,HEX...]> --token-in <HEX> --amount-in <N> [--min-out <N>]
    swap-out         --path <HEX[,HEX...]> --token-in <HEX> --amount-out <N> --max-in <N>
    htlc-lock        --to <ACCOUNT_HEX> --amount <N> --expiry-in <ROUNDS>
                     (--preimage <HEX32> | --hashlock <HEX32>) [--token <HEX>]
    htlc-claim       --htlc-id <HEX> --preimage <HEX32>
    htlc-refund      --htlc-id <HEX>

Identifiers derive from the signer and the nonce, so `create-token` and `create-pair`
print the id their transaction will produce if it executes.

Keys are distinct by design: the node key signs blocks and controls no funds, the
foundation key holds value and signs no blocks. Only the node key is ever loaded by
`popcorn node`.
";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("help");

    let result = match command {
        "keygen" => keygen(&args),
        "genesis" => genesis(&args),
        "node" => node(&args),
        "verify" => verify(&args),
        "account" => account(&args),
        "submit" => submit(&args),
        "help" | "--help" | "-h" => {
            print!("{USAGE}");
            Ok(())
        }
        other => Err(format!("unknown command `{other}`\n\n{USAGE}")),
    };

    if let Err(message) = result {
        eprintln!("error: {message}");
        std::process::exit(1);
    }
}

/// `--name value` lookup.
fn flag(args: &[String], name: &str) -> Option<String> {
    let position = args.iter().position(|arg| arg == name)?;
    args.get(position + 1).cloned()
}

fn required(args: &[String], name: &str) -> Result<String, String> {
    flag(args, name).ok_or_else(|| format!("missing {name}\n\n{USAGE}"))
}

fn keygen(args: &[String]) -> Result<(), String> {
    let out = PathBuf::from(required(args, "--out")?);
    if out.exists() {
        // Overwriting a key destroys funds or an identity; refuse rather than ask.
        return Err(format!("{} already exists", out.display()));
    }
    let key = keys::generate();
    keys::save(&key, &out).map_err(|e| e.to_string())?;
    println!("wrote {}", out.display());
    println!("public key: {}", to_hex(&key.verifying_key().to_bytes()));
    println!(
        "account id: {}",
        to_hex(&popcorn_core::crypto::account_id_from_pubkey(
            &key.verifying_key().to_bytes()
        ))
    );
    Ok(())
}

fn account(args: &[String]) -> Result<(), String> {
    let path = PathBuf::from(required(args, "--key")?);
    let pubkey = keys::public_of(&path).map_err(|e| e.to_string())?;
    println!("public key: {}", to_hex(&pubkey));
    println!(
        "account id: {}",
        to_hex(&popcorn_core::crypto::account_id_from_pubkey(&pubkey))
    );
    Ok(())
}

fn genesis(args: &[String]) -> Result<(), String> {
    let data = PathBuf::from(required(args, "--data")?);
    let node_key = PathBuf::from(required(args, "--node-key")?);
    let foundation_key = PathBuf::from(required(args, "--foundation-key")?);

    std::fs::create_dir_all(&data).map_err(|e| e.to_string())?;

    let node_pubkey = keys::public_of(&node_key).map_err(|e| e.to_string())?;
    let foundation_pubkey = keys::public_of(&foundation_key).map_err(|e| e.to_string())?;
    if node_pubkey == foundation_pubkey {
        return Err("the node key and the foundation key must be distinct (§1)".to_string());
    }

    // Default to the round current at genesis time; the mapping round(h) = G + h − 1 starts
    // from whatever is stamped here and never skips afterwards (§3.4).
    let genesis_drand_round = match flag(args, "--drand-round") {
        Some(value) => value
            .parse()
            .map_err(|_| "--drand-round must be a number")?,
        None => {
            let timelock = DrandTimelock::connect(&DRAND_REMOTES).map_err(|e| format!("{e:?}"))?;
            timelock.round_for_time(popcorn_node::producer::now_seconds()) + 2
        }
    };

    let config = GenesisConfig {
        genesis_drand_round,
        node_pubkey,
        foundation_pubkey,
    };
    let chain = Chain::initialize(&chain_path(&data), config).map_err(|e| e.to_string())?;

    println!("initialized chain at {}", data.display());
    println!("genesis drand round: {genesis_drand_round}");
    println!("genesis supply:      {GENESIS_SUPPLY} (fair launch: nothing is allocated)");
    println!("node pubkey:         {}", to_hex(&node_pubkey));
    println!(
        "foundation account:  {}",
        to_hex(&chain.config().foundation_account())
    );
    println!("genesis state root:  {}", to_hex(&chain.head().state_root));
    Ok(())
}

fn node(args: &[String]) -> Result<(), String> {
    let data = PathBuf::from(required(args, "--data")?);
    let node_key_path = PathBuf::from(required(args, "--node-key")?);
    let listen = flag(args, "--listen").unwrap_or_else(|| "127.0.0.1:8080".to_string());
    // The explorer and wallet are served from the node itself, so the page is same-origin
    // with the API it signs against. `--no-web` is for operators who want the endpoints
    // only; it changes nothing a verifier depends on (§13.3).
    let serve_web = !args.iter().any(|arg| arg == "--no-web");
    // Third-party front ends: off unless asked for. The page this node serves is same-origin
    // and needs nothing here; the flag is for somebody else's explorer or wallet, hosted
    // elsewhere, whose browser would otherwise refuse to read this API at all. It is not a
    // security boundary — there is no authorization to protect — see `cors.rs`.
    let cors = match flag(args, "--cors") {
        Some(value) => Some(CorsPolicy::parse(&value)?),
        None => None,
    };

    let node_key = keys::load(&node_key_path).map_err(|e| e.to_string())?;
    let chain = Chain::open(&chain_path(&data)).map_err(|e| e.to_string())?;

    if chain.config().node_pubkey != node_key.verifying_key().to_bytes() {
        return Err("this key is not the node key stamped into genesis".to_string());
    }

    let timelock = DrandTimelock::connect(&DRAND_REMOTES).map_err(|e| format!("{e:?}"))?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;

    runtime.block_on(async move {
        let chain = Arc::new(Mutex::new(chain));
        let mempool = Arc::new(Mempool::new());
        let timelock: Arc<dyn TimelockProvider> = Arc::new(timelock);
        let (blocks, _) = broadcast::channel(256);

        let api = Arc::new(NodeApi {
            chain: Arc::clone(&chain),
            mempool: Arc::clone(&mempool),
            timelock: Arc::clone(&timelock),
            node_key: node_key.clone(),
            blocks: blocks.clone(),
        });

        let producer = Arc::new(Producer {
            chain: Arc::clone(&chain),
            mempool: Arc::clone(&mempool),
            timelock: Arc::clone(&timelock),
            node_key,
            blocks,
        });

        {
            let chain = chain.lock().await;
            println!(
                "head {} · next round {} · listening on {listen}",
                chain.head().height,
                chain.next_round()
            );
            if serve_web {
                println!("explorer and wallet: http://{listen}/");
            }
            if let Some(policy) = &cors {
                println!("cross-origin reads allowed from: {}", policy.describe());
            }
        }

        // One batch per drand round (§11).
        tokio::spawn(Arc::clone(&producer).run(Duration::from_secs(3)));

        let listener = tokio::net::TcpListener::bind(&listen)
            .await
            .map_err(|e| e.to_string())?;
        axum::serve(listener, router(api, serve_web, cors))
            .with_graceful_shutdown(async {
                let _ = tokio::signal::ctrl_c().await;
                println!("\nshutting down");
            })
            .await
            .map_err(|e| e.to_string())
    })
}

/// Replay a chain and check every commitment it makes.
///
/// Two sources, and the remote one is the point: §10 describes a verifier that downloads
/// `/chain/export` and re-executes, not one that reads the operator's files. A third party
/// has no access to the data directory — and does not need it, because transactions travel in
/// the clear inside blocks.
fn verify(args: &[String]) -> Result<(), String> {
    let (config, blocks) = match (flag(args, "--node"), flag(args, "--data")) {
        (Some(url), _) => fetch_chain(&url)?,
        (None, Some(data)) => {
            let storage = Storage::open(&chain_path(&PathBuf::from(data))).map_err(|e| {
                format!(
                    "{e}\n\nA running node holds this database. Either stop it, or verify over \
                     HTTP with `popcorn verify --node <URL>`, which is what a third party \
                     would do."
                )
            })?;
            let config = storage
                .genesis_config()
                .map_err(|e| e.to_string())?
                .ok_or("chain is not initialized")?;
            let blocks = storage.blocks_from(0).map_err(|e| e.to_string())?;
            (config, blocks)
        }
        (None, None) => return Err(format!("pass --node <URL> or --data <DIR>\n\n{USAGE}")),
    };

    let report = verify_chain(&config, &blocks);
    println!("replayed {} blocks", report.blocks_checked);

    // The collection audit is the other half of §10, and it needs what replay does not: the
    // blobs themselves. Only a node or a mirror can serve those, so it is opt-in and only
    // over HTTP — state replay stays self-contained, and saying which is which matters.
    let mut audit_divergences = Vec::new();
    if args.iter().any(|arg| arg == "--audit-collection") {
        match flag(args, "--node") {
            Some(url) => {
                let (audited, found) = audit_collection_over_http(&url, &blocks)?;
                println!("audited {audited} manifested blobs against the beacon in each block");
                audit_divergences = found;
            }
            None => {
                return Err(
                    "--audit-collection needs --node <URL>: the blobs live on the \
                            node or a mirror, not in the block stream"
                        .to_string(),
                )
            }
        }
    }

    if report.is_clean() && audit_divergences.is_empty() {
        let global = &report.final_state.global;
        println!("every state root, root and monetary invariant matches");
        println!("  emitted:   {}", global.native_emitted);
        println!("  burned:    {}", global.native_burned);
        println!("  staked:    {}", global.total_staked);
        println!("  reserved:  {}", global.staking_reserved);
        println!("  state root: {}", to_hex(&report.final_state.state_root()));
        Ok(())
    } else {
        for divergence in report.divergences.iter().chain(&audit_divergences) {
            eprintln!("  {divergence}");
        }
        Err(format!(
            "{} divergences — this is cryptographic proof of incorrectness",
            report.divergences.len() + audit_divergences.len()
        ))
    }
}

/// Re-derive `unusable` for every block from the blobs the node serves (§5.1, §10).
///
/// This is the check a false `unusable` claim cannot survive: anyone holding the blob and the
/// round's beacon can decrypt it themselves. Withholding a manifested blob is a finding too —
/// the audit cannot be performed, and that is visible obstruction rather than a pass.
fn audit_collection_over_http(
    url: &str,
    blocks: &[popcorn_core::types::Block],
) -> Result<(usize, Vec<popcorn_node::verify::Divergence>), String> {
    use popcorn_node::client;
    use popcorn_node::encoding::{from_base64, hex32};
    use popcorn_node::verify::audit_collection;
    use std::collections::BTreeMap;

    let base = client::Url::parse(url)?;
    let chain_hash =
        hex32(popcorn_core::constants::DRAND_CHAIN_HASH).ok_or("pinned chain hash is malformed")?;

    // Fetch every manifested blob once, then audit from the local copy.
    let mut blobs: BTreeMap<[u8; 32], Vec<u8>> = BTreeMap::new();
    let mut audited = 0usize;
    for block in blocks {
        for hash in &block.blob_manifest {
            if blobs.contains_key(hash) {
                continue;
            }
            audited += 1;
            let path = format!("/blob/{}", to_hex(hash));
            if let Ok(response) = client::get(&base.join(&path)) {
                if let Some(encoded) = response["blob"].as_str() {
                    if let Some(bytes) = from_base64(encoded) {
                        blobs.insert(*hash, bytes);
                    }
                }
            }
        }
    }

    let mut divergences = Vec::new();
    for block in blocks {
        divergences.extend(audit_collection(block, &chain_hash, &|hash| {
            blobs.get(hash).cloned()
        }));
    }
    Ok((audited, divergences))
}

/// Build, sign, timelock-encrypt and submit one transaction.
///
/// This is the bot flow of §3.2 in miniature: the wallet signs, the client encrypts toward a
/// future round, and the node answers with a receipt it cannot take back. Obtaining that
/// receipt before the round's deadline is the only per-blob protection the model offers
/// (§9.2), so the receipt is printed, not swallowed.
fn submit(args: &[String]) -> Result<(), String> {
    use popcorn_core::types::{Action, SignedTx, TxPayload};
    use popcorn_node::client;
    use popcorn_node::encoding::to_base64;

    let key_path = PathBuf::from(required(args, "--key")?);
    let key = keys::load(&key_path).map_err(|e| e.to_string())?;
    let pubkey = key.verifying_key().to_bytes();
    let signer = popcorn_core::crypto::account_id_from_pubkey(&pubkey);

    let node_url = flag(args, "--node").unwrap_or_else(|| "http://127.0.0.1:8080".to_string());
    let base = client::Url::parse(&node_url)?;

    let action = parse_action(args)?;

    // The next usable nonce is whatever the chain says was last executed, plus one.
    let account = client::get(&base.join(&format!("/account/{}", to_hex(&signer))))
        .map_err(|e| format!("this account cannot transact yet: {e}"))?;
    let nonce = account["nonce"].as_u64().unwrap_or(0) + 1;

    // Target a round far enough ahead that the blob arrives before collection closes.
    let head = client::get(&base.join("/head"))?;
    let current_round = head["drand_round"].as_u64().unwrap_or(0);
    let lead: u64 = flag(args, "--lead")
        .map(|v| v.parse().unwrap_or(2))
        .unwrap_or(2);
    let target_round = current_round + lead;

    // `htlc-lock` carries its expiry as a number of rounds ahead; resolve it now that the
    // target round is settled.
    let action = match action {
        Action::HtlcLock {
            to,
            token,
            amount,
            hashlock,
            expiry_round,
        } => Action::HtlcLock {
            to,
            token,
            amount,
            hashlock,
            expiry_round: target_round + expiry_round,
        },
        other => other,
    };

    let payload = TxPayload {
        nonce,
        target_round,
        action,
    };
    let tx = SignedTx {
        signature: popcorn_core::crypto::sign_payload(&key, &payload),
        payload,
        signer_pubkey: pubkey,
    };
    let encoded = borsh::to_vec(&tx).map_err(|e| e.to_string())?;

    let timelock = DrandTimelock::connect(&DRAND_REMOTES).map_err(|e| format!("{e:?}"))?;
    let blob = timelock
        .encrypt(&encoded, target_round)
        .map_err(|e| format!("{e:?}"))?;
    if blob.len() > popcorn_core::constants::MAX_BLOB_SIZE {
        return Err(format!(
            "blob is {} bytes, over the {} byte wire limit",
            blob.len(),
            popcorn_core::constants::MAX_BLOB_SIZE
        ));
    }

    let receipt = client::post(
        &base.join("/tx"),
        &serde_json::json!({ "blob": to_base64(&blob), "target_round": target_round }),
    )?;

    // Derived ids, printed before the wait: a caller cannot look up a token that does not
    // exist yet, and these are a pure function of the signer and the nonce (§4.1).
    match &tx.payload.action {
        Action::CreateToken { .. } => println!(
            "token id:     {}",
            to_hex(&popcorn_core::ids::token_id(&signer, nonce))
        ),
        Action::CreatePair {
            token_a,
            token_b,
            fee_bps,
        } => println!(
            "pair id:      {}",
            to_hex(&popcorn_core::ids::pair_id(token_a, token_b, *fee_bps))
        ),
        Action::HtlcLock { .. } => println!(
            "htlc id:      {}",
            to_hex(&popcorn_core::ids::htlc_id(&signer, nonce))
        ),
        _ => {}
    }
    println!("tx_id:        {}", to_hex(&tx.tx_id()));
    println!("target round: {target_round} (head is at {current_round})");
    println!(
        "blob hash:    {}",
        receipt["blob_hash"].as_str().unwrap_or("?")
    );
    println!(
        "receipt:      {}",
        receipt["signature"].as_str().unwrap_or("?")
    );
    println!(
        "\nKeep this receipt. If the blob never appears in the manifest of round {target_round},"
    );
    println!(
        "the receipt and that block are two signatures by the same node contradicting each other."
    );
    Ok(())
}

fn parse_action(args: &[String]) -> Result<popcorn_core::types::Action, String> {
    use popcorn_core::types::Action;
    use popcorn_node::encoding::{from_hex, hex32};

    let amount = || -> Result<u128, String> {
        required(args, "--amount")?
            .parse()
            .map_err(|_| "--amount must be a whole number".to_string())
    };

    // The action is the first bare word after the command, skipping `--flag value` pairs:
    // without skipping the values, a key path would be read as the action.
    let mut kind = None;
    let mut index = 1;
    while index < args.len() {
        if args[index].starts_with("--") {
            index += 2;
        } else {
            kind = Some(args[index].as_str());
            break;
        }
    }
    let kind = kind.ok_or_else(|| format!("missing action\n\n{USAGE}"))?;

    match kind {
        "transfer" => {
            let to = hex32(&required(args, "--to")?).ok_or("--to must be 32 hex bytes")?;
            let token = match flag(args, "--token") {
                Some(value) => hex32(&value).ok_or("--token must be 32 hex bytes")?,
                None => popcorn_core::constants::NATIVE_TOKEN,
            };
            Ok(Action::Transfer {
                token,
                to,
                amount: amount()?,
            })
        }
        "create-token" => {
            let raw = required(args, "--name")?;
            if raw.len() > 16 || !raw.bytes().all(|b| (0x20..=0x7E).contains(&b)) {
                return Err("--name must be at most 16 printable ASCII bytes".to_string());
            }
            // Right zero-padded, which is the canonical form (§14.9).
            let mut name = [0u8; 16];
            name[..raw.len()].copy_from_slice(raw.as_bytes());
            Ok(Action::CreateToken {
                name,
                supply: required(args, "--supply")?
                    .parse()
                    .map_err(|_| "--supply must be a whole number".to_string())?,
            })
        }
        "create-pair" => Ok(Action::CreatePair {
            token_a: token_arg(args, "--token-a")?,
            token_b: token_arg(args, "--token-b")?,
            fee_bps: required(args, "--fee-bps")?
                .parse()
                .map_err(|_| "--fee-bps must be one of 5, 30, 100".to_string())?,
        }),
        "add-liquidity" => Ok(Action::AddLiquidity {
            pair: hex32(&required(args, "--pair")?).ok_or("--pair must be 32 hex bytes")?,
            amount0_desired: number(args, "--amount0")?,
            amount1_desired: number(args, "--amount1")?,
            amount0_min: optional_number(args, "--min0"),
            amount1_min: optional_number(args, "--min1"),
        }),
        "remove-liquidity" => Ok(Action::RemoveLiquidity {
            pair: hex32(&required(args, "--pair")?).ok_or("--pair must be 32 hex bytes")?,
            lp_amount: number(args, "--lp")?,
            amount0_min: optional_number(args, "--min0"),
            amount1_min: optional_number(args, "--min1"),
        }),
        "swap-in" => Ok(Action::SwapExactIn {
            path: path_arg(args)?,
            token_in: token_arg(args, "--token-in")?,
            amount_in: number(args, "--amount-in")?,
            min_amount_out: optional_number(args, "--min-out"),
        }),
        "swap-out" => Ok(Action::SwapExactOut {
            path: path_arg(args)?,
            token_in: token_arg(args, "--token-in")?,
            amount_out: number(args, "--amount-out")?,
            max_amount_in: number(args, "--max-in")?,
        }),
        "htlc-lock" => {
            // Either give the secret and let the tool hash it, or give the hashlock when the
            // secret belongs to a counterparty on another chain.
            let hashlock = match (flag(args, "--preimage"), flag(args, "--hashlock")) {
                (Some(preimage), None) => {
                    let bytes = hex32(&preimage).ok_or("--preimage must be 32 hex bytes")?;
                    popcorn_core::crypto::sha256(&bytes)
                }
                (None, Some(hashlock)) => {
                    hex32(&hashlock).ok_or("--hashlock must be 32 hex bytes")?
                }
                _ => return Err("pass exactly one of --preimage or --hashlock".to_string()),
            };
            // Relative to the target round, because the caller does not know it yet: the
            // client picks the round, and the expiry has to land after it (§5.2, step 5).
            let expiry_in: u64 = required(args, "--expiry-in")?
                .parse()
                .map_err(|_| "--expiry-in must be a number of rounds".to_string())?;
            if expiry_in == 0 {
                return Err("--expiry-in must be at least 1 round".to_string());
            }
            Ok(Action::HtlcLock {
                to: hex32(&required(args, "--to")?).ok_or("--to must be 32 hex bytes")?,
                token: flag(args, "--token")
                    .map(|value| hex32(&value).ok_or("--token must be 32 hex bytes"))
                    .transpose()?
                    .unwrap_or(popcorn_core::constants::NATIVE_TOKEN),
                amount: amount()?,
                hashlock,
                // Filled in by the caller once the target round is known.
                expiry_round: expiry_in,
            })
        }
        "htlc-claim" => Ok(Action::HtlcClaim {
            htlc_id: hex32(&required(args, "--htlc-id")?)
                .ok_or("--htlc-id must be 32 hex bytes")?,
            preimage: hex32(&required(args, "--preimage")?)
                .ok_or("--preimage must be 32 hex bytes")?,
        }),
        "htlc-refund" => Ok(Action::HtlcRefund {
            htlc_id: hex32(&required(args, "--htlc-id")?)
                .ok_or("--htlc-id must be 32 hex bytes")?,
        }),
        "stake" => Ok(Action::Stake { amount: amount()? }),
        "unstake" => Ok(Action::Unstake { amount: amount()? }),
        "claim" => Ok(Action::ClaimRewards {}),
        "publish" => {
            let topic = hex32(&required(args, "--topic")?).ok_or("--topic must be 32 hex bytes")?;
            let data = from_hex(&required(args, "--data")?).ok_or("--data must be hex")?;
            Ok(Action::Publish { topic, data })
        }
        other => Err(format!("unknown action `{other}`\n\n{USAGE}")),
    }
}

/// Download a chain over HTTP and decode it for replay.
///
/// The genesis parameters come from `/params`, and they are printed rather than silently
/// trusted: a verifier should compare them against the published genesis, since a node that
/// lied about its own node key could otherwise "verify" its own fork.
fn fetch_chain(url: &str) -> Result<(GenesisConfig, Vec<popcorn_core::types::Block>), String> {
    use popcorn_node::client;
    use popcorn_node::encoding::{from_base64, hex32};

    let base = client::Url::parse(url)?;
    let params = client::get(&base.join("/params"))?;

    let node_pubkey = hex32(params["node_pubkey"].as_str().unwrap_or(""))
        .ok_or("node pubkey in /params is malformed")?;
    let foundation_pubkey = hex32(params["foundation_pubkey"].as_str().unwrap_or(""))
        .ok_or("foundation pubkey in /params is malformed")?;
    let genesis_drand_round = params["genesis_drand_round"]
        .as_u64()
        .ok_or("genesis round in /params is malformed")?;

    println!("verifying against parameters served by {url}:");
    println!("  node pubkey:         {}", to_hex(&node_pubkey));
    println!("  foundation pubkey:   {}", to_hex(&foundation_pubkey));
    println!("  genesis drand round: {genesis_drand_round}");
    println!("  compare these against the published genesis before trusting the result\n");

    let export = client::get(&base.join("/chain/export?from=0"))?;
    let encoded = export["blocks"]
        .as_array()
        .ok_or("/chain/export did not return a block list")?;

    let mut blocks = Vec::with_capacity(encoded.len());
    for (index, value) in encoded.iter().enumerate() {
        let text = value.as_str().ok_or("a block was not a string")?;
        let bytes = from_base64(text).ok_or(format!("block {index} is not valid base64"))?;
        blocks.push(
            borsh::from_slice(&bytes).map_err(|e| format!("block {index} does not decode: {e}"))?,
        );
    }

    Ok((
        GenesisConfig {
            genesis_drand_round,
            node_pubkey,
            foundation_pubkey,
        },
        blocks,
    ))
}

/// A token argument: 32 hex bytes, or the word `native`.
fn token_arg(args: &[String], name: &str) -> Result<[u8; 32], String> {
    use popcorn_node::encoding::hex32;
    let raw = required(args, name)?;
    if raw == "native" {
        return Ok(popcorn_core::constants::NATIVE_TOKEN);
    }
    hex32(&raw).ok_or_else(|| format!("{name} must be 32 hex bytes or `native`"))
}

/// A swap path: one or more pair ids, comma-separated, in hop order.
fn path_arg(args: &[String]) -> Result<Vec<[u8; 32]>, String> {
    use popcorn_node::encoding::hex32;
    required(args, "--path")?
        .split(',')
        .map(|entry| {
            hex32(entry.trim()).ok_or_else(|| "--path entries must be 32 hex bytes".to_string())
        })
        .collect()
}

fn number(args: &[String], name: &str) -> Result<u128, String> {
    required(args, name)?
        .parse()
        .map_err(|_| format!("{name} must be a whole number"))
}

fn optional_number(args: &[String], name: &str) -> u128 {
    flag(args, name).and_then(|v| v.parse().ok()).unwrap_or(0)
}

fn chain_path(data: &Path) -> PathBuf {
    data.join("popcorn.redb")
}
