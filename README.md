<div align="center">

<img src="assets/popcorn-logo.jpg" alt="POPCORN" width="480">

# POPCORN

**A single-operator deterministic/verifiable execution chain**<br>
*One node produces every block. Trust is replaced by verifiability.*

[Documentation](docs/README.md) · [Specification](SPEC.md) · [Consensus lock](CONSENSUS-LOCK.md) · [Changelog](CHANGELOG.md)

`v0.9.3` · pre-genesis · `CONSENSUS_VERSION = 0x0000_0009_0003`

</div>

---

There is no distributed consensus here and no P2P layer. One operator runs the only node, and
that is said first because everything else follows from it. What the chain spends its
complexity on is making what that operator does **checkable** — anyone replays the chain from
zero, recomputes every root, and says so publicly if anything fails to match.

```bash
popcorn verify --node https://a-popcorn-node.example --audit-collection
```

> **Status: pre-genesis.** The normative document is [`SPEC.md`](SPEC.md). Parameters and
> semantics stay editable until genesis is produced; after that, every change listed in §13 is
> effectively a new chain.

## What it can prove

| | Guarantee | Mechanism |
|---|---|---|
| 1 | **Pre-beacon blindness** | Transactions arrive timelock-encrypted toward a future drand round — unreadable until that round's beacon exists |
| 2 | **Deterministic ordering** | Fisher-Yates over a BLAKE3 stream seeded by the beacon. Never an operator choice |
| 3 | **Receipt accountability** | A receipted blob missing from a block's manifest is two signatures by the same node contradicting each other |
| 4 | **State root verifiability** | Any accounting or execution error diverges under replay |

And what it does not: POPCORN makes **no claim to censorship resistance**. Inclusion is the
only gate, and the receipt makes a refusal provable rather than impossible. The rest of the
declared limits are in [Security](docs/security.md) — collected in one place rather than spread
thin.

## Features

| | | |
|---|---|---|
| 💱 | **[DEX](docs/dex.md)** | Uniswap V2 generalized to fee tiers: multi-hop, exact-in and exact-out, LP tokens as first-class balances |
| 🔒 | **[HTLC](docs/htlc.md)** | User-carried atomic cross-chain swaps. SHA-256 hashlocks, settlement by anyone who holds the secret, no bridge and no custody |
| 📰 | **[Publish](docs/publish.md)** | A data board timestamped by the beacon — signed, ordered, consumed off-chain. No VM, and it says so |
| 🥩 | **[Staking](docs/staking.md)** | Fair launch, halving emission, an O(1) accumulator transcribed from audited Synthetix math |
| 🛡 | **[Security](docs/security.md)** | Two separated keys, censorship that leaves evidence, and the measured cost of a flood |
| 🔌 | **[API](docs/api.md)** | Twelve endpoints, one of which is an obligation rather than a feature |
| 🖥 | **[Explorer & wallet](docs/web.md)** | Served by the node itself: live blocks and pools, and a wallet that signs and timelock-encrypts in your browser |
| ▶️ | **[Running a node](docs/running-a-node.md)** | Keys, genesis, block production, and a client that speaks every action |
| ✅ | **[Verification](docs/verification.md)** | How to check the chain yourself — and the two real bugs that checking caught |

## At a glance

| | |
|---|---|
| Block time | one drand quicknet round — 3 s, never skipped |
| Identity | ed25519, `AccountId = blake3(verifying_key)` — a Phantom/Solflare keypair works |
| Accounts | implicit on first receipt of funds; the public key appears on first spend |
| Supply | fair launch, nothing allocated at genesis; ≈ 21.02 M as an upper bound |
| Fees | flat and **burned** — the operator earns nothing from them |
| Storage | one redb file; append-only blocks are the source of truth, state is a rebuildable cache |
| Dependencies | all Rust, zero native C/C++, every consensus-relevant one pinned exactly |
| Front end | explorer and wallet compiled into the node binary; loads nothing from any other origin |

## Quick start

```bash
cargo build --release
cargo test --workspace          # 93 tests
./ci/consensus-gates.sh         # structural rules a test cannot express

# a chain of your own
popcorn keygen  --out node.key
popcorn keygen  --out foundation.key
popcorn genesis --data ./data --node-key node.key --foundation-key foundation.key
popcorn node    --data ./data --node-key node.key --listen 127.0.0.1:8080
# the explorer and wallet are then at http://127.0.0.1:8080/ — same origin, nothing external

# a transaction: signed locally, timelock-encrypted, receipted
popcorn submit --key foundation.key --node http://127.0.0.1:8080 \
    transfer --to <ACCOUNT> --amount 1000000000

# and the part that matters
popcorn verify --node http://127.0.0.1:8080 --audit-collection
```

Full walkthrough: [Running a node](docs/running-a-node.md).

## Repository layout

```
crates/popcorn-core/       consensus: types, IDs, state root, AMM, staking, validation, execution
crates/popcorn-timelock/   TimelockProvider: drand beacons, POPCORN-TLOCK-AGE-V1 blobs
crates/popcorn-node/       storage, HTTP/WS API, block producer, replay verifier, CLI
web/                       the explorer and wallet the node serves, plus the browser-path gate
reference/                 an independent reference executor, in Python, written from the spec
interop/                   the Go and JavaScript halves of the cross-language gate
vectors/                   committed fixtures any implementation can check itself against
docs/                      these pages
SPEC.md                    the normative specification
CONSENSUS-LOCK.md          exact pinned versions of everything that can move a state root
```

`popcorn-core` is pure: no I/O, no async, no clock. Everything that can influence a state root
lives there and nothing else does — which is what makes both a replay verifier and an
independent reference executor possible at all.

## Before genesis

Every gate the specification requires now exists and passes: the staking property test,
canonicity, an independent reference executor across 400 scenarios, 26 borderline signature
vectors, a three-language interoperability gate, an end-to-end fixture against a real drand
round, a measured availability benchmark, and a browser-path gate that builds one transaction of
every kind through the page's own bundle.

Two of them found real problems — a missing bucket in the monetary invariant, and a DoS cost
that runs opposite to intuition. Both are written up in
[Verification](docs/verification.md#what-the-gates-found), because a gate that never caught
anything has not been shown to work.

## Naming

The name is a **consensus identifier**, not a marketing variable: it enters signed preimages
(`SIGN_DOMAIN = "popcorn-v1"`, receipt domain `"popcorn-receipt-v1"`, the POPCORN-TLOCK-AGE-V1 /
POPCORN-CONSENSUS / POPCORN-V2-MATH profiles). Any occurrence of ARENA / arena-chain, in any
form, is stale by construction.

## License

MIT.
