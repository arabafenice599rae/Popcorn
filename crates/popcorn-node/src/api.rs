//! The node API (SPEC.md §9.1).
//!
//! Serving is not consensus (§13.3), with one exception that is not decoration: `GET
//! /blob/{hash}` must answer for every manifested blob. Without it the collection audit of
//! §10 is impossible for third parties, which is why §10 makes mirroring an operational
//! obligation rather than a nicety. Refusing to serve a manifested blob is visible
//! obstruction.

use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use ed25519_dalek::SigningKey;
use popcorn_core::constants::{self, NATIVE_TOKEN};
use popcorn_core::execute::account_pending;
use popcorn_core::types::{Action, Block};
use popcorn_timelock::TimelockProvider;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::{broadcast, Mutex};

use crate::chain::Chain;
use crate::encoding::{from_base64, hex32, to_base64, to_hex};
use crate::mempool::Mempool;
use crate::producer::now_ms;

pub struct NodeApi {
    pub chain: Arc<Mutex<Chain>>,
    pub mempool: Arc<Mempool>,
    pub timelock: Arc<dyn TimelockProvider>,
    pub node_key: SigningKey,
    pub blocks: broadcast::Sender<Block>,
}

pub fn router(node: Arc<NodeApi>) -> Router {
    Router::new()
        .route("/tx", post(submit_tx))
        .route("/head", get(head))
        .route("/block/{height}", get(block))
        .route("/account/{id}", get(account))
        .route("/pair/{id}", get(pair))
        .route("/tokens", get(tokens))
        .route("/pairs", get(pairs))
        .route("/supply", get(supply))
        .route("/topic/{topic}", get(topic))
        .route("/blob/{hash}", get(blob))
        .route("/chain/export", get(export))
        .route("/params", get(params))
        .route("/stream", get(stream))
        .with_state(node)
}

#[derive(Deserialize)]
struct SubmitRequest {
    blob: String,
    target_round: u64,
}

/// Blind submission. The node queues bytes it cannot read and signs a receipt for them.
async fn submit_tx(
    State(node): State<Arc<NodeApi>>,
    Json(request): Json<SubmitRequest>,
) -> impl IntoResponse {
    let Some(blob) = from_base64(&request.blob) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "blob is not valid base64" })),
        );
    };

    let current_round = { node.chain.lock().await.next_round() };
    match node.mempool.submit(
        blob,
        request.target_round,
        current_round,
        &node.node_key,
        now_ms(),
    ) {
        Ok(receipt) => (
            StatusCode::OK,
            Json(json!({
                "domain": receipt.payload.domain,
                "blob_hash": to_hex(&receipt.payload.blob_hash),
                "target_round": receipt.payload.target_round,
                "timestamp_ms": receipt.payload.timestamp_ms,
                "receipt_hash": to_hex(&receipt.payload.receipt_hash()),
                "node_pubkey": to_hex(&receipt.node_pubkey),
                "signature": to_hex(&receipt.signature),
            })),
        ),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": error.to_string() })),
        ),
    }
}

async fn head(State(node): State<Arc<NodeApi>>) -> Json<Value> {
    let chain = node.chain.lock().await;
    Json(header_json(chain.head()))
}

async fn block(State(node): State<Arc<NodeApi>>, Path(height): Path<u64>) -> impl IntoResponse {
    let chain = node.chain.lock().await;
    match chain.storage().block(height) {
        Ok(Some(block)) => {
            let encoded = borsh::to_vec(&block).unwrap_or_default();
            (
                StatusCode::OK,
                Json(json!({
                    "header": header_json(&block.header),
                    "block_hash": to_hex(&block.header.block_hash()),
                    "node_signature": to_hex(&block.node_signature),
                    "drand_signature": to_hex(&block.drand_signature),
                    "blob_manifest": block.blob_manifest.iter().map(|h| to_hex(h)).collect::<Vec<_>>(),
                    "unusable": block.unusable.iter().map(|h| to_hex(h)).collect::<Vec<_>>(),
                    "tx_ids": block.txs.iter().map(|tx| to_hex(&tx.tx_id())).collect::<Vec<_>>(),
                    "results": block.results.iter().map(|r| format!("{r:?}")).collect::<Vec<_>>(),
                    "rejected": block.rejected.iter()
                        .map(|(id, reason)| json!({ "tx_id": to_hex(id), "reason": format!("{reason:?}") }))
                        .collect::<Vec<_>>(),
                    "borsh_base64": to_base64(&encoded),
                })),
            )
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "no such block" })),
        ),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        ),
    }
}

async fn account(State(node): State<Arc<NodeApi>>, Path(id): Path<String>) -> impl IntoResponse {
    let Some(account_id) = hex32(&id) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "account id must be 32 hex bytes" })),
        );
    };
    let chain = node.chain.lock().await;
    let state = chain.state();
    match state.account(&account_id) {
        Some(account) => (
            StatusCode::OK,
            Json(json!({
                "id": to_hex(&account_id),
                "pubkey": account.pubkey.map(|k| to_hex(&k)),
                "nonce": account.nonce,
                "staked": account.staked.to_string(),
                "pending_rewards": account_pending(state, &account_id).to_string(),
                "balances": account.balances.iter()
                    .map(|(token, amount)| json!({ "token": to_hex(token), "amount": amount.to_string() }))
                    .collect::<Vec<_>>(),
            })),
        ),
        // An account that has never received funds does not exist yet (§4.2); saying so is
        // more honest than inventing an empty one.
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "account does not exist yet" })),
        ),
    }
}

