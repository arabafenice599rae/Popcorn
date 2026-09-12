[← Docs](README.md) · [Overview](overview.md) · [Architecture](architecture.md) · [DEX](dex.md) · [HTLC](htlc.md) · [Publish](publish.md) · [Staking](staking.md) · [Security](security.md) · **API** · [Web](web.md) · [Node](running-a-node.md) · [Verification](verification.md)

# API

> Serving is not consensus — two nodes with different HTTP layers still produce the same
> chain. With one exception, marked below, which is an obligation rather than a feature.
> *Specification: [§9](../SPEC.md#9-node-api)*

## Endpoints

| Method | Path | Returns |
|---|---|---|
| `POST` | `/tx` | Submit `{blob, target_round}` → a **signed receipt** |
| `GET` | `/head` | The latest header, with every root |
| `GET` | `/block/{height}` | A full block: header, manifest, `unusable`, tx ids, results, rejections, and the raw Borsh |
| `GET` | `/account/{id}` | Balances, nonce, materialized pubkey, stake, pending rewards |
| `GET` | `/pair/{id}` | Reserves, fee tier, LP supply, LP token id |
| `GET` | `/tokens` · `/pairs` | Listings |
| `GET` | `/supply` | All five buckets of the monetary invariant, their total, and whether it holds |
| `GET` | `/topic/{topic}?from={h}` | Publishes on a topic — a convenience index over blocks, not state |
| `GET` | `/blob/{hash}` | **A manifested encrypted blob.** See below |
| `GET` | `/chain/export?from={h}` | The block stream. This alone re-derives every state root |
| `GET` | `/params` | Genesis parameters, drand chain info, node and foundation public keys |
| `WS` | `/stream` | Blocks pushed as they are produced |
| `GET` | `/` · `/app.js` · `/app.css` · `/logo.jpg` | The [explorer and wallet](web.md), compiled into the binary. `--no-web` removes these four — and nothing a verifier needs |

### Reading this API from another origin

A node sends no `Access-Control-Allow-Origin` by default: the page it serves is same-origin
and needs none. A browser front end hosted anywhere else is blocked until the operator opts
in, per origin or for everyone:

```bash
popcorn node --data ./chain --node-key node.key --cors 'https://your-frontend.example'
popcorn node --data ./chain --node-key node.key --cors '*'
```

Say plainly what this is and is not. It is **not** a security boundary: there are no cookies,
no sessions and no authorization here, every endpoint answers the same to everyone, and
`POST /tx` takes bytes from anyone by design. A page that cannot call this API from JavaScript
can still call it from its own backend, so the header decides whether third-party pages need a
proxy — nothing more. Credentials are never allowed, because there are none to send. CLI, bot
and server-side clients need no flag, and `/stream` is unaffected either way: WebSocket
handshakes are not subject to CORS.

## `/tx` — and the receipt you should keep

```bash
curl -X POST http://127.0.0.1:8080/tx \
  -H 'content-type: application/json' \
  -d '{"blob":"<BASE64>","target_round":32120357}'
```

```json
{ "domain": "popcorn-receipt-v1",
  "blob_hash": "d4c0fcef…", "target_round": 32120357, "timestamp_ms": 1757635200000,
  "receipt_hash": "bc148e19…", "node_pubkey": "ed4928c6…", "signature": "1fd6981a…" }
```

The signature covers `blake3(borsh(ReceiptPayload))` — one normative preimage, reproducible in
any language. Keep it: if the blob never appears in that round's manifest, the receipt and the
block are two signatures by the same node contradicting each other.

`timestamp_ms` is declared by the node and is **non-consensus**. It is never used for ordering,
validity, or as proof of time.

## `/blob/{hash}` — the obligation

Every other endpoint is a convenience. This one is not: without the manifested blobs, nobody
outside can re-derive `unusable` and the collection audit of
[§10](../SPEC.md#10-third-party-verification) is impossible. Serving them, and mirroring them,
is an **operational obligation** of the operator. A node that stops answering here is
obstructing the audit, visibly.

## `/supply` — checking the invariant without leaving the endpoint

```json
{ "emitted": "22500000000", "burned": "40000",
  "circulating": "21746944848", "staked": "0", "in_htlcs": "0",
  "in_pools": "753015152", "staking_reserved": "0",
  "bucket_total": "22499960000", "invariant_holds": true,
  "height": 150, "accounts": 2 }
```

`bucket_total` must equal `emitted − burned`. The buckets are reported separately because an
auditor should not have to join this against `/pairs` by hand — that friction is what stops
people checking.

## `/params` — read before trusting a verification

```json
{ "consensus_version": "0x0000000000090002",
  "sign_domain": "popcorn-v1", "receipt_domain": "popcorn-receipt-v1",
  "genesis_drand_round": 32120318,
  "node_pubkey": "30e583b8…", "foundation_pubkey": "9018644a…",
  "drand": { "scheme": "bls-unchained-g1-rfc9380", "chain_hash": "52db9ba7…" },
  "parameters": { … } }
```

`popcorn verify --node` reads these and **prints them** rather than silently trusting them: a
node that lied about its own key could otherwise "verify" its own fork. Compare them against
the published genesis.

## Wire limits

These are outside consensus and exist because blind collection cannot charge a fee
([Security](security.md#availability-the-measured-cost-of-a-flood)):

| Limit | Value |
|---|---|
| `MAX_BLOB_SIZE` | 2 KiB, on the whole encrypted blob |
| `BLOB_ROUND_HORIZON` | 200 rounds (≈ 10 min) ahead |
| `MAX_TX_PER_BATCH` | 10,000 admitted per round |
| `MAX_TOTAL_INGRESS_PER_ROUND` | `MAX_TX_PER_BATCH × MAX_BLOB_SIZE` |

A blob for a round that has already been collected is refused, as is one beyond the horizon:
the timelock is fair ordering over minutes, not storage.

---

[← Security](security.md) · Next: [Running a node →](running-a-node.md)
