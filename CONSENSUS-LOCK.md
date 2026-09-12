<h1 align="center">CONSENSUS-LOCK</h1>

<p align="center">
  The genesis annex required by <a href="SPEC.md#13-popcorn-consensus--normative-definition">SPEC.md §13</a>
</p>

---

> "*Pinned version* without a number contradicts this very section: the numbers live in the
> annex, stamped into genesis next to `CONSENSUS_VERSION`." — SPEC.md §13

`CONSENSUS_VERSION = 0x0000_0009_0003`
`lock_digest = 5be582738ffa6616bcb899eab0bbdf93e08dd05e26f9cafe28ad7e8717d521cd`

Both are written into the `global` singleton at genesis and are therefore inside **every**
state root (§5.4, §13). The digest is over the **list below**, not over this document: a
corrected typo in the prose must not change what a chain committed to, while a changed version
number must. `ci/check-lock.py` fails the build if the list stops matching what Cargo actually
resolves — a stamped list that has drifted is worse than no list at all, because it looks like
a guarantee.

## The digest preimage (`POPCORN-CONSENSUS-LOCK-V1`)

`lock_digest` commits to the ordered list below and nothing else, so a verifier holding only
the exported blocks and this section can recompute it — the prose tables further down are a
human-readable gloss, not the preimage. The list here is the authoritative one; it mirrors
`CONSENSUS_LOCK` in `crates/popcorn-core/src/constants.rs`, and `ci/check-lock.py` fails the
build if either drifts from the other or from `Cargo.lock`.

```
count = 12
 1. borsh            1.8.1
 2. borsh-derive     1.8.1
 3. ed25519-dalek    2.2.0
 4. curve25519-dalek 4.1.3
 5. blake3           1.8.7
 6. sha2             0.10.9
 7. primitive-types  0.14.0
 8. tlock            0.0.10
 9. tlock_age        0.0.10
10. age              0.11.5
11. age-core         0.11.0
12. drand_core       0.0.19
```

Digest algorithm (byte-exact, length-prefixed so no `(name, version)` pair can be re-cut into
another with the same digest):

```
lock_digest = blake3( "popcorn-consensus-lock-v1"          (25 ASCII bytes, the domain tag)
                      || LE32(count)
                      || for each entry, in the order above:
                           LE32(len(name))    || name       (name in ASCII)
                           LE32(len(version)) || version )  (version in ASCII)
```

With the list above this yields
`5be582738ffa6616bcb899eab0bbdf93e08dd05e26f9cafe28ad7e8717d521cd`, and the empty genesis it
produces has state root
`cc4c02f6d738d909a64849380ac604d8057522c808dec3ce7882252e1b4151b4` (§5.4, §13).

This file records the **exact** version of every dependency whose behaviour can influence a
state root. Upgrading any of them is a declared consensus change under §13.3 — never a side
effect of `cargo update`. Post-genesis, such an upgrade is effectively a new chain.

## Toolchain

| | |
|---|---|
| Rust edition | 2021 |
| Minimum toolchain | 1.85 (`rust-version`: the oldest compiler the code needs) |
| Pinned toolchain | 1.98.1 (`rust-toolchain.toml`: what CI and the maintainers build with) |
| Arithmetic | `u128` with `U256` intermediates; overflow is a deterministic `Failed`, never a panic or a wrap |

The toolchain pin is **not** a consensus rule — codegen does not change a Borsh byte or a state
root. It is pinned so the lint set cannot drift under the project: with a floating `stable` and
`-D warnings`, a commit that is clean on one machine fails on another having changed nothing.

## Consensus-relevant dependencies

Pinned with `=` in the workspace manifest, so resolution cannot drift.

| Crate | Version | Features | What it decides |
|---|---|---|---|
| `borsh` | 1.8.1 | `derive`, `de_strict_order` | Every byte that enters a hash or the state (§2.3). `de_strict_order` enforces strictly increasing collection order on decode |
| `borsh-derive` | 1.8.1 | — | Discriminants of the committed enums (§13.1), with `use_discriminant = true` so the tabulated numbers are the encoded ones |
| `ed25519-dalek` | 2.2.0 | `std`, `rand_core` | Signature acceptance, under `verify_strict` semantics (§3.1) |
| `curve25519-dalek` | 4.1.3 | — | The curve arithmetic underneath it (audited 2023) |
| `blake3` | 1.8.7 | `std` | All hashing, and the XOF that drives the shuffle (§3.7) |
| `sha2` | 0.10.9 | — | HTLC hashlocks only — the single non-blake3 point in the protocol (§7.6) |
| `primitive-types` | 0.14.0 | — | `U256` intermediates in the AMM and the staking accumulator (§6, §8) |

### What a pin actually protects against

Not the same thing for every entry, and the difference is worth stating because it changes
what a version mismatch would mean.

**Where the behaviour belongs to the library**, a version bump can change what is accepted or
what bytes come out, with no bug involved — this is the class where **semantic drift** is real
and the pin is the defence:

- `borsh` and `borsh-derive`: the serialization itself, collection ordering on decode, and the
  enum discriminants of §13.1. Every byte that enters a hash comes from here.
- `ed25519-dalek`: `verify_strict` is a *policy*, not the RFC. Which signatures are accepted —
  small-order points, non-canonical `s` — is this library's decision, and §3.1 freezes it.
- `tlock`, `tlock_age`, `age`, `age-core`: the blob format and the grease behaviour §3.6 is
  written against. Same input, different version, legitimately different bytes.
- `drand_core`: the beacon acceptance policy of §3.3.

