# Changelog

Version history of the POPCORN protocol specification ([`SPEC.md`](SPEC.md)).
All entries are **pre-genesis**: until genesis is produced, every change is free.
After genesis, anything listed as consensus-breaking in §13 is effectively a new chain.

## v0.9.3 — decryption is total (a remote halt, closed)

A prolonged hostile-load run against a live node found a critical defect: a single crafted
blob could permanently stop block production. A blob that passes POPCORN-TLOCK-AGE-V1 but
carries an inconsistent IBE point drives the pinned `tlock 0.0.10` to an `assert_eq!`
(`ibe.rs:313`) — a panic, not an error. The panic unwound a decryption worker, the producer's
`handle.join().expect(...)` re-panicked the producer task, and the node stopped producing
blocks while its API kept answering. Collection is blind (§5.1), so the trigger is input, not
volume: anyone who can POST a blob — everyone, by design, with no fee and no signature — could
halt the chain, and worse in release, where `panic = "abort"` turns the same input into a whole
-process abort.

- **§3.6 and §5.1 now state decryption as a total function.** For a profile-valid blob,
  decryption maps `(blob, beacon)` to a valid plaintext or to `unusable`, with no third
  outcome — a failed unwrap, a failed AEAD, an undecodable plaintext, *or an abnormal
  termination of the underlying primitive* all being `unusable`, identically for node and
  verifier. The rule is written as totality, not mechanism: it binds a Python verifier (whose
  primitive raises an exception) exactly as it binds Rust (whose primitive asserts). The
  accepted set does not move — for such a blob the outcome was previously *undefined* (a dead
  node), not a different verdict; the change fills a hole in the function.
- **The node realizes that totality by containing each blob's decryption** at the per-blob
  boundary in `blob::decrypt`; an abnormal termination becomes `TimelockError::Aborted`, an
  ordinary `unusable`. Node and verifier share this one function, so they still agree. The
  containment covers the whole class, not just `ibe.rs:313`: a grep of the decrypt-reachable
  code found sibling assertions and a `panic!` (`ibe.rs:208`, `:271`, `:313`, `:320`), any of
  which another crafted input could reach.
- **`panic = "unwind"` is pinned in the release profile and recorded in CONSENSUS-LOCK.md as
  consensus-relevant.** Under `abort` the containment is inert and the remote halt returns in
  full; a future change to `abort` "to shrink the binary" would reopen it. The pinned `tlock`
  is not vendored, so its assertion is left in place — under the totality rule a panic and an
  `Err` map to the same `unusable`, so patching it would have zero semantic effect and would
  only mean carrying a fork; the decision to contain rather than fork is recorded.
- **A committed vector (`vectors/halt.json`) and a regression test** freeze one such blob: it
  passes the profile, decrypts to `Aborted`/`unusable`, and does not crash the test. The
  cross-language gate gained a fourth section — all three implementations must profile-accept
  it, decrypt it without crashing, and produce no plaintext; a crash there would be an
  implementation halt, a successful decrypt there would be an unsound profile.
- **A new harness, `popcorn-node/examples/hostile_load.rs`**, drives sustained adversarial
  load with an honest canary underneath and checks survival, liveness, accountability and
  conservation against a running node. It is what found this. (An early canary bug — a
  zero-amount transfer, invalid by the `amount > 0` rule — was corrected to a self-transfer of
  one; the corrected run is what hit the real defect.)

## v0.9.3 — consensus identity stamped into genesis

- **`CONSENSUS_VERSION = 0x0000_0009_0003`**, the definitive value fixed by the freeze. It was
  `0x0000_0009_0002` — encoding 0.9.2 — while the document had been v0.9.3 for some time, and
  §13 ties the patch level to spec revisions. v0.9.3 changed rules that decide state: the
  grease-stanza policy of §3.6, §14.7, and the discriminants of §13.1.
- **It is now actually stamped.** §13 said "stamped into genesis and `/params`" and
  CONSENSUS-LOCK.md repeated it; neither was true. The version lived only in a compile-time
  constant: not in `GenesisConfig`, not in the stored metadata, not in any header, not in the
  state root. A node rebuilt with a different constant served a different `/params` over the
  same chain with nothing to notice, and a third party replaying from `/chain/export` had no
  way to check that the rules being applied were the rules the chain was created under. The
  one thing the identifier exists to pin was the one thing it was not bound to.
