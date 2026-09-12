[← Docs](README.md) · [Overview](overview.md) · **Architecture** · [DEX](dex.md) · [HTLC](htlc.md) · [Publish](publish.md) · [Staking](staking.md) · [Security](security.md) · [API](api.md) · [Web](web.md) · [Node](running-a-node.md) · [Verification](verification.md)

# Architecture

> One batch per drand round. The order of its phases is not arrangement — each boundary
> exists because of a specific failure it prevents.
> *Specification: [§5](../SPEC.md#5-batch-lifecycle)*

## The batch, phase by phase

| # | Phase | Why it sits exactly here |
|---|---|---|
| 1 | **Blind collection** | Blobs arrive timelock-encrypted. The node queues bytes it cannot read and signs a receipt for each |
| 2 | **Collection commitment** | The manifest is frozen and `collection_root` computed — the node commits to *what it received* before it can see any of it |
| 3 | **Beacon** | `get(R)` against drand, BLS-verified. No beacon, no block: the chain waits |
| 4 | **Decryption** | Every manifested blob must resolve into a transaction, a rejection, or `unusable` — no fourth option |
| 5 | **Static validation** | A nine-step ordered pipeline; a transaction dropped at one step takes no part in the next |
| 6 | **Ordering** | Shuffle seeded by the beacon, then per-account nonce normalization |
| 7 | **Fee collection** | All fees burned in a **single phase, before any action runs** |
| 8 | **Execution** | Sequential, each transaction snapshot-and-rollback |
| 9 | **Close** | HTLC auto-settlement, then emission, then the state root and the signature |

Three of those boundaries are load-bearing:

- **Fees before execution.** Otherwise the first transaction drains the balance and everything
  after it rides free. Every transaction pays the full fee whether it succeeds or fails.
- **Auto-settlement after execution.** A claim in the same batch therefore wins, and a publish
  carrying the same preimage becomes a deterministic no-op rather than a race.
- **Emission last.** Nothing minted in a batch is spendable inside it.

## Ordering: why the operator cannot pick

```
seed = blake3(drand_signature ‖ LE64(height))          ← the beacon, which nobody controls
stream = seed.finalize_xof()                            ← an endless byte stream
uniform(n): draw 8 bytes; reject above the largest multiple of n; retry
            (the stream always advances — a rejected draw is spent)
Fisher-Yates over the valid transactions, sorted by tx_id
then: each account's own transactions are re-slotted in ascending nonce order
```

The shuffle decides *positions*; nonce normalization decides which of an account's own
transactions lands in which of its own slots. So a contiguous run never fails because the
shuffle scattered it, and no account's transactions can be reordered relative to another's.

A verifier recomputes all of it from the block's own stored beacon signature. There is nothing
to trust.

## What a block commits to

The signed object is the **header**, and every list in the block hangs off one of its roots:

| Root | Covers |
|---|---|
| `collection_root` | the set of distinct blobs received — a *set*, so ten submissions of one blob is one entry |
| `txs_root` | transaction ids, in execution order |
| `rejected_root` | `(tx_id, reason)` pairs — the reason is committed too, so it cannot be restated later |
| `results_root` | the outcome of every executed transaction |
| `state_root` | the whole state after the batch |

```
block_hash     = blake3(borsh(Header))
node_signature = ed25519(node_key, block_hash)
```

## The state root

Five tables, visited in a frozen order, each entry length-prefixed so no concatenation is
ambiguous:

```
for table in [0x01 accounts, 0x02 tokens, 0x03 pairs, 0x04 htlcs, 0x05 global]:
    update(tag)
    for (k, v) in table, keys in lexicographic byte order:
        update(LE32(len(borsh(k)))); update(borsh(k))
        update(LE32(len(borsh(v)))); update(borsh(v))
```

One architectural rule keeps this honest, and it is enforced by CI rather than by good
intentions: **no unordered collections anywhere near a commitment**. `HashMap` and `HashSet`
are forbidden in the consensus crate regardless of library versions, because iteration order
that depends on insertion history is a consensus split waiting for a busy day.

## Crates

| Crate | Contains | Can it move a state root? |
|---|---|---|
| `popcorn-core` | types, ids, state root, AMM, staking, validation, execution, emission | **Yes** — and nothing else can |
| `popcorn-timelock` | the `TimelockProvider` trait, drand beacons, the blob profile | The blob profile, yes; the transport, no |
| `popcorn-node` | storage, collection, block production, API, verifier, CLI | **No** |

`popcorn-core` has no I/O, no async and no clock. That is what makes a replay verifier and an
independent reference executor possible at all — and it means two nodes running different
versions of `popcorn-node` still produce the same chain.

## Storage

One redb file. The `blocks` table is append-only and is the **source of truth**; state is
treated as exactly what the specification calls it, a rebuildable cache — checkpoints are
written periodically and startup replays whatever comes after the newest one. A lost or
corrupted checkpoint costs time, never correctness.

Manifested blobs are stored too, because [§10](../SPEC.md#10-third-party-verification) makes
serving them an operational obligation: without them nobody outside can audit the collection.

---

[← Overview](overview.md) · Next: [DEX →](dex.md)