**Where the function is fixed by a specification**, any correct implementation produces the
same bytes, so a version change cannot silently move a state root. The pin here guards against
**substitution and defect** — a compromised release, a miscompiled backend — not against
drift:

- `blake3` (and its XOF), `sha2`, `curve25519-dalek`, `primitive-types`.

That distinction is why the `sha2` duplicate below is safe twice over, and why saying so is
not the same as saying the check does not matter: the digests would be identical either way,
but the stamped list must still be true about what is compiled.

> **A second `sha2` is in the tree, and it is not this one.** `age` pulls `rust-embed` for its
> localized error strings, and that pulls `sha2 0.11.0`. Every cryptographic user —
> `popcorn-core`, `ed25519-dalek`, `tlock`, `age` itself, `age-core`, `drand_core`, `scrypt` —
> resolves 0.10.9. This is recorded rather than tidied away because "there is one copy of
> `sha2`" would be false, and a gate that checks this annex has to be checking something true.
> `ci/check-lock.py` therefore asserts the version on every *consensus edge*, not the absence
> of duplicates anywhere in the tree.

## Timelock stack

The blob format is consensus (§3.6): node and verifiers must accept and reject the same bytes.

| Crate | Version | Note |
|---|---|---|
| `tlock` | 0.0.10 | The raw 16-byte timelock primitive, unmodified |
| `tlock_age` | 0.0.10 | age with a tlock recipient — the blob format itself |
| `age` | 0.11.5 | Transitive, pinned by `tlock_age`. Not declared separately: a second declaration would only be a conflict to resolve. **This is the version whose grease behaviour §3.6 is written against** |
| `age-core` | 0.11.0 | Transitive. Source of `grease_the_joint()`, whose randomized `<random>-grease` stanza the profile tolerates |
| `drand_core` | 0.0.19 | Beacon fetch and BLS verification against the pinned chain info |

### Panic-safety of the decrypt path (operational requirement)

Decryption is a total function (SPEC.md §3.6, §5.1): a profile-valid blob maps to a plaintext
or to `unusable`, never to a halt. The pinned timelock crates do **not** honour that on their
own. A grep of the decrypt-reachable code in `tlock 0.0.10` alone finds several assertions and
a `panic!` — `ibe.rs:208`, `:271`, `:313` (`assert_eq!(c.u, r_g)`, the one a live flood hit)
and `:320` — any of which a crafted-but-profile-valid ciphertext can reach. Per-site patching
would be whack-a-mole across three crates and every future version; the guarantee is instead a
**containment at the per-blob boundary** in `blob::decrypt`, which turns any abnormal
termination into `TimelockError::Aborted` — an ordinary `unusable`. It covers the sites above
and the ones not yet found.

Two consequences are pinned, not incidental:

- **`panic = "unwind"` in the release profile is consensus-relevant.** Under `panic = "abort"`
  the containment is inert and a single crafted blob aborts the whole process — the remote
  halt, restored in full. The root `Cargo.toml` sets `unwind` and says why; a future change to
  `abort` "to shrink the binary" would reopen the hole.
- **The pinned `tlock` is not vendored, so its assertion is not patched.** It could be — under
  the totality rule a panic and an `Err` map to the same `unusable`, so patching `ibe.rs:313`
  to return an error would have zero semantic effect — but that would mean vendoring the crate
  and carrying a commit here, and the boundary containment already covers the whole class. The
  decision is to contain, not to fork; recorded so it is a choice, not an oversight.

### drand pinning

| | |
|---|---|
| `DRAND_SCHEME` | `bls-unchained-g1-rfc9380` |
| `DRAND_CHAIN_HASH` | `52db9ba70e0cc0f6eaf7803dd07447a1f5477735fd3f661792ba94600c84e971` (quicknet) |
| Period | 3 s |
| `GENESIS_DRAND_ROUND` | stamped at genesis; `round(h) = G + h − 1`, never skipped (§3.4) |

## Not consensus

These are free to change under §13.3, and changing them cannot fork the chain: `redb` (4.2.0),
`axum` (0.8.9), `tokio` (1.53.1), `serde` (1.0.228), `serde_json` (1.0.145). Hex, base64, CLI
parsing and the HTTP client used by `popcorn submit` are written out in-tree rather than taken
as dependencies, because §2 treats the dependency list as part of the audit surface.

### The browser client

The explorer and wallet in [`web/`](web/) are served by the node and are not consensus either —
but the distinction is finer than it looks. The page **produces** consensus data: it decides
what bytes a key signs and what blob is submitted, so it has to agree with the node exactly,
the way any other implementation does. What is free is everything around that: the framework
(none), the layout, the endpoints it happens to read.

Its dependencies are pinned by `web/package-lock.json` — `tlock-js` for the age/tlock blob,
`@noble/hashes` for BLAKE3 and SHA-256, `@noble/curves` for ed25519 when the key is held in the
tab rather than in a wallet, and `esbuild` to bundle. The bundle in `web/dist/` is committed and
compiled into the binary, and `web/dist/build.json` records the digest of every input so a
stale bundle fails CI instead of shipping. Agreement with the node is checked the same way
agreement between Rust, Go and Python is: `web/test/browser-path.sh`, one transaction per
action kind, every byte compared.

## Before genesis

The version numbers above must be re-read from `Cargo.lock` and frozen at the moment genesis is
produced, together with the hash of the lockfile itself. Until then this annex tracks the
working tree; after it, it is history.

An implementation in another language does not need these crates — it needs to agree with them,
byte for byte, on the fixtures in [`vectors/`](vectors/) and on the rejection cases of §3.6.
That agreement, not a shared dependency tree, is what makes verification reproducible.
