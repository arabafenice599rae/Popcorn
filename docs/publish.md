[← Docs](README.md) · [Overview](overview.md) · [Architecture](architecture.md) · [DEX](dex.md) · [HTLC](htlc.md) · **Publish** · [Staking](staking.md) · [Security](security.md) · [API](api.md) · [Node](running-a-node.md) · [Verification](verification.md)

# Publish & oracles

> A board where anyone can pin a signed, timestamped note. The data lives in the block and
> **never touches state** — the ledger does not grow by a byte.
> *Specification: [§7.5](../SPEC.md#75-publish--the-data-board-user-carried-oracles)*

## What it is

`Publish { topic, data }` has no state effect at all. It costs a fee, it lands in a block, and
that is the whole mechanism. What it buys you is exactly three properties, which happen to be
the three an oracle feed needs:

| Property | Where it comes from |
|---|---|
| **Authenticated** | The publisher's ed25519 signature is the feed's identity. Its whole history is on-chain |
| **Timestamped** | The drand round is a cryptographic timestamp — nobody, including the operator, chose it |
| **Ordered** | Within a block, the canonical order is the execution position in `txs`. Across blocks, the round |

## What it is not

**No on-chain logic consumes this data.** POPCORN has no VM. Nothing reads a published price
and moves funds because of it.

That is a real limitation and worth being blunt about: a price feed here is a *coordination
surface*, not an oracle a contract can trust, because there are no contracts. Consumers are
bots and services off-chain. The typical shape is a provider (Pyth-style) signing updates,
anyone republishing them here, and the consumer verifying the provider's signature themselves —
POPCORN guarantees the note was pinned at that round by that key, and nothing about whether the
number is true.

## The fee, and why it scales

```
tx_fee = FEE_TX + PUBLISH_BYTE_FEE × max(0, len(data) − PUBLISH_FREE_BYTES)
       = 5_000  + 50              × max(0, len(data) − 128)
```

128 bytes ride on the flat fee; beyond that you pay per byte, up to `MAX_PUBLISH_SIZE` of 512.
A full 512-byte publish costs about 0.0000242 native. The surcharge exists because block space
is the one thing a publish actually consumes, and it is burned like every other fee.

## Where it stops being just data

A `Publish` whose `data` is exactly 32 bytes is also a potential HTLC key. At batch close the
node checks whether its `sha256` opens an unexpired lock, and settles it if so — see
[HTLC](htlc.md#settlement-without-a-courier). This is the one place the board reaches into
state, and it does so without the publisher needing to be party to anything.

## Reading a feed

```bash
curl "http://127.0.0.1:8080/topic/<TOPIC-HEX>?from=1"
```

```json
{ "entries": [
    { "height": 312, "drand_round": 32120629, "position": 0,
      "publisher": "231b751c…", "data_base64": "QkJCQkJC…" } ] }
```

The endpoint is a convenience index over blocks, not state. Rebuild it yourself from
`/chain/export` if you would rather not trust the node's index — the blocks carry everything.

## Publishing

```bash
popcorn submit --key feed.key --node http://127.0.0.1:8080 \
    publish --topic <32-BYTE-HEX> --data <HEX>
```

---

[← HTLC](htlc.md) · Next: [Staking & economy →](staking.md)
