# Changelog

Version history of the POPCORN protocol specification ([`SPEC.md`](SPEC.md)).
All entries are **pre-genesis**: until genesis is produced, every change is free.
After genesis, anything listed as consensus-breaking in §13 is effectively a new chain.

## v0.9.3 — final coherence (freeze candidate)

- Removed the three stale mentions of accumulator dust that contradicted v0.9.2 (§7.1, the
  `native_emitted` definition in §7.2, §13).
- Pinned the three remaining open cases: static `amount > 0` for `Transfer` / `Stake` /
  `Unstake`; the sub-order of validation step 4 (`PubkeyMismatch` before `NonceExhausted`);
  `amount_out ≥ reserve_out` in exact-out ⇒ `Failed(SlippageExceeded)`.
- `CONSENSUS_VERSION` expressed as three 16-bit fields.
- Blob mirroring promoted from optional to an **operational obligation** of the operator
  (outside consensus) — without it the collection audit is not practicable.

## v0.9.3-en — English edition, reference implementation

The specification was translated to English and restructured; normative content is unchanged
except where implementation forced a decision, which is recorded in §14. Three of those are
corrections rather than clarifications:

- **§5.5, the monetary invariant was missing a bucket.** The invariant listed four places a
  native unit can live and omitted **AMM pool reserves**. A pair may hold `NATIVE_TOKEN` on
  either side, and those units leave a balance to get there — so the equality became false the
  moment anyone provided native liquidity. Since §10 checks it at *every block*, an honest
  chain would have failed its own verification as soon as a native pool was funded. Found by
  the independent reference executor of §10: both implementations agreed with each other and
  with the old text, and both reported the invariant broken, which is what a spec bug looks
  like from the inside. The invariant is now five buckets.

- **§3.6, grease stanzas.** "Exactly one recipient stanza" was unimplementable against the
  pinned `age` version, which appends a randomized `-grease` stanza to every header it writes.
  The profile now admits one `tlock` stanza plus at most one grease stanza, and refuses every
  other stanza type. The consequence — one transaction can be encrypted into unlimited
  distinct blobs — is declared in §3.6.
- **§3.2, armored JavaScript output.** `tlock-js` returns an armored age file, which the
  profile forbids. JavaScript clients must de-armor before submitting; without it every
  browser-submitted blob would be `unusable`. A client requirement, not a consensus change.
- **§14.7, `CreatePair` check order.** The LP-token check must precede the existence check, or
  `LpTokenAsPairSide` is unreachable and its discriminant is dead in a committed enum.

## v0.9.2 — staking is literal Synthetix (fixes the P0 on `reserved ≥ Σ pending`)

The inequality declared in v0.9.1 was false. Counterexample: the dust routed to the foundation
was exactly what pending claims would have drawn → the first claim underflows → halt.

Fixed by literally transcribing the audited Synthetix `StakingRewards` math:

1. `staking_reserved += staker_share` **whole** — accumulator dust no longer exists, and the
   foundation is capped at EXACTLY its nominal 15%.
2. `reward_debt` (in units) replaced by **`paid_acc`** (an accumulator snapshot, Synthetix's
   `userRewardPerTokenPaid`), with `pending = ⌊staked × (acc − paid_acc) / P⌋` — **a single
   floor over the difference**, always ≤ the true entitlement.

Solvency now holds by construction: each batch adds `staker_share` both to `reserved` and to
the entitlement pot; each settle pays ≤ the accrued entitlement ⇒ `reserved ≥ Σ pending ≥ 0`,
always. The property-test gate was extended to assert `reserved ≥ Σ pending ≥ 0` after every
operation — four-bucket equality alone did not catch the bug.

Also closed seven inconsistencies: `CONSENSUS_VERSION` alignment, `ROUND_TOO_LATE` clarified as
removed, `SupplyOutOfRange` declared unreachable, the `SelfTransferNoop` rule written down, the
position of `NonceExhausted` pinned, legacy pre-age text replaced, a v0.8.4 changelog typo.

## v0.9.1 — closing the two P0s mathematically, plus P1/P2 formalizations

**P0-A** — new accounting bucket **`staking_reserved`** in `global`: the monetary invariant
becomes an EXACT four-bucket equality by construction (the old invariant stated with
`Σ pending` was mathematically false because of double flooring). Two distinct rounding layers:
accumulator dust → foundation; per-account residue → protocol liability, never burned and never
attributed. A consensus-grade property test becomes the gate.