- `consensus_version` and `lock_digest` are now the first two fields of the `global` singleton
  (§5.4), written at genesis and never touched again. Being inside `global` puts them inside
  **every state root**: a verifier implementing different rules diverges at block 0 holding
  nothing but exported blocks. A node refuses to open a chain whose stamped identity is not
  its own instead of extending it, and `/params` reports the identity read from the chain
  state rather than from the binary.
- **The annex is stamped as a digest of the list**, not of the document
  (`5be582738ffa6616bcb899eab0bbdf93e08dd05e26f9cafe28ad7e8717d521cd`): a corrected typo in
  the prose must not change what a chain committed to, while a changed version number must.
  Length-prefixed so no name/version pair can be re-cut into another with the same digest.
  `ci/check-lock.py` fails the build when the stamped list stops matching what Cargo resolves.
- That gate found something on its first run: `sha2` resolves to **two** versions. Every
  cryptographic user is on 0.10.9; `age` pulls 0.11.0 through `rust-embed`, for localized
  error strings. Harmless, and now written down in the annex rather than tidied away — the
  check verifies the version on every *consensus* edge, not the absence of duplicates.
- The Python reference executor computes the same digest from the specification and agrees on
  every state root across the 400 differential scenarios; the end-to-end vector was
  regenerated against a real drand round.

## v0.9.3 — explorer and wallet

- **The node now serves its own front end** (`web/`, compiled into the binary; `--no-web`
  turns it off). An explorer — live blocks, block detail with manifest and rejections,
  accounts, tokens, pairs, the data board, the five-bucket supply — and a wallet that runs the
  client flow of §3.2 in the browser: sign with a Solana wallet or a key held in the tab,
  timelock-encrypt toward a future round, de-armor, submit blind, keep the receipt, then check
  inclusion against the block rather than against the node's word for it.
- Serving it from the node is not convenience. The flow puts a signature and an encryption in
  a browser, so a page fetched from a third party is a page that can be swapped for one that
  signs something else. Served by the node it is same-origin with the API, loads nothing from
  any other origin, and says so in a `Content-Security-Policy` with no `unsafe-inline`.
- **New gate: `web/test/browser-path.sh`.** The page decides what bytes a key signs, which
  makes it consensus-relevant code however it is served — and a browser that encodes a payload
  differently does not fail loudly, it signs something nobody asked for. The gate builds one
  transaction of every one of the fourteen action kinds through the page's own bundle, then
  has the Rust inspector confirm the bytes re-encode identically and the account id, tx id,
  signing hash and `verify_strict` all agree, has Rust and Go accept the blob under
  POPCORN-TLOCK-AGE-V1, decrypts it back to the signed bytes, and checks that an armored blob
  is still refused. It runs offline against round 1000.
- §3.2 and §9.1 record the page and its four routes; §13.3 already classified serving as free,
  and nothing here changes that.
- **`--cors` for third-party front ends.** The API sent no `Access-Control-Allow-Origin`,
  which was right for the page the node serves — same-origin, nothing needed — and wrong for
  anyone building their own browser client: it was blocked outright. The flag takes `*` or a
  list of origins, answers preflights (so `POST /tx`, which has no `OPTIONS` route, works),
  matches origins exactly rather than by prefix, and stays off by default. It is documented
  as what it is: **not** a security boundary. This API has no cookies, sessions or
  authorization, every endpoint answers the same to everyone, and `POST /tx` takes bytes from
  anyone by design — so the header decides whether third-party pages need a proxy, not who may
  read the chain. `Allow-Credentials` is never sent, because there are none.
- `web/README.md` documents the reusable modules — `borsh.js`, `popcorn.js`, `actions.js`,
  `wallet.js`, `api.js`, shipped together as `dist/popcorn.mjs` — for people writing their own
  client, including the three things that are easy to get wrong (de-armoring, the signed
  target round, `BigInt` amounts) and the fact that pools have no owner: a `Pair` carries no
  creator, its id is derivable by anyone, and "a front end for my pools" is a client-side
  filter on pair ids, not an on-chain relationship.
- Two client-side traps closed while testing it against a live chain: an HTLC passphrase now
  becomes the 32-byte preimage (hashing a passphrase straight into the hashlock produced an
  escrow whose preimage was the wrong length, so it could never be claimed — only refunded
  after expiry), and the default target round moved from two rounds ahead to eight, because a
  wallet prompt plus a BLS encryption in a tab does not fit in six seconds.

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