async fn pair(State(node): State<Arc<NodeApi>>, Path(id): Path<String>) -> impl IntoResponse {
    let Some(pair_id) = hex32(&id) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "pair id must be 32 hex bytes" })),
        );
    };
    let chain = node.chain.lock().await;
    match chain.state().pairs.get(&pair_id) {
        Some(pair) => (StatusCode::OK, Json(pair_json(pair))),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "no such pair" })),
        ),
    }
}

async fn tokens(State(node): State<Arc<NodeApi>>) -> Json<Value> {
    let chain = node.chain.lock().await;
    let tokens: Vec<Value> = chain
        .state()
        .tokens
        .values()
        .map(|token| {
            json!({
                "id": to_hex(&token.id),
                "creator": to_hex(&token.creator),
                "name": String::from_utf8_lossy(&token.name).trim_end_matches('\0').to_string(),
                "total_supply": token.total_supply.to_string(),
            })
        })
        .collect();
    Json(json!({ "tokens": tokens }))
}

async fn pairs(State(node): State<Arc<NodeApi>>) -> Json<Value> {
    let chain = node.chain.lock().await;
    let pairs: Vec<Value> = chain.state().pairs.values().map(pair_json).collect();
    Json(json!({ "pairs": pairs }))
}

async fn supply(State(node): State<Arc<NodeApi>>) -> Json<Value> {
    let chain = node.chain.lock().await;
    let state = chain.state();
    let global = &state.global;
    // Circulating is what is liquid: stake, escrow, pool reserves and the staking reserve all
    // exist but are not spendable from a balance. All five buckets of §5.5 are reported here,
    // so an auditor can check the invariant off this endpoint alone rather than joining it
    // against /pairs — which is what the first run against a chain with a native pool had to
    // do, and is exactly the kind of friction that stops people checking.
    let in_pools = state.total_native_in_pools();
    let buckets = state.total_native_balances()
        + global.total_staked
        + state.total_native_in_htlcs()
        + in_pools
        + global.staking_reserved;
    Json(json!({
        "genesis_supply": constants::GENESIS_SUPPLY.to_string(),
        "emitted": global.native_emitted.to_string(),
        "burned": global.native_burned.to_string(),
        "circulating": state.total_native_balances().to_string(),
        "staked": global.total_staked.to_string(),
        "in_htlcs": state.total_native_in_htlcs().to_string(),
        "in_pools": in_pools.to_string(),
        "staking_reserved": global.staking_reserved.to_string(),
        "bucket_total": buckets.to_string(),
        "invariant_holds": state.monetary_invariant_holds(constants::GENESIS_SUPPLY),
        "upper_bound": popcorn_core::emission::supply_upper_bound(constants::GENESIS_SUPPLY).to_string(),
        "height": global.height,
        "accounts": global.account_count,
    }))
}

#[derive(Deserialize)]
struct FromQuery {
    from: Option<u64>,
}

/// Publishes on a topic: a convenience index over blocks, not state (§7.5).
async fn topic(
    State(node): State<Arc<NodeApi>>,
    Path(topic): Path<String>,
    Query(query): Query<FromQuery>,
) -> impl IntoResponse {
    let Some(topic_id) = hex32(&topic) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "topic must be 32 hex bytes" })),
        );
    };
    let chain = node.chain.lock().await;
    let from = query.from.unwrap_or(1);
    let mut entries = Vec::new();
    if let Ok(blocks) = chain.storage().blocks_from(from) {
        for block in blocks {
            // Canonical feed order is execution order within the block (§7.5).
            for (index, tx) in block.txs.iter().enumerate() {
                if let Action::Publish { topic, data } = &tx.payload.action {
                    if *topic == topic_id {
                        entries.push(json!({
                            "height": block.header.height,
                            "drand_round": block.header.drand_round,
                            "position": index,
                            "publisher": to_hex(&tx.signer()),
                            "data_base64": to_base64(data),
                        }));
                    }
                }
            }
        }
    }
    (StatusCode::OK, Json(json!({ "entries": entries })))
}

/// Serve a manifested blob, so anyone can re-derive `unusable` for themselves (§9.2).
async fn blob(State(node): State<Arc<NodeApi>>, Path(hash): Path<String>) -> impl IntoResponse {
    let Some(blob_hash) = hex32(&hash) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "hash must be 32 hex bytes" })),
        );
    };
    let chain = node.chain.lock().await;
    match chain.storage().blob(&blob_hash) {
        Ok(Some(bytes)) => (
            StatusCode::OK,
            Json(json!({ "hash": to_hex(&blob_hash), "blob": to_base64(&bytes) })),
        ),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "blob not held" })),
        ),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        ),
    }
}

