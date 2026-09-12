[← Docs](README.md) · **Overview** · [Architecture](architecture.md) · [DEX](dex.md) · [HTLC](htlc.md) · [Publish](publish.md) · [Staking](staking.md) · [Security](security.md) · [API](api.md) · [Web](web.md) · [Node](running-a-node.md) · [Verification](verification.md)

# Overview

> One node produces every block. Trust is replaced by verifiability: anyone replays the chain
> from zero and compares state roots. *Specification: [§1](../SPEC.md#1-trust-model)*

POPCORN is a chain with one operator and no consensus protocol. That is stated first and
plainly, because everything else follows from it — including what the chain deliberately does
not promise.

## What it can prove

| | Guarantee | Mechanism |
|---|---|---|
| 1 | **Pre-beacon blindness** | Transactions arrive timelock-encrypted toward a future drand round. The operator cannot read one before that round's beacon exists |
| 2 | **Deterministic ordering** | Execution order is Fisher-Yates over a BLAKE3 stream seeded by the beacon. Not a choice the operator gets to make |
| 3 | **Receipt accountability** | Every accepted blob gets a signed receipt. A receipted blob missing from a block's manifest is two signatures by the same node contradicting each other |
| 4 | **State root verifiability** | Every block commits to the state it produced. Any accounting or execution error diverges under replay |

## What it does not promise

POPCORN does **not** promise censorship resistance, and says so rather than implying otherwise:

- **Inclusion is the only gate.** An operator can refuse to include a transaction. The receipt
  makes that refusal *provable*, not impossible.
- **Consensus does not force collection to close before the beacon.** A node may wait, decrypt,
  and only then choose what to manifest — the block stays formally valid. The protection
  against that window is per-blob: a receipt obtained before your own deadline.
- **Accountability covers receipted blobs.** If the node never answers you at all, you can say
  you sent something, but you hold no proof. That is the chosen limit of a single-operator
  model, not a hidden defect.

> This page's job is to make those limits as easy to find as the guarantees. A system that
> buries them is asking to be trusted about the part it did not check.

## At a glance

| | |
|---|---|
| Block time | one drand quicknet round — 3 s |
| Round mapping | `round(h) = GENESIS_DRAND_ROUND + h − 1`, never skipped |
| Identity | ed25519, `AccountId = blake3(verifying_key)` — a Phantom/Solflare keypair works |
| Accounts | implicit on first receipt of funds; the public key appears on first spend |
| Supply | fair launch, nothing allocated at genesis; ≈ 21.02 M as an upper bound |
| Emission | per batch, halving yearly, 85% to stakers and 15% to the foundation |
| Fees | flat, and **burned** — the operator earns nothing from them |
| Built-in DEX | Uniswap V2 generalized to fee tiers, LP tokens are first-class |
| Cross-chain | HTLCs users build swaps with. No bridge, no custody of foreign assets |
| Storage | one redb file; append-only blocks are the source of truth, state is a rebuildable cache |

## Why a single operator at all

The alternative to one operator is not "a better operator" — it is a second block producer,
and that is a different architecture with different costs. POPCORN takes the single-operator
model seriously instead: it spends its complexity budget on making what the operator does
*checkable*, and on writing down precisely where checking stops.

Where that line falls is the subject of [Security](security.md).

---

[← Docs index](README.md) · Next: [Architecture →](architecture.md)