**P0-B** — fairness reformulated as what is actually provable: **pre-beacon blindness + receipt
accountability + deterministic ordering + state root verifiability**. Consensus does NOT
temporally bind collection closure to the beacon (the old §5.1 claimed A while implementing B);
per-blob protection is the receipt obtained before the deadline — client policy, not consensus.

**P1** — `GENESIS_DRAND_ROUND` and the normative mapping `round(h) = G + h − 1` with empty-block
catch-up (rounds are never skipped: wall-clock stays out of consensus and HTLC windows stay
whole); the POPCORN-TLOCK-AGE-V1 profile; normative discriminant tables for `RejectReason` and
`FailReason`; `AddLiquidity` with actual amounts; the explicit exact-out sequence; `unusable` as
derived evidence with a normative derivation; the CONSENSUS-LOCK annex for exact versions.

**P2** — roots over empty lists; a Borsh-canonical receipt; canonical ordering of Publish feeds;
encoding of the `global` singleton; beacon error taxonomy (`ROUND_TOO_LATE` REMOVED from
consensus: lateness is telemetry, not a ledger category); per-case overflow including
`NonceExhausted`; a formal tx_id tie-break; the scope of the k-invariant; wording on node-key
compromise.

## v0.9 — consensus becomes normative

1. New §13 **POPCORN-CONSENSUS**: every semantics that can influence the state root is defined
   algorithmically with a pinned version, an explicit `CONSENSUS_VERSION`, and a
   breaking/non-breaking change classification — determinism is normative, not emergent.
2. ed25519 signatures = **`ed25519_dalek::verify_strict` at a pinned version**, as a consensus
   rule (one semantics, chosen and frozen; ZIP-215 only if batch verification is ever needed).
3. **Zero unordered collections in the commitment** as an architectural policy, not merely a
   hashbrown ≥ 0.15.1 patch, plus a mandatory canonicity test.
4. **`TimelockProvider`** as the single interface toward tlock/drand, with consensus-defined
   policy for a missing or invalid beacon.
5. Declared: the timelock is fair ordering on a short horizon, NEVER long-term storage
   (`BLOB_ROUND_HORIZON`).
6. The alloy-primitives migration demoted to P3, post-freeze.
7. Mandatory consensus-grade test vectors and an independent reference executor; XOF stream
   advancement on rejection made explicit.

## v0.8.4 — protocol freeze audit

- **Blob format = age with a tlock recipient** (`tlock_age`): the raw tlock primitive encrypts
  16 bytes, and the key+AEAD hybrid the spec had been describing by hand is exactly what the age
  format standardizes. The custom layer is gone; Rust↔Go↔JS interop is guaranteed by the shared
  format, with cross-language test vectors as a CI gate.
- The DoS surface of blind collection frozen as **operational requirements outside consensus**,
  plus a mandatory pre-genesis worst-case benchmark.
