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
vectors/              committed end-to-end fixtures, replayable offline
ci/consensus-gates.sh structural gates: unordered collections, exact pinning, naming
SPEC.md               normative specification (v0.9.3)
CONSENSUS-LOCK.md     exact pinned versions of every consensus-relevant dependency
```

`popcorn-core` is pure: no I/O, no async, no clock. Everything that can influence the state
root lives there, which is what makes the reference executor and the replay verifier possible.
The node crate decides only operational things — when to close collection, how to survive a
blind phase that collects no fees, what to serve — and §13.3 makes those free to change.

## Build and run

```bash
cargo build --release
cargo test --workspace       # 90 tests, including the gates below
./ci/consensus-gates.sh      # structural rules a test cannot express
```

Start a chain. The two keys are distinct by design: the node key signs blocks and controls no
funds, the foundation key holds value and signs no blocks.

```bash
popcorn keygen --out node.key
popcorn keygen --out foundation.key
popcorn genesis --data ./data --node-key node.key --foundation-key foundation.key
popcorn node    --data ./data --node-key node.key --listen 127.0.0.1:8080
```

Genesis allocates nothing. The first native units appear when block 1 closes, as the
foundation's 15% share — and with nothing staked yet, the other 85% is simply never born.

Submit a transaction. The client signs, encrypts toward a future drand round, and gets back a
receipt the node cannot take back:

```bash
popcorn submit --key foundation.key --node http://127.0.0.1:8080 \
    transfer --to <ACCOUNT_HEX> --amount 1000000000
popcorn submit --key alice.key --node http://127.0.0.1:8080 stake --amount 500000000
popcorn submit --key alice.key --node http://127.0.0.1:8080 claim
```

Verify the whole chain from genesis — every signature, every root, every state root, and the
monetary invariant at every block:

```bash
popcorn verify --data ./data
```

## Consensus gates

These run in `cargo test` and are the conditions under which genesis may be produced at all:

- **Staking property test** (§8) — random emission/stake/unstake/claim sequences checking,
  after *every* operation: the four-bucket monetary invariant as an exact equality, total
  conservation at each settle, and `staking_reserved ≥ Σ pending ≥ 0`.
- **Canonicity test** (§2.3) — same logical state, different insertion orders → identical
  bytes → identical `state_root`.
- **Consensus-grade vectors** (§10) — [`vectors/end_to_end.json`](vectors/end_to_end.json) is a
  real fixture: a transaction encrypted toward drand quicknet round 1000, that round's actual
  BLS signature, and every derived value through `state_root`, `block_hash` and the receipt. It
  replays with no network at all, so an independent implementation can check itself against it.
- **Profile acceptance** (§3.6) — every prefix and every single-byte corruption of a valid blob
  must be refused rather than crash, and the rejection cases are pinned alongside the round
  trip: two implementations that disagree about which blobs are `unusable` disagree about which
  transactions exist.
- **Cross-language blob vectors** (§2.4) — *still to do*: encrypt/decrypt across Rust
  `tlock_age`, tlock-js and drand/tlock Go, byte-identical. The Rust half is in place; the Go
  and JS halves are not, and until they are, the cross-language claim is unproven.

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
