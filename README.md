<p align="center">
  <img src="assets/popcorn-logo.jpg" alt="POPCORN" width="520">
</p>

<h1 align="center">POPCORN</h1>

<p align="center">
  A single-node chain with a native economy and a finite maximum supply.<br>
  <em>Single-operator deterministic/verifiable execution chain.</em>
</p>

<p align="center">
  <a href="SPEC.md"><strong>Specification</strong></a> ·
  <a href="CHANGELOG.md">Changelog</a> ·
  <a href="CONSENSUS-LOCK.md">Consensus lock</a>
</p>

---

There is no distributed consensus and no P2P layer here. Trust is replaced by
**verifiability**: anyone downloads the chain and re-executes state from zero — including
every emission and every burn — and compares state roots. The operator cannot rewrite
history, emit outside the formula, or alter supply without diverging.

> **Status: pre-genesis.** The normative document is [`SPEC.md`](SPEC.md) — v0.9.3, freeze
> candidate, `CONSENSUS_VERSION = 0x0000_0009_0002`. Parameters and semantics stay editable
> until genesis is produced; after that, every change listed in §13 is effectively a new chain.

## The four guarantees

POPCORN does **not** promise censorship resistance. It promises, and can prove:

| | Guarantee | Mechanism |
|---|---|---|
| 1 | **Pre-beacon blindness** | tlock over drand quicknet: the operator cannot read a blob before the round's beacon exists |
| 2 | **Deterministic ordering** | Fisher-Yates over BLAKE3-XOF seeded by the beacon — never an operator choice |
| 3 | **Receipt accountability** | a receipted blob missing from the manifest is a contradiction between two node signatures |
| 4 | **State root verifiability** | any accounting or execution error diverges under replay |

The limit is declared, not buried: inclusion remains the only gate, and accountability covers
blobs the node receipted — it is not a universal proof that every packet sent was received
(§1.2, §9.2).

## At a glance

| | |
|---|---|
| Batch | one per drand quicknet round (3 s); `round(h) = GENESIS_DRAND_ROUND + h − 1`, never skipped |
| Identity | ed25519, `AccountId = blake3(verifying_key)` — a Phantom/Solflare keypair works |
| Accounts | implicit on first receipt of funds; pubkey materializes on first spend (P2PKH) |
| Economy | fair launch (`GENESIS_SUPPLY = 0`), per-batch emission with halving, 85/15 staker/foundation split, fees **burned** |
| Supply | ≈ 21.02 M as an upper bound; the effective figure is recomputed by the verifier batch by batch |
| DEX | Uniswap V2 generalized to fee tiers: multi-hop, exact-in/exact-out, LP tokens as first-class tokens |
| Staking | O(1) accumulator, a literal transcription of Synthetix `StakingRewards` |
| HTLC | user-carried atomic cross-chain swaps; SHA-256 hashlocks, no protocol bridge |
| Publish | a data board timestamped by the beacon; no state effect, no VM |
| Storage | single-file redb; append-only `blocks` is the source of truth, `state` is a rebuildable cache |

## Repository layout

```
crates/
  popcorn-core/       consensus: types, IDs, state root, AMM, staking, validation, execution
  popcorn-timelock/   TimelockProvider: drand beacons, POPCORN-TLOCK-AGE-V1 blobs
  popcorn-node/       storage (redb), HTTP/WS API, block producer, replay verifier, CLI
SPEC.md               normative specification (v0.9.3)
CONSENSUS-LOCK.md     exact pinned versions of every consensus-relevant dependency
```

`popcorn-core` is pure: no I/O, no async, no clock. Everything that can influence the state
root lives there, which is what makes the reference executor and the replay verifier possible.

## Build and run

```bash
cargo build --release
cargo test --workspace              # includes the consensus gates below

# initialize a chain (block 0, empty state, fair launch)
./target/release/popcorn keygen --out node.key
./target/release/popcorn keygen --out foundation.key
./target/release/popcorn genesis --data ./data \
    --node-key node.key --foundation-key foundation.key

# run the node
./target/release/popcorn node --data ./data --listen 127.0.0.1:8080

# replay the chain from genesis and compare every state root
./target/release/popcorn verify --data ./data
```

## Consensus gates

These run in `cargo test` and are the conditions under which genesis may be produced at all:

- **Staking property test** (§8) — random emission/stake/unstake/claim sequences checking,
  after *every* operation: the four-bucket monetary invariant as an exact equality, total
  conservation at each settle, and `staking_reserved ≥ Σ pending ≥ 0`.
- **Canonicity test** (§2.3) — same logical state, different insertion orders → identical
  bytes → identical `state_root`.
- **Consensus-grade vectors** (§10) — byte-for-byte fixtures from signed transaction through
  ordered batch to `state_root` and receipt, reproducible by an independent implementer.
- **Cross-language blob vectors** (§2.4) — encrypt/decrypt across Rust `tlock_age`, tlock-js
  and drand/tlock Go, byte-identical, rejection cases included.

## Operational obligations (outside consensus)

The operator **must** mirror the manifested encrypted blobs (§10): without them the collection
audit (`manifest → txs / rejected / unusable`) is not practicable by third parties. Availability
defences for the blind collection phase — which has no fees, since the signer is unknown until
decryption — are wire-level (`MAX_TOTAL_INGRESS_PER_ROUND`, per-IP rate limiting, a decryption
CPU budget) and never touch the protocol (§11).

## Naming

The name is a **consensus identifier**, not a marketing variable: it enters signed preimages
(`SIGN_DOMAIN = "popcorn-v1"`, receipt domain `"popcorn-receipt-v1"`, the POPCORN-TLOCK-AGE-V1 /
POPCORN-CONSENSUS / POPCORN-V2-MATH profiles). Any occurrence of ARENA / arena-chain, in any
form, is stale by construction.
