[← Docs](README.md) · [Overview](overview.md) · [Architecture](architecture.md) · [DEX](dex.md) · [HTLC](htlc.md) · [Publish](publish.md) · [Staking](staking.md) · [Security](security.md) · [API](api.md) · [Web](web.md) · [Node](running-a-node.md) · **Verification**

# Verification

> The chain's only real claim is that you do not have to believe it. This page is how you
> check, and what checking has already caught.
> *Specification: [§10](../SPEC.md#10-third-party-verification)*

## Two properties, deliberately kept apart

They need different inputs, and conflating them is how a system ends up claiming more than it
can show.

| | Needs | Proves |
|---|---|---|
| **State replay** | `/chain/export` alone | Every state root, root, signature and the monetary invariant at every block |
| **Collection audit** | the blobs *and* the beacon | That every manifested blob resolves honestly — and that `unusable` is not a lie |

Replay is self-contained because transactions travel **in the clear inside blocks**. The
encrypted blobs do not live in the block, so auditing the collection needs the node or a
mirror to serve them.

```bash
popcorn verify --node http://127.0.0.1:8080                      # replay
popcorn verify --node http://127.0.0.1:8080 --audit-collection   # replay + audit
popcorn verify --data ./data                                     # replay, offline
```

```
verifying against parameters served by http://127.0.0.1:8080:
  node pubkey:         30e583b87a0b1357…
  foundation pubkey:   9018644aaf67fc36…
  genesis drand round: 32120318
  compare these against the published genesis before trusting the result

replayed 408 blocks
audited 15 manifested blobs against the beacon in each block
every state root, root and monetary invariant matches
```

The parameters are **printed, not trusted**: a node that lied about its own key could otherwise
verify its own fork.

## What replay actually checks

Per block: the node's signature over the header; `prev_hash`; the drand signature against the
public quicknet chain info; `drand_sig_hash`; that the round matches `G + h − 1`; a **recomputed**
shuffle and nonce normalization; `collection_root`, `rejected_root`, `results_root`; that
`unusable ⊆ manifest` and both are sorted sets; re-execution of every transaction; and the
five-bucket monetary invariant.

Nothing is taken from the block that can be derived from it instead. Any divergence is
cryptographic proof of incorrectness — and the tool says so in those words.

## The one property replay cannot check

"The node did not see the transactions before the round" rests on **client-side encryption**,
not on blocks. Nothing in the chain can prove it after the fact; it is the single property that
depends on client behaviour. The cross-language vectors are what make the encryption itself
reproducible, but the discipline of encrypting before submitting is yours.

## The gates

Every gate the specification makes a precondition for genesis, and how to run it:

| Gate | Proves | Command |
|---|---|---|
| **Staking property** | the invariant, per-settle conservation, and `reserved ≥ Σ pending ≥ 0` after *every* operation over millions of random sequences | `cargo test -p popcorn-core --release --test staking_gate` |
| **Canonicity** | same logical state, different insertion orders → identical bytes → identical root | `cargo test -p popcorn-core --test consensus` |
| **Reference executor** | a second implementation, in Python, agrees on order, results, rejections, all five roots and the invariant across 400 scenarios | `python3 reference/differential.py vectors/differential.json` |
| **Borderline signatures** | the pinned `verify_strict` semantics on 26 edge cases | `python3 reference/signatures.py vectors/signatures.json` |
| **Cross-language** | Rust, Go and JavaScript read each other's blobs and agree on 19 rejection verdicts | `./interop/run-gate.sh` |
| **Browser path** | the [page the node serves](web.md) encodes, signs and encrypts all fourteen action kinds exactly as the node reads them | `./web/test/browser-path.sh` |
| **End-to-end vector** | a real drand round through to `state_root` and a receipt, replayed offline | `cargo test -p popcorn-timelock --test vectors` |
| **Availability** | what a flood actually costs | `cargo run --release -p popcorn-timelock --example dos_benchmark -- 10000` |
| **Structural** | no unordered collections near a commitment, exact `=` pinning, the naming rule | `./ci/consensus-gates.sh` |

## Checking yourself against POPCORN

You do not need this implementation's crates — you need to agree with them, byte for byte, on
the committed fixtures:

| Fixture | Contains |
|---|---|
| [`vectors/end_to_end.json`](../vectors/end_to_end.json) | a transaction encrypted toward drand round 1000, that round's real BLS signature, and every derived value through `state_root`, `block_hash` and the receipt |
| [`vectors/differential.json`](../vectors/differential.json) | 400 pre-state + batch scenarios with the expected order, results, rejections and roots |
| [`vectors/signatures.json`](../vectors/signatures.json) | 26 borderline signature cases with the verdict the pinned implementation gives each |
| [`vectors/profile/`](../vectors/profile/) | blob acceptance and rejection cases with the verdict name all implementations must return |

`reference/` is a worked example of doing exactly that: a Python executor written from the
specification, including a pure-Python `verify_strict`.

## What the gates found

Two real problems, which is the only reason to have gates.

**The monetary invariant was missing a bucket.** It listed four places a native unit can live
and omitted **AMM pool reserves**. A pair may hold the native token, and those units leave a
balance to get there — so the equality became false the moment anyone provided native
liquidity. Since replay checks it at every block, an honest chain would have failed its own
verification. Driven on a live chain: with a pool funded, the four-bucket sum came up short by
exactly the pool's native reserve, and the same chain verified with the old rule reported **50
divergences**. The invariant is now five buckets.

What marks it as a *specification* bug rather than a code bug is how it presented: the two
implementations agreed with each other on every root, result and rejection — and both reported
the invariant broken. Neither was wrong about the other. The text was wrong about the world.

**The CPU worst case is not the obvious one.** Garbage costs 0.1% of a round; blobs that look
legitimate cost ~2.5 ms each. See [Security](security.md#availability-the-measured-cost-of-a-flood).

Three smaller corrections came from the cross-language work: grease stanzas (the `age` crate
writes one, drand's Go tlock does not, so "exactly one recipient stanza" was unimplementable),
armored output from `tlock-js` (which browser clients must de-armor), and the check order in
`CreatePair` that had left one committed enum discriminant unreachable.

## What is not covered, and why

| | |
|---|---|
| `Overflow` | A defensive catch-all. Every field it guards is bounded by finite supply, so reaching it would itself be the bug |
| `SupplyOutOfRange` | Declared unreachable: static validation catches it first with `FieldOutOfRange` |
| `Malformed` | A Borsh decode failure makes a blob `unusable` rather than rejected; covered by the profile tests instead |
| Scale | No chain has run past a few hundred blocks. Startup cost on a long chain is bounded by the checkpoint interval by design, but that bound has not been measured |

---

[← Running a node](running-a-node.md) · [Docs index →](README.md)