- Foundation semantics with `total_staked == 0` rewritten unambiguously (nominal 15% = 100% of
  that batch's effective emission).
- AMM validation rules made explicit.
- `undecryptable` renamed **`unusable`** (it covers failed tlock, failed AEAD and failed decode —
  a Borsh failure is decryptable yet unusable).
- `collection_root` commits to the **set** of distinct blobs, not the multiplicity of
  submissions.

## v0.8.3 — accounting fixes from review

1. Fees are collected in a **single phase** after ordering and before execution — the `min()`
   clamp disappears, every transaction always pays the full fee, and the "drain the balance with
   the first tx and the rest ride free" bypass is closed.
2. §8 mandates **U256 intermediates** (`S × acc` overflows u128 in the dust-staker case; results
   fit back in u128 thanks to supply bounds).
3. Four pins: the hashlock index is a derivable cache outside the state root; auto-settlement
   happens AFTER execution (a claim in the same batch wins, and the publish becomes a no-op);
   pubkey materialization only on executed transactions, never on rejected ones; a Borsh failure
   is `unusable`.

Minor notes: `HtlcDuplicateHashlock` griefing, the receipt timestamp is not committed, the
manifest is a set, `Unstake` at zero balance.

## v0.8.2 — collection commitment (commit-then-decrypt)

The node commits to the set of blobs it received BEFORE it can decrypt them: `collection_root` in
the header, `blob_manifest` and the `unusable` list in the block, and complete manifest →
txs/rejected/unusable accounting verifiable by anyone holding the public beacon.

Selective post-decrypt censorship becomes either a signed self-contradiction (receipt vs
manifest) or a publicly falsifiable lie (a refutable `unusable` claim). Precedent: Shutter's
commit-then-decrypt and the accountability of Ethereum inclusion lists, adapted to a single node.
Declared residue: blind refusal at ingress (no receipt issued).

## v0.8.1 — HTLC auto-settlement via Publish

At batch close, every `Publish` whose `data` is exactly 32 bytes and whose `sha256(data)` matches
an open HTLC's hashlock automatically settles that HTLC toward the recipient. The preimage
becomes carrier-independent (anyone can deliver it, the recipient may be offline) and censoring
settlement requires blind mass censorship, self-incriminating through §9.2 receipts.

`HtlcClaim` remains as the direct route. A `hashlock → htlc_id` index is added to state.
Declared edge case: inclusion remains the only gate (a limit of the model, §1).

## v0.8 — HTLC (non-custodial user-side bridging)

Three actions — `HtlcLock` / `HtlcClaim` / `HtlcRefund` — for user-carried atomic cross-chain
swaps. Hashlocks use **SHA-256** (the lingua franca of Bitcoin/Lightning/EVM/Solana — the only
non-blake3 point in the protocol, for interoperability) with a fixed 32-byte preimage. Refund is
invocable by anyone (garbage collection); HTLCs have a maximum lifetime; the `htlcs` table joins
the state root and the monetary invariant is extended. Security notes from the literature:
MAD-HTLC/bribery, timeout staggering, the free option.

## v0.7.2 — protocol fixes

Account bootstrap repaired (`signer_pubkey` in `SignedTx`, P2PKH model: the key materializes on
first signature, since ed25519 has no key recovery); the uniformity proof for `uniform(n)` added
as a comment (the algorithm was already correct); a liquidity guard on `Stake` (a fee to exit
always remains); `rejected_root` also commits to the `RejectReason`; the submission receipt
requalified as *evidence of receipt*; drand/tlock interoperability pinning with cross-language
test vectors; the block commitment made explicit (the Header is what is signed).

## v0.7 — fair launch

`GENESIS_SUPPLY = 0` (no allocation to the creator); when `total_staked == 0` the staker share
**is not emitted** — it is never born, neither to the foundation nor burned — so the foundation
is capped at 15% of emission from the very first batch. Maximum supply ≈ 21.02 M; bootstrap via
the foundation share from block 1.

## v0.6 — user data and Solana-level fees

New `Publish` action (a data board for user-carried oracles — no state effect, the data lives in
the block); `FEE_TX` aligned with Solana (5,000 units = 0.000005 native); a per-byte surcharge on
publishes above the free threshold.

## v0.5.2 — closing the books before implementation

Staking dust accounted for (→ foundation); a frozen definition of `native_emitted`; the moment of
emission fixed (batch close, not spendable within the same batch); the static validation pipeline
ordered with a duplicate tie-break; transaction identity declared; the effective cap in exact
form; fee wording corrected.

## v0.5.1 — accounting rigour

The supply cap defined as an upper bound (effective emission is computed by the verifier batch by
batch); `pending` declared a derived accounting liability; fee solvency in static validation with
a runtime fallback; foundation staking declared explicitly legitimate.

## v0.5 — new economy and access

- Faucet, invites and `CreateAccount` removed: accounts are implicit on first receipt of funds
  (the Ethereum model).
- A foundation pool at genesis with `GENESIS_SUPPLY` (a public account like any other).
- Per-batch emission with halving (Bitcoin style): finite total supply in closed form, split
  85/15 between stakers and the foundation.
- Fees burned (EIP-1559 style): the old 50/50 split is gone.
- Solana wallet compatibility (Phantom/Solflare via `signMessage`): client flow and signing
  domain defined.

## Earlier

- **v0.4** — frozen semantics: nonce + shuffle, failure handling, domain-separated IDs, state
  root, header.
- **v0.3** — tlock and drand_core vendored, redb storage, shuffle over blake3 XOF, per-round
  beacon fetch with multiple endpoints.
- **v0.2** — multi-hop, fee tiers, exact-out.