/// Blocks for replay. This endpoint alone is enough to re-derive every state root.
async fn export(
    State(node): State<Arc<NodeApi>>,
    Query(query): Query<FromQuery>,
) -> impl IntoResponse {
    let chain = node.chain.lock().await;
    let from = query.from.unwrap_or(0);
    match chain.storage().blocks_from(from) {
        Ok(blocks) => {
            let encoded: Vec<String> = blocks
                .iter()
                .map(|block| to_base64(&borsh::to_vec(block).unwrap_or_default()))
                .collect();
            (
                StatusCode::OK,
                Json(json!({ "from": from, "blocks": encoded })),
            )
        }
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        ),
    }
}

async fn params(State(node): State<Arc<NodeApi>>) -> Json<Value> {
    let chain = node.chain.lock().await;
    let config = chain.config();
    Json(json!({
        "consensus_version": format!("{:#018x}", constants::CONSENSUS_VERSION),
        "sign_domain": String::from_utf8_lossy(constants::SIGN_DOMAIN),
        "receipt_domain": constants::RECEIPT_DOMAIN,
        "genesis_drand_round": config.genesis_drand_round,
        "node_pubkey": to_hex(&config.node_pubkey),
        "foundation_pubkey": to_hex(&config.foundation_pubkey),
        "foundation_account": to_hex(&config.foundation_account()),
        "drand": {
            "scheme": constants::DRAND_SCHEME,
            "chain_hash": constants::DRAND_CHAIN_HASH,
            "remotes": constants::DRAND_REMOTES,
        },
        "parameters": {
            "max_tx_per_account_per_batch": constants::MAX_TX_PER_ACCOUNT_PER_BATCH,
            "max_tx_per_batch": constants::MAX_TX_PER_BATCH,
            "max_path_len": constants::MAX_PATH_LEN,
            "fee_tiers": constants::FEE_TIERS,
            "max_supply": constants::MAX_SUPPLY.to_string(),
            "genesis_supply": constants::GENESIS_SUPPLY.to_string(),
            "emission_0": constants::EMISSION_0.to_string(),
            "halving_interval": constants::HALVING_INTERVAL,
            "emission_staker_bps": constants::EMISSION_STAKER_BPS.to_string(),
            "fee_tx": constants::FEE_TX.to_string(),
            "max_publish_size": constants::MAX_PUBLISH_SIZE,
            "publish_free_bytes": constants::PUBLISH_FREE_BYTES,
            "publish_byte_fee": constants::PUBLISH_BYTE_FEE.to_string(),
            "minimum_liquidity": constants::MINIMUM_LIQUIDITY.to_string(),
            "precision": constants::PRECISION.to_string(),
            "max_blob_size": constants::MAX_BLOB_SIZE,
            "htlc_max_lifetime_rounds": constants::HTLC_MAX_LIFETIME_ROUNDS,
            "blob_round_horizon": constants::BLOB_ROUND_HORIZON,
            "native_token": to_hex(&NATIVE_TOKEN),
        },
    }))
}

/// Push every block as it is produced.
async fn stream(State(node): State<Arc<NodeApi>>, upgrade: WebSocketUpgrade) -> impl IntoResponse {
    let receiver = node.blocks.subscribe();
    upgrade.on_upgrade(move |socket| push_blocks(socket, receiver))
}

async fn push_blocks(mut socket: WebSocket, mut receiver: broadcast::Receiver<Block>) {
    while let Ok(block) = receiver.recv().await {
        let payload = json!({
            "height": block.header.height,
            "drand_round": block.header.drand_round,
            "block_hash": to_hex(&block.header.block_hash()),
            "collection_root": to_hex(&block.header.collection_root),
            "state_root": to_hex(&block.header.state_root),
            "txs": block.txs.len(),
            "rejected": block.rejected.len(),
            "unusable": block.unusable.len(),
        });
        if socket
            .send(Message::Text(payload.to_string().into()))
            .await
            .is_err()
        {
            return;
        }
    }
}

fn header_json(header: &popcorn_core::types::Header) -> Value {
    json!({
        "height": header.height,
        "prev_hash": to_hex(&header.prev_hash),
        "drand_round": header.drand_round,
        "drand_sig_hash": to_hex(&header.drand_sig_hash),
        "collection_root": to_hex(&header.collection_root),
        "txs_root": to_hex(&header.txs_root),
        "rejected_root": to_hex(&header.rejected_root),
        "results_root": to_hex(&header.results_root),
        "state_root": to_hex(&header.state_root),
        "block_hash": to_hex(&header.block_hash()),
    })
}

fn pair_json(pair: &popcorn_core::types::Pair) -> Value {
    json!({
        "id": to_hex(&pair.id),
        "token0": to_hex(&pair.token0),
        "token1": to_hex(&pair.token1),
        "fee_bps": pair.fee_bps,
        "reserve0": pair.reserve0.to_string(),
        "reserve1": pair.reserve1.to_string(),
        "lp_supply": pair.lp_supply.to_string(),
        "lp_token": to_hex(&popcorn_core::ids::lp_token_id(&pair.id)),
    })
}
