<div align="center">
  <img src="../assets/popcorn-logo.jpg" alt="POPCORN" width="220">
  <h1>POPCORN Documentation</h1>
  <p><em>A single-operator deterministic/verifiable execution chain</em></p>
</div>

---

These pages explain how POPCORN works and why it is built the way it is. They are **not
normative** — [`SPEC.md`](../SPEC.md) is, and every page points at the sections it explains.
Where a page and the specification disagree, the specification is right and the page is a bug.

## Start here

| | Page | What you will find |
|---|---|---|
| 🍿 | **[Overview](overview.md)** | What the chain is, the four things it can prove, and the limits it declares instead of hiding |
| 🏗 | **[Architecture](architecture.md)** | The batch lifecycle: blind collection, beacon, decryption, ordering, execution, commitment |
| 💱 | **[DEX](dex.md)** | Pairs, fee tiers, multi-hop routing, exact-in and exact-out, the liquidity lifecycle |
| 🔒 | **[HTLC](htlc.md)** | User-carried atomic swaps, carrier-independent settlement, and the attacks the design assumes |
| 📰 | **[Publish & oracles](publish.md)** | The data board: timestamped by a beacon, consumed off-chain, and how it settles locks |
| 🥩 | **[Staking & economy](staking.md)** | Fair launch, halving emission, the O(1) accumulator, and the five buckets every unit lives in |
| 🛡 | **[Security](security.md)** | Trust model, key separation, censorship accountability, and the measured cost of a flood |
| 🔌 | **[API](api.md)** | Every endpoint, what it returns, and which ones an auditor needs |
| 🖥 | **[Explorer & wallet](web.md)** | The front end the node serves: live blocks, accounts, pools, and a wallet that signs and encrypts in your browser |
| ▶️ | **[Running a node](running-a-node.md)** | Keys, genesis, producing blocks, submitting transactions |
| ✅ | **[Verification](verification.md)** | How to check the chain yourself, and the gates that must pass before genesis |

## The shape of the thing

```
                    ┌──────────── drand quicknet ────────────┐
                    │  one beacon every 3 s, BLS-verified     │
                    └────────────────────┬───────────────────┘
                                         │  seeds the ordering,
                                         │  unlocks the blobs
  clients                                ▼
  ───────      ┌───────────────────────────────────────────────┐
  sign ──┐     │  POPCORN node                                 │
  tlock ─┼───▶ │  collect blindly → commit → decrypt → order    │──▶ block
  POST ──┘     │  → burn fees → execute → emit → commit state   │
      ▲        └───────────────────────────────────────────────┘
      │                                         │
      └────── signed receipt ───────────────────┘
              (evidence the node cannot take back)
                                                │
                            anyone ─────────────▼──────────────
                            replay every block, recompute every
                            root, check the invariant, and say
                            so publicly if it does not match
```

## Reference material

- **[`SPEC.md`](../SPEC.md)** — the normative specification, v0.9.3
- **[`CONSENSUS-LOCK.md`](../CONSENSUS-LOCK.md)** — exact pinned versions of everything that can move a state root
- **[`CHANGELOG.md`](../CHANGELOG.md)** — what changed between versions and why
- **[`vectors/`](../vectors/)** — committed fixtures any implementation can check itself against
