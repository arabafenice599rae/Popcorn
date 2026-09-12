[← Docs](README.md) · [Overview](overview.md) · [Architecture](architecture.md) · [DEX](dex.md) · [HTLC](htlc.md) · [Publish](publish.md) · [Staking](staking.md) · [Security](security.md) · [API](api.md) · [Web](web.md) · **Node** · [Verification](verification.md)

# Running a node

> One binary: `popcorn`. Everything below except `verify` is for the operator; `verify` is for
> everyone else.

## Build

```bash
cargo build --release
cargo test --workspace          # 93 tests
./ci/consensus-gates.sh         # structural rules a test cannot express
```

The compiler is pinned in `rust-toolchain.toml`. That is not a consensus rule — codegen does
not change a state root — but it keeps the lint set from drifting, so "it passes here" means
"it passes in CI".

## Keys

```bash
popcorn keygen --out node.key
popcorn keygen --out foundation.key
popcorn account --key foundation.key     # prints the public key and account id
```

Two keys, and they must be different — `genesis` refuses if they are not. The node key signs
blocks and controls no funds; the foundation key holds value and signs no blocks. `popcorn
node` only ever loads the first. Key files are written `0600`, and `keygen` refuses to
overwrite an existing file: overwriting one destroys either funds or an identity.

## Genesis

```bash
popcorn genesis --data ./data \
    --node-key node.key --foundation-key foundation.key \
    [--drand-round <N>]
```

```
initialized chain at ./data
genesis drand round: 32120318
genesis supply:      0 (fair launch: nothing is allocated)
node pubkey:         30e583b87a0b1357…
foundation account:  52ae3b8ef1720d84…
genesis state root:  2a199f73bba6e9a6…
```

Without `--drand-round` the current round plus a small lead is used. Whatever is stamped here
anchors the mapping `round(h) = G + h − 1` forever, and rounds are never skipped afterwards:
after downtime the node produces the missing blocks in sequence — empty ones if it collected
nothing — so HTLC windows stay whole rather than being jumped over.

## Producing blocks

```bash
popcorn node --data ./data --node-key node.key --listen 127.0.0.1:8080
```

One batch per drand round. If the beacon is not available the chain **waits**; if every trusted
source serves a signature that does not verify, the node **halts** rather than producing a
block on an unverified beacon.

Decryption runs across cores. On the numbers in [Security](security.md#availability-the-measured-cost-of-a-flood),
plan for roughly nine cores if you intend to admit a full 10,000-blob batch every round, or set
a `MAX_TLOCK_DECRYPT_WORK_PER_ROUND` you can actually meet.

Two flags decide what the node serves, and neither is consensus (§13.3) — a node with both
turned off still serves everything a verifier needs:

| Flag | Effect |
|---|---|
| `--no-web` | Do not serve the [explorer and wallet](web.md) at `/`. The endpoints stay |
| `--cors '*'` or `--cors <ORIGIN>[,<ORIGIN>...]` | Let browser pages on other origins read this API. Off by default; see [API](api.md#reading-this-api-from-another-origin) for why this is not a security setting |

## Submitting transactions

The client signs, encrypts toward a future round, and posts. The node answers with a receipt it
cannot take back.

```bash
popcorn submit --key alice.key --node http://127.0.0.1:8080 <ACTION> [--lead 3]
```

| Action | Arguments |
|---|---|
| `transfer` | `--to <ACCOUNT> --amount <N> [--token <HEX>]` |
| `stake` · `unstake` | `--amount <N>` |
| `claim` | — |
| `publish` | `--topic <HEX> --data <HEX>` |
| `create-token` | `--name <NAME> --supply <N>` |
| `create-pair` | `--token-a <HEX\|native> --token-b <HEX> --fee-bps <5\|30\|100>` |
| `add-liquidity` | `--pair <HEX> --amount0 <N> --amount1 <N> [--min0 <N>] [--min1 <N>]` |
| `remove-liquidity` | `--pair <HEX> --lp <N> [--min0 <N>] [--min1 <N>]` |
| `swap-in` | `--path <HEX[,HEX…]> --token-in <HEX> --amount-in <N> [--min-out <N>]` |
| `swap-out` | `--path <HEX[,HEX…]> --token-in <HEX> --amount-out <N> --max-in <N>` |
| `htlc-lock` | `--to <ACCOUNT> --amount <N> --expiry-in <ROUNDS> (--preimage <HEX32> \| --hashlock <HEX32>)` |
| `htlc-claim` | `--htlc-id <HEX> --preimage <HEX32>` |
| `htlc-refund` | `--htlc-id <HEX>` |

`create-token`, `create-pair` and `htlc-lock` print the identifier their transaction will
produce, since those derive from the signer and the nonce — you cannot look up a token that
does not exist yet.

`--lead N` targets the round N ahead of the current head; the default of 2 leaves time for
encryption and the round trip.

> **A new account cannot transact.** An account is born when it first *receives* funds, and it
> needs a native balance to cover `FEE_TX`. Bootstrapping means being sent something.

## Verifying

```bash
popcorn verify --node http://127.0.0.1:8080                      # as a third party
popcorn verify --node http://127.0.0.1:8080 --audit-collection   # plus the blob audit
popcorn verify --data ./data                                     # as the operator, offline
```

See [Verification](verification.md). The remote form is the one that matters — a third party
has no access to the operator's files and does not need it.

## Operating notes

- **Mirror the blobs.** Serving `/blob/{hash}` for manifested blobs is an obligation, not a
  nicety: without it nobody can audit the collection.
- **Publish the collection root early.** Not a consensus rule, but pushing it on `/stream`
  before the round closes gives observers a timestamp of the commit-then-decrypt.
- **Archive headers off-site.** A compromised node key cannot spend anything, but it can sign a
  fork. Timestamped external mirrors are what makes the divergence attributable.
- **The database is single-writer.** `verify --data` against a running node hits the lock; the
  error says so and points at the `--node` form.

---

[← API](api.md) · Next: [Verification →](verification.md)
