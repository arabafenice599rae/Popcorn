<p align="center">
  <img src="assets/popcorn-logo.jpg" alt="POPCORN" width="560">
</p>

<h1 align="center">POPCORN — Technical Specification</h1>

<p align="center">
  <strong>v0.9.3 (freeze candidate)</strong> · pre-genesis · <code>CONSENSUS_VERSION = 0x0000_0009_0002</code>
</p>

---

A **single-node chain** with a native economy and a **finite maximum supply**. Honest
classification: a *single-operator deterministic/verifiable execution chain*. There is no
distributed consensus and no P2P layer — trust is replaced by **verifiability**.

**Naming.** The chain is called **POPCORN** (formerly ARENA-CHAIN). The name is a
**consensus identifier, not a marketing variable**: it enters signed preimages
(`SIGN_DOMAIN = "popcorn-v1"`, receipt domain `"popcorn-receipt-v1"`, the
POPCORN-TLOCK-AGE-V1 / POPCORN-CONSENSUS / POPCORN-V2-MATH profiles). Renaming is free
pre-genesis; post-genesis it would have been consensus-breaking. Any future occurrence of
ARENA / arena-chain, in any form, is stale by construction.

Version history lives in [`CHANGELOG.md`](CHANGELOG.md).

## Contents

- [0. About this document](#0-about-this-document)
- [1. Trust model](#1-trust-model)
- [2. Dependencies](#2-dependencies)
- [3. Cryptographic primitives and clients](#3-cryptographic-primitives-and-clients)
- [4. Identifiers and data formats](#4-identifiers-and-data-formats)
- [5. Batch lifecycle](#5-batch-lifecycle)
- [6. AMM math — POPCORN-V2-MATH](#6-amm-math--popcorn-v2-math)
- [7. Economics](#7-economics)
- [8. Staking — O(1) distribution](#8-staking--o1-distribution)
- [9. Node API](#9-node-api)
- [10. Third-party verification](#10-third-party-verification)
- [11. Protocol parameters](#11-protocol-parameters)
- [12. Declared out of scope](#12-declared-out-of-scope)
- [13. POPCORN-CONSENSUS — normative definition](#13-popcorn-consensus--normative-definition)
- [14. Implementation clarifications](#14-implementation-clarifications)

---

## 0. About this document

**MUST**, **MUST NOT**, **SHOULD** and **MAY** carry their RFC 2119 meaning. Everything
labelled *frozen* is settled: changing it post-genesis is consensus-breaking per §13.

Three classes of statement appear here, and they are never interchangeable:

| Class | Meaning |
|---|---|
| **Consensus rule** | Affects the state root. Every implementation MUST reproduce it bit for bit. |
| **Operational requirement** | Binds the operator, not the ledger. Violating it is visible but does not fork the chain. |
| **Client policy** | Binds nobody. It is the rational behaviour of a participant given what consensus does and does not guarantee. |

Conflating the three is how protocols end up promising what they do not deliver; §1 and §5.1
exist precisely to keep them apart.

---

## 1. Trust model

- **One node** (the operator). No distributed consensus, no P2P.
- Trust is replaced by **verifiability**: anyone downloads the chain and re-executes state
  from zero, **including every emission and every burn** — the money supply is fully
  reconstructible from replay.
- **Separate keys (frozen).** The **node key** (signs blocks, hot on the server) and the
  **foundation key** (holds value, cold, offline) are **distinct** ed25519 keys, both
  declared in genesis. The node key controls no funds; the foundation key signs no blocks.
- Selling or distributing the native token against external assets (SOL, fiat, …) happens
  **off-chain and outside the protocol**: the chain custodies no other network's assets
  (no bridge).

### 1.1 The four real guarantees

POPCORN does not promise censorship resistance. It promises, and can prove:

1. **Pre-beacon blindness** — the timelock prevents the operator from learning the plaintext
   of any blob before the round's beacon exists.
2. **Deterministic ordering** — execution order is a function of the beacon, never an
   operator choice.
3. **Receipt accountability** — for any receipted blob, omission from the manifest is a
   contradiction between two signatures by the same node.
4. **State root verifiability** — any accounting or execution incorrectness diverges under
   replay.

The operator cannot rewrite history, emit outside the formula, or alter supply without a
`state_root` divergence.

### 1.2 What consensus does NOT guarantee (declared)

The node is not mathematically forced to close collection before the beacon: it MAY wait for
the beacon, decrypt, and only then choose what to manifest, producing a formally valid block.
Protection against that window is **per-blob**: the **receipt obtained before the deadline**
(§9.2).

POPCORN provides strong accountability for blobs the node has receipted — **not a universal
cryptographic proof that every packet ever sent was received**. That is the chosen limit of
the single-operator model, not a hidden defect.

The operator **can** censor transactions (detectable via the signed receipt of §9.2, not
preventable) and holds the key to the `FOUNDATION` account — whose movements are public and
traced like anyone else's.

### 1.3 Key compromise

- **Node key compromised**: cannot spend funds, but can produce a **validly signed fork**.
  The divergence is publicly detectable; choosing the canonical history remains a property of
  the single-operator model, not something distributed consensus protects here. Operational
  mitigations: timestamped external mirrors, observers archiving headers.
- **Foundation key compromised**: foundation funds are lost, the ledger stays intact.

---

## 2. Dependencies

All Rust, a single binary, **zero native C/C++ code**.

### 2.1 External crates (10 — `serde` and `serde_json` counted separately)

| Crate | Role | Items used | Audit status |
|---|---|---|---|
| `tokio` | Async runtime | `#[tokio::main]`, `tokio::spawn`, `tokio::time::interval` | Battle-tested |
| `axum` | HTTP/WebSocket server | `Router`, `routing::{get, post}`, `extract::{State, Json, Path}`, `WebSocketUpgrade` | Battle-tested |
| `ed25519-dalek` v2 | Account and block signatures | `SigningKey`, `VerifyingKey`, `Signature`, `Signer::sign`, `verify_strict` | curve25519-dalek audit (2023) |
| `blake3` | Hashing + XOF | `blake3::hash`, `Hasher::{new, update, finalize, finalize_xof}`, `OutputReader::fill` | Official implementation by the authors |
| `borsh` | Canonical serialization | `BorshSerialize`, `BorshDeserialize`, `borsh::to_vec`, `borsh::from_slice` | Battle-tested (NEAR, Solana) |
| `age` | Encrypted blob format (used through `tlock_age`) | payload encryption/decryption in the age format (internal STREAM ChaCha20-Poly1305) | Publicly specified format, battle-tested implementation; interop is a property of the format |
| `sha2` | HTLC hashlocks only (§7.6, cross-chain interop) | `Sha256::digest` | RustCrypto, family covered by the NCC audit (2020) |
| `primitive-types` | U256 for the AMM | `U256::{from, checked_mul, checked_div, integer_sqrt}` | Battle-tested (Parity) |
| `redb` | Storage (single file, ACID, pure Rust) | `Database::create`, `TableDefinition`, `begin_write`, `begin_read`, `open_table`, `insert`, `get`, `commit` | Stable format, maintained, continuous fuzzing |
| `serde` + `serde_json` | API layer only (never for hashing or state) | `Serialize`/`Deserialize` derives | Battle-tested |

### 2.2 Crates vendored in the workspace (3)

| Crate | Origin | Rationale |
|---|---|---|
| `tlock` | thibmeu/tlock-rs (MIT, ~750 SLoC) | Upstream idle since 2024; protocol frozen → pinned internal fork, inspectable. **Unaudited**; peer-reviewed scheme, interop with drand/tlock Go |
| `drand_core` | thibmeu/drand-rs (MIT, ~1,500 SLoC) | As above. BLS verification against pinned chain-info; unchained/G1 |
| `tlock_age` | thibmeu/tlock-rs (MIT) | The blob format: age with a tlock recipient. The raw `tlock` primitive encrypts 16 bytes (the file key); `tlock_age` uses it inside the age format for arbitrary payloads — the standardized "timelock key + AEAD" hybrid, interoperable with drand/tlock Go and tlock-js |

Functions: `tlock::encrypt(&mut dst, src, &pubkey, round)`, `tlock::decrypt(&mut dst, src, &signature)`; `drand_core::HttpClient`, `chain_info()`, `get(round)`, `ChainInfo::public_key()`, `Beacon::{signature(), round()}`.

### 2.3 Hard rules

1. Anything entering a hash or the state goes through **Borsh, never JSON**.
2. The blob is **age with a tlock recipient** (§3).
3. **Zero unordered collections in the commitment** (*architectural policy*). In serialized
   state and in any structure touching a root, only `BTreeMap` / `BTreeSet` / explicitly
   ordered `Vec` / arrays / primitives are allowed. `HashMap`, `HashSet` and any unordered
   iteration are **forbidden**, *independently* of the borsh/hashbrown version
   (reference: RUSTSEC-2024-0402, non-canonical encoding → consensus split; the
   hashbrown ≥ 0.15.1 patch is due but does not replace the policy).
   CI enforces a gate that fails if a consensus structure introduces a forbidden collection,
   plus a **canonicity test**: same logical state, different insertion orders → identical
   bytes → identical `state_root`.

### 2.4 Interoperability pinning (frozen at genesis)

- `DRAND_SCHEME = bls-unchained-g1-rfc9380`
- `DRAND_CHAIN_HASH = 52db9ba70e0cc0f6eaf7803dd07447a1f5477735fd3f661792ba94600c84e971` (quicknet)
- The exact commit of the vendored `tlock` / `tlock_age` / `drand_core` forks is recorded in
  genesis (profile: the raw 16-byte tlock primitive is NOT modified; payloads go through the
  age format).
- A **cross-language test vector suite** lives in the repository as a CI gate
  (`interop/run-gate.sh`): every one of the three implementations encrypts, and the other two
  decrypt, in all nine directions; all three then return **identical verdicts** on the shared
  rejection vectors in `vectors/profile/`.

That suite is the precondition for "reproducible verification in any language" to hold for
encryption too, not just for replay.

> **Ciphertexts are not byte-identical across implementations, and must not be expected to
> be.** tlock encryption is randomized, and `age` greases its headers (§3.6), so the same
> plaintext encrypts differently every time even within one implementation. What the gate
> compares is the **plaintext after a round trip** and the **acceptance verdict** — those are
> the two things a disagreement could fork the chain over.

---

## 3. Cryptographic primitives and clients

### 3.1 Identity and signatures

- **Account identity**: ed25519. `AccountId = blake3(verifying_key)` (32 bytes). Same curve
  as Solana: **a Phantom/Solflare keypair is a valid identity**.
- **Signing domain (frozen)**: the signature is ed25519 over
  `blake3(SIGN_DOMAIN || borsh(payload))` with `SIGN_DOMAIN = "popcorn-v1"` (ASCII, 10 bytes).
  This prevents cross-chain reuse of signatures produced by Solana wallets and vice versa.
- **Verification semantics (consensus rule)**:
  `signature_valid = ed25519_dalek::<PINNED_VERSION>::verify_strict(...)` — not "any standard
  Ed25519". RFC 8032 is under-specified and implementations diverge on borderline signatures;
  a replaying verifier MUST accept and reject EXACTLY the same signatures as the node, so the
  semantics are defined by the algorithm, version included.
  `verify_strict` (rejects small-order points and non-canonical `s`) is the frozen choice for
  the single-signer model. It is NOT "intrinsically better" than ZIP-215 — it is ONE
  semantics, chosen and frozen. Should batch verification ever be needed, migrating to
  ZIP-215 (`ed25519-zebra`) would be a declared consensus change.
  Test vectors with borderline signatures (torsion components, non-canonical `R`/`A`) are
  mandatory, and node and verifier MUST treat them identically.

### 3.2 Client flows

- **Human client (Solana wallet)**: web frontend → Wallet Adapter
  `signMessage(blake3(SIGN_DOMAIN || borsh(payload)))` → encryption with **tlock-js** toward
  the target round → **de-armor** (below) → `POST /tx`. No emulated Solana RPC: the wallet
  signs, our client talks to our API.

  > **De-armoring is mandatory for JavaScript clients.** `tlock-js` returns an **armored**
  > age file (`-----BEGIN AGE ENCRYPTED FILE-----`), and armor is forbidden by the profile
  > (§3.6) — it is a second encoding of the same ciphertext, so accepting it would give one
  > transaction two blob hashes, and both the manifest and the receipts key on that hash. A
  > client strips the PEM wrapper and base64-decodes the body before submitting; the Rust and
  > Go clients emit binary already. The cross-language gate covers this, and
  > `interop/js/interop.mjs` is the three-line reference.
- **Bot client**: an ed25519 keypair in a file plus an HTTP client; encryption with tlock
  (Rust), tlock-js (JS/TS) or drand/tlock (Go) — all interoperable.

### 3.3 Beacon

drand **quicknet** (unchained, 3 s). The chain hash is pinned in genesis and every beacon is
BLS-verified against the pinned `ChainInfo`. Fetch is **always by round** (`get(R)`), never
`/latest`; fallback proceeds in order through `DRAND_REMOTES`.

**Beacon policy (consensus rule)** — enumerated outcomes, each with ONE deterministic
consequence; nothing is left to the runtime:

| Outcome | Consequence |
|---|---|
| `ROUND_AVAILABLE` (BLS signature verified) | block `R` is produced |
| `ROUND_NOT_AVAILABLE` (no remote answers) | retry with backoff, **the chain waits**: no block for `R` until the beacon arrives. Never skip, never fall back to other randomness |
| `ROUND_INVALID` (BLS signature fails) | treated as NOT_AVAILABLE for that remote; if ALL remotes return an invalid signature for the same round, the node **halts** (explicit halt, never a block with an unverified beacon) |

Beacon *lateness* is NOT a consensus category: it is liveness and telemetry. For the ledger
there is only "valid beacon for the expected round".

**Fetch error taxonomy**: `FETCH_FAILURE` (HTTP/timeout), `MALFORMED_RESPONSE`, `WRONG_ROUND`
and `WRONG_CHAIN` are *remote* errors → next remote / retry (equivalent to NOT_AVAILABLE).
Only `INVALID_SIGNATURE` **from every trusted source** for the same round leads to halt.

### 3.4 Round → block mapping (consensus rule)

```
round(h) = GENESIS_DRAND_ROUND + h − 1        for every h ≥ 1
```

`GENESIS_DRAND_ROUND` is stamped into genesis. **Rounds are never skipped**: after downtime
the node catches up by producing the missing blocks in sequence — beacon for the expected
round → empty collection → empty block (with its emission) → next round. There is no special
catch-up rule; the "gap" case does not exist, only the sequence does.

Wall-clock time never enters consensus, and HTLC windows stay whole by construction: an
expiry falling inside downtime is *crossed*, never *jumped over*.

### 3.5 TimelockProvider and the timelock horizon

All tlock/drand logic lives behind a single trait
(`encrypt` / `decrypt` / `chain_hash` / `round_for_time` / `get_beacon`); the state machine
does not know which implementation sits underneath. The timelock dependency is
**substitutable by construction** — tlock-rs today, a native implementation over audited BLS
tomorrow — not an inseparable part of the architecture.

The node accepts blobs only for rounds within `BLOB_ROUND_HORIZON` of the current round.
**POPCORN's timelock is fair ordering on a horizon of seconds to minutes, NEVER long-term
storage**: a drand sunset (precedent: fastnet, key material destroyed) would affect chain
availability, not the decryptability of distant ciphertexts — which by construction do not
exist.

### 3.6 POPCORN-TLOCK-AGE-V1 (consensus rule)

age is an extensible format: POPCORN does not accept "any valid age ciphertext", only this
profile.

| Requirement | Value |
|---|---|
| Format | age v1, binary |
| Armor | **forbidden** |
| Recipient stanzas | **exactly one** of type `tlock`, toward the target round, plus **at most one** grease stanza (below) |
| Encoding | canonical (canonical base64 without padding, lowercase hex, LF only) |
| Round argument | canonical decimal, no leading zeros |
| Chain hash argument | the pinned `DRAND_CHAIN_HASH`, lowercase hex |
| Header size | ≤ 1 KiB, from the first byte through the MAC line |
| Payload | STREAM v1, and non-empty |

Any deviation ⇒ `unusable`. The **acceptance policy is normative**: node and verifiers MUST
accept and reject the same bytes — the cross-language test vectors cover rejection cases, not
just round-trips.

**The grease stanza (amended v0.9.3-en).** Earlier text said "exactly one recipient stanza",
full stop. That rule is **unimplementable** with the pinned stack: the `age` implementation
appends a randomized stanza tagged `<random>-grease` to every header it writes, by design, to
keep parsers from ossifying. Enforcing "exactly one" would reject every blob produced by the
Rust client this document itself names as interoperable. The rule is therefore:

- exactly one stanza of type `tlock`, carrying the target round and the pinned chain hash;
- at most one additional stanza whose type ends in `-grease`, which is **ignored**;
- any other stanza ⇒ `unusable`.

Refusing foreign stanzas is the part that carries weight: a second recipient stanza would be a
decryption path for someone other than the round. A grease stanza cannot become one — no
implementation unwraps a file key from an unknown tag — and the header cap bounds the
attacker-chosen bytes it can carry.

One consequence follows and is declared rather than hidden: because the grease stanza is
random, the same transaction encrypts to **different blobs** on every attempt. A client can
therefore mint unlimited distinct blobs for one transaction. `collection_root` deduplicates
identical blobs only, so these arrive as distinct manifest entries; the logical duplicate is
still resolved by nonce deduplication (§5.2, step 6), and the volume is bounded by the
wire-level admission limits of §11, not by consensus.

**Transaction encryption (frozen)**: the blob is the Borsh payload encrypted in the **age
format with a tlock recipient toward round `R`** (`tlock_age`): age generates the file key,
protects it with the tlock primitive (16 bytes, as is native to it), and encrypts the payload
with STREAM/ChaCha20-Poly1305 — the hybrid is the format, not our code. Client interop:
**drand/tlock (Go), tlock-js, tlock_age (Rust)** produce and read the same ciphertext.

### 3.7 Deterministic shuffle

Fixed by this specification, not by a library.

```
seed_stream = blake3::Hasher::new()
                .update(drand_signature_R)      // raw bytes of the BLS signature
                .update(LE64(height))
                .finalize_xof()

next_u64():  8 bytes from the stream (OutputReader::fill), little-endian

uniform(n):  limit = u64::MAX - (u64::MAX % n)   // = ⌊(2⁶⁴−1)/n⌋·n → exact multiple of n
             loop { x = next_u64(); if x < limit { return x % n } }
             // on rejection: the NEXT 8 bytes from the stream (the stream always
             // advances, never re-reads) — part of the consensus definition

// UNIFORMITY PROOF: the accepted set is [0, limit), of cardinality limit, an exact
// multiple of n by construction → x % n is exactly uniform over [0, n).
// (For n a power of two, n values are rejected at the tail instead of 0: negligible
// waste, uniformity intact.)

// Fisher-Yates over the valid tx list sorted by ascending tx_id (lexicographic bytes)
for i in (1..len).rev():
    j = uniform(i + 1)
    swap(txs[i], txs[j])
```

---

## 4. Identifiers and data formats

### 4.1 Identifiers — domain separation (frozen)

Every ID is `[u8; 32]`. The first byte of the preimage is a **domain tag**: no collisions
between namespaces.

```
NATIVE_TOKEN  = [0x00; 32]                                        // constant
TokenId       = blake3(0x01 || creator: AccountId || LE64(payload.nonce))
LpTokenId     = blake3(0x02 || PairId)
PairId        = blake3(0x03 || token0 || token1 || LE16(fee_bps))  // token0 < token1 (lexicographic)
tx_id         = blake3(borsh(SignedTx))
HtlcId        = blake3(0x04 || sender: AccountId || LE64(payload.nonce))
FOUNDATION    = AccountId of the foundation key, declared in genesis
```

**Transaction identity (frozen)**: `tx_id` covers the complete `SignedTx`, **signature
included** — it is the hash of what is actually transmitted and executed. Since a signer can
produce different signatures for the same payload, the same intent can yield several tx_ids;
nonce deduplication (§5.2, step 6) guarantees at most one of them executes.

**Identity invariant (frozen)**: for every account with `pubkey == Some(pk)`,
`AccountId == blake3(pk)`. The pubkey materializes on the account's first included signed
transaction (§4.2) and is never reassignable. Coherence holds by construction: the node
**always** derives `signer = blake3(signer_pubkey)` from the transaction field, never from a
reverse lookup.

**Size limits (frozen)**: `MAX_BLOB_SIZE` is a **wire-level** limit on the whole encrypted
blob (POPCORN-TLOCK-AGE-V1 format, §3) accepted by `POST /tx`; `MAX_PUBLISH_SIZE` limits the
cleartext `Action::Publish.data` field. They are independent: a 512 B cleartext `Publish`
must still fit, encrypted and with overhead, inside the 2 KiB wire limit.

**LP tokens are first-class tokens**: they live in `balances` under an `LpTokenId`, so
`Transfer` works on them with no dedicated code. `token0`/`token1` in a `PairId` may be
`NATIVE_TOKEN` or a `TokenId`, **never** an `LpTokenId` (no pools of LP tokens: rejected in
validation).

### 4.2 Implicit accounts and key materialization (frozen)

There is no account-creation action. An `Account` record is born automatically (nonce 0,
empty balances, `pubkey = None`) the **first time it receives funds**: an incoming
`Transfer`, an LP credit, a staking payout, a swap output. The sender only names
`to: AccountId` — it need not know the recipient's key.

**Materialization**: ed25519 has no key recovery, so the verifying key must travel
explicitly — it sits in the `signer_pubkey` field of every `SignedTx`. On the account's
**first executed signed transaction** (`Ok` or `Failed`, i.e. present in `txs`),
`account.pubkey` goes from `None` to `Some(signer_pubkey)`, permanently. `rejected`
transactions do **not** materialize it: fixing the key without paying a fee must not be
possible. This is Bitcoin's P2PKH model: you pay to a hash, the key is revealed on first
spend.

An account with no native balance cannot transact (it cannot cover `FEE_TX`): bootstrapping a
new user means receiving native from someone (the foundation, another user, an internal
exchange).

### 4.3 Structures

```rust
type AccountId = [u8; 32];
type Amount    = u128;       // 9 decimals for every token

struct Account {
    pubkey: Option<[u8; 32]>,              // None until the account signs its first tx (§4.2)
    nonce: u64,                            // last EXECUTED nonce (starts at 0)
    balances: BTreeMap<[u8;32], Amount>,   // NATIVE_TOKEN | TokenId | LpTokenId
    staked: Amount,
    paid_acc: u128,   // snapshot of acc_per_stake at the last settle
                      // (Synthetix userRewardPerTokenPaid)
}

struct Token {
    id: [u8; 32],
    creator: AccountId,
    name: [u8; 16],                   // printable ASCII 0x20–0x7E, right zero-padded
    total_supply: Amount,             // 1 ..= MAX_SUPPLY, immutable
}

struct Pair {
    id: [u8; 32],
    token0: [u8; 32],
    token1: [u8; 32],
    fee_bps: u16,                     // ∈ FEE_TIERS
    reserve0: Amount,
    reserve1: Amount,
    lp_supply: Amount,
}

struct SignedTx {
    payload: TxPayload,
    signer_pubkey: [u8; 32],          // ed25519 verifying key; signer = blake3(signer_pubkey)
    signature: [u8; 64],              // ed25519 over blake3(SIGN_DOMAIN || borsh(payload))
}

struct TxPayload {
    nonce: u64,
    target_round: u64,
    action: Action,
}

enum Action {
    Transfer      { token: [u8;32], to: AccountId, amount: Amount },
    CreateToken   { name: [u8;16], supply: Amount },
    CreatePair    { token_a: [u8;32], token_b: [u8;32], fee_bps: u16 },
    AddLiquidity  { pair: [u8;32], amount0_desired: Amount, amount1_desired: Amount,
                    amount0_min: Amount, amount1_min: Amount },
    RemoveLiquidity { pair: [u8;32], lp_amount: Amount,
                    amount0_min: Amount, amount1_min: Amount },
    SwapExactIn   { path: Vec<[u8;32]>, token_in: [u8;32],
                    amount_in: Amount, min_amount_out: Amount },
    SwapExactOut  { path: Vec<[u8;32]>, token_in: [u8;32],
                    amount_out: Amount, max_amount_in: Amount },
    Publish       { topic: [u8;32], data: Vec<u8> },   // §7.5 — no state effect
    HtlcLock      { to: AccountId, token: [u8;32], amount: Amount,
                    hashlock: [u8;32], expiry_round: u64 },        // §7.6 — sha256(preimage)
    HtlcClaim     { htlc_id: [u8;32], preimage: [u8;32] },         // §7.6
    HtlcRefund    { htlc_id: [u8;32] },                            // §7.6 — anyone may send it
    Stake         { amount: Amount },
    Unstake       { amount: Amount },
    ClaimRewards  {},
}

enum ExecStatus { Ok, Failed(FailReason) }     // aligned 1:1 with Block.txs

struct Header {
    height: u64,
    prev_hash: [u8; 32],              // block_hash of the previous block; genesis: [0;32]
    drand_round: u64,
    drand_sig_hash: [u8; 32],         // blake3(drand_signature)
    collection_root: [u8; 32],        // blake3(concat(blake3(blob))), lexicographic — §5.1
    txs_root: [u8; 32],               // blake3(concat(tx_id) in EXECUTION ORDER)
    rejected_root: [u8; 32],          // blake3(concat(tx_id || borsh(RejectReason))),
                                      // pairs sorted by tx_id
    results_root: [u8; 32],           // blake3(borsh(Vec<ExecStatus>))
    state_root: [u8; 32],             // §5.4
}

struct Block {
    header: Header,
    drand_signature: Vec<u8>,         // full BLS signature (for verification and tlock replay)
    blob_manifest: Vec<[u8;32]>,      // blake3 of EVERY blob received for R, lexicographic
    unusable: Vec<[u8;32]>,           // ⊆ manifest: failed tlock OR age/AEAD OR Borsh decode
                                      // (a Borsh failure is decryptable yet unusable — hence
                                      // the name)
    txs: Vec<SignedTx>,               // execution order
    results: Vec<ExecStatus>,
    rejected: Vec<([u8;32], RejectReason)>,   // tx_id, lexicographically sorted
    node_signature: [u8; 64],         // ed25519 over block_hash
}
```

**Roots over empty lists (frozen)**: `txs_root`, `rejected_root` and `collection_root` over an
empty list are `blake3` of the empty input; `results_root` over an empty list is
`blake3(borsh(Vec::<ExecStatus>::new()))`. No special cases.

```
block_hash     := blake3(borsh(Header))
node_signature := Ed25519(node_key, block_hash)
```

The signed and committed object is the **Header**. `Block` is a container whose fields
(`txs`, `results`, `rejected`, `drand_signature`) are bound by the roots in the Header.

**Path semantics**: a sequence of `PairId`s. The node starts from `token_in`; for each pair it
derives the output token (the other side). Validation fails if a pair does not contain the
current token. `1 <= path.len() <= MAX_PATH_LEN`.

**Storage (redb, single file)**: tables `blocks` (`u64 → borsh(Block)`, append-only — **the
source of truth**), `state` (rebuildable cache for replay), `meta` (head, drand chain-info,
node pubkey, foundation pubkey). One `WriteTransaction` per batch: block and state delta are
atomic.

**Genesis (block 0)**: no transactions, **no allocation** (fair launch). Initial state is
empty; `state_root` is computed over the empty state; parameters and drand chain-info are
stamped into `meta`. The first native units are born with the emission closing block 1 (the
foundation share).

---

## 5. Batch lifecycle

One batch per drand round (3 s). Batch `R` executes the transactions encrypted toward round
`R`.

### 5.1 Phases

1. **Collection** (until `T(R) − ε`): `POST /tx` with `{blob, target_round: R}`; the node
   queues blindly and signs the receipt (§9.2).
2. **Collection commitment**: the node freezes the `blob_manifest` (blake3 of every blob
   received, lexicographic order) and computes `collection_root`.
   **Consensus does not temporally bind this closure to the beacon**: a node may wait for the
   beacon, decrypt, and only then manifest — the block stays formally valid. The guarantees
   are: (a) every **receipted** blob must appear in the manifest (otherwise it is a signed
   contradiction, §9.2); (b) every manifest entry must resolve in the complete accounting of
   phase 4.
   *Recommended operational practice (not consensus)*: publish the root on `WS /stream` before
   `T(R)`, giving third-party observers a timestamp of the commit-then-decrypt.
3. **Beacon**: `get(R)` with multi-endpoint fallback; BLS verification.
4. **Decryption and complete accounting** (below).
5. **Static validation** (§5.2) → `valid` + `rejected`.
6. **Ordering** (§5.3): shuffle + nonce normalization.
7. **Fee collection (frozen)**: for every valid transaction, burn `tx_fee(tx)` from the
   signer, in a **single phase** — static solvency (step 9, pre-batch balance) guarantees
   capacity, so the deduction cannot fail. Execution starts with fees already burned.
8. **Sequential execution** (§5.2).
9. **Close**: HTLC auto-settlement (§7.6), emission (§7.2), state root (§5.4), header,
   signature, atomic redb commit, WS push, external mirror.

Transactions arriving late for `R` are discarded (the client re-encrypts toward a future
round).

**Complete accounting (frozen).** Every manifest hash MUST resolve into exactly one of: a
transaction in `txs`, an entry in `rejected`, or the `unusable` list. Every claim of
undecryptability is **falsifiable by anyone**: blob + public beacon → reproducible
decryption. The node serves manifested blobs via `GET /blob/{hash}` and includes them in the
mirror.

**Normative derivation of `unusable`**:

```
unusable := manifest ∖ { blobs of the SignedTx in txs ∪ rejected }
```

The block's `unusable` field is **derived evidence, not consensus input**: two verifiers
holding the manifest, the blobs (from the mirror) and the beacon MUST derive the same set by
applying the POPCORN-TLOCK-AGE-V1 profile and Borsh decoding. An `unusable` field
inconsistent with that derivation is an incorrect block.

A Borsh decode failure of the decrypted `SignedTx` counts as `unusable` (the blob hash),
never as `rejected` — a `rejected` entry commits to a `tx_id`, which for an undecodable blob
does not exist.

`collection_root` commits to the **set of distinct blobs received, NOT the multiplicity of
submissions**: the same blob submitted ten times (ten receipts) is one manifest entry — a
multiset manifest is an incorrect implementation. Logical duplicates are handled by nonce
deduplication anyway.

### 5.2 Validation and failure (frozen)

**Static validation — ordered pipeline.** Steps apply in this order; a transaction dropped at
one step does not take part in the following ones.

| # | Step | Reject reason |
|---|---|---|
| 1 | Borsh decode of the `SignedTx` | *(blob counts as `unusable`, no tx_id exists)* |
| 2 | ed25519 signature of `signer_pubkey` over `blake3(SIGN_DOMAIN \|\| borsh(payload))`; derive `signer = blake3(signer_pubkey)` | `BadSignature` |
| 3 | `target_round == R` | `WrongRound` |
| 4 | account `signer` exists → if `account.pubkey == Some(pk)`, require `pk == signer_pubkey` → if `account.nonce == u64::MAX`, the account is **terminal** | `UnknownAccount`, then `PubkeyMismatch`, then `NonceExhausted` |
| 5 | Field ranges (below) | `FieldOutOfRange` |
| 6 | **Nonce dedup**: for equal `(signer, nonce)`, the lexicographically smallest `tx_id` survives | `DuplicateNonce` |
| 7 | **Contiguity**: the account's nonces must form a contiguous run from `account.nonce + 1`; transactions beyond the first gap are dropped | `NonceGap` |
| 8 | **Budget**: at most `MAX_TX_PER_ACCOUNT_PER_BATCH` transactions per account, kept in ascending nonce order | `OverBudget` |
| 9 | **Fee solvency**: **pre-batch** native balance ≥ `Σ tx_fee(tx)` over the account's transactions surviving steps 1–8; if insolvent, drop transactions **from the highest nonce downward** until the condition holds | `FeeInsolvent` |

Step 4's sub-order is pinned: existence → pubkey coherence → exhausted nonce. All else being
equal, `PubkeyMismatch` wins. The terminal-nonce check happens **here**, before dedup and
contiguity.

Step 6's formal tie-break: equal `tx_id`s ⇒ byte-identical `SignedTx` ⇒ the same transaction,
which collapses into a single entry.

Step 5 ranges: `fee_bps ∈ FEE_TIERS`; `supply ∈ 1..=MAX_SUPPLY`;
`1 <= path.len() <= MAX_PATH_LEN`; printable-ASCII name; `data.len() <= MAX_PUBLISH_SIZE`;
**`amount > 0` for `Transfer`, `Stake` and `Unstake`**; for `HtlcLock`, `amount > 0` and
`R < expiry_round <= R + HTLC_MAX_LIFETIME_ROUNDS`.

> An account funded in the same batch can only transact from the next batch onward.

**Transfer rule**: `Transfer` with `to == signer` ⇒ `Failed(SelfTransferNoop)` — no
self-transfers (they would pay a fee for an accounting-ambiguous no-op).

**Canonical fee (frozen)**:

```
tx_fee(tx) = FEE_TX + PUBLISH_BYTE_FEE × max(0, len(data) − PUBLISH_FREE_BYTES)   if Publish
tx_fee(tx) = FEE_TX                                                               otherwise
```

This is the **only** fee definition, used everywhere: solvency (step 9), burn on `Ok`, burn on
`Failed`.

**Outcomes.**

- **`rejected`** (failed static validation): not executed, **zero fee, nonce untouched**;
  recorded in the block as `(tx_id, RejectReason)`.
- **`Failed`** (failed at runtime — `min_amount_out` violated, insufficient balance, pool
  missing at execution time, overflow): rollback to the snapshot, **nonce consumed**
  (`account.nonce += 1`), `ExecStatus::Failed(reason)` in `results`.
- **`Ok`**: effects applied, nonce consumed.

**Fees and execution (frozen)**: the fees of ALL valid transactions are burned in phase 7,
before any action executes — every transaction always pays the full fee (`Ok` and `Failed`),
no action can spend funds earmarked for later fees, and the old `min()` clamp no longer
exists. Then, for each transaction in order: (1) snapshot the state; (2) execute the action.
A `Failed` rollback restores the snapshot; the fee, burned in phase 7, is outside the
rollback by construction and never refunded.

### 5.3 Nonce + shuffle (frozen)

The global shuffle (§3.7) assigns **positions**. Then, **per-account normalization**: for
each account with several transactions in the batch, let `P = {p1 < p2 < …}` be the positions
of its transactions after the shuffle; its transactions are reassigned to `P` in **ascending
nonce order**. Transactions of different accounts do not move.

Properties: deterministic; the distribution of positions stays uniform; a contiguous sequence
never fails because of internal disorder; an intermediate `Failed` does not block the
account's later transactions (the nonce is consumed regardless).

### 5.4 Canonical state root (frozen)

```
h = blake3::Hasher::new()
for table in [0x01 accounts, 0x02 tokens, 0x03 pairs, 0x04 htlcs, 0x05 global]:
    h.update([table_tag])
    for (k, v) in table, with k in lexicographic order of the bytes of borsh(k):
        bk = borsh(k); bv = borsh(v)
        h.update(LE32(len(bk))); h.update(bk)
        h.update(LE32(len(bv))); h.update(bv)
state_root = h.finalize()
```

`global` (field order frozen): `height: u64`, `total_staked: Amount`, `acc_per_stake: u128`,
`staking_reserved: Amount`, `native_emitted: Amount`, `native_burned: Amount`,
`account_count: u64`.

**Encoding of the `global` singleton (frozen)**: table `0x05` contains exactly one pair with
an empty key:

```
h.update([0x05]); h.update(LE32(0)); h.update(LE32(len(borsh(global)))); h.update(borsh(global))
```

No inference is required from a non-Rust implementer.

### 5.5 Five-bucket monetary invariant (exact equality by construction)

```
  Σ balances[NATIVE]
+ Σ staked
+ Σ htlcs[token == NATIVE].amount
+ Σ pairs[NATIVE side].reserve
+ staking_reserved
    = GENESIS_SUPPLY + native_emitted − native_burned
```

Every native unit lives in **exactly one** of five places: liquid balance, stake, HTLC escrow,
AMM reserve, or staking liability (`staking_reserved`). No unit "lives inside a formula".

> **The AMM bucket was missing, and this is the correction (v0.9.3-en).** Earlier text listed
> four buckets and omitted pool reserves. A pair may hold `NATIVE_TOKEN` on either side
> (§4.1), and those units left somebody's balance to get there — so the moment anyone provided
> native liquidity, the stated equality became false. Since §10 checks the invariant at *every
> block*, a perfectly honest chain would have failed its own verification as soon as a native
> pool was funded. The independent reference executor of §10 is what surfaced it: both
> implementations agreed with each other and with the old text, and both reported the
> invariant broken — the specification was wrong, not the code.

The older invariant stated with `Σ pending` was **mathematically false** (double flooring over
different bases: `Σ⌊xᵢ⌋ ≤ ⌊Σxᵢ⌋`) and has been replaced.
`pending(a) = ⌊staked × (acc_per_stake − paid_acc) / PRECISION⌋` is the derived formula
determining how much a settle *transfers* from `staking_reserved` to the balance;
`staking_reserved ≥ Σ pending(a) ≥ 0` holds **by construction** (proof in §8), and the
difference is the rounding residue: a protocol liability, not attributable without O(N) work,
never burned and never credited to anyone.

Pool reserves are a liability of the pair to its LP holders in exactly the same sense, and are
accounted the same way: present in the equality, owned by nobody's balance.

`native_emitted` means exactly: nominal units created by the protocol. `staking_reserved`:
units created as the staker share and not yet moved into a balance.

---

## 6. AMM math — POPCORN-V2-MATH

Uniswap V2 generalized to fee tiers. Intermediate arithmetic in `U256`, results back in `u128`
with checks. Division is floor. No floats. `fee_num = 10_000 - fee_bps`.

**Exact-in (per hop):**

```
amount_in_with_fee = amount_in * fee_num
amount_out = (amount_in_with_fee * reserve_out)
           / (reserve_in * 10_000 + amount_in_with_fee)
```

**Exact-out (per hop):**

```
require(amount_out < reserve_out)   // violation ⇒ Failed(SlippageExceeded)
amount_in = (reserve_in * amount_out * 10_000)
          / ((reserve_out - amount_out) * fee_num) + 1
```

**Multi-hop.** Exact-in runs forward hop by hop, then `require(final_out >= min_amount_out)`.

Exact-out runs backward first: with `path = P1..Pk` and desired final output `X`,
`in_k = exact_out(Pk, X)`, `in_{k−1} = exact_out(P_{k−1}, in_k)`, …,
`in_1 = exact_out(P1, in_2)`; then `require(in_1 <= max_amount_in)`; then execution goes
**forward** with exactly the amounts from the backward pass (`P1: in_1 → in_2`, …,
`Pk: in_k → X`), never recomputed. Each hop updates its own pair's reserves and the hop's fee
stays with that pool's LPs. A multi-hop transaction is atomic (§5.2).

**Liquidity lifecycle (frozen):**

```
genesis (reserve0 == 0 && reserve1 == 0 && lp_supply == 0):
            liquidity = integer_sqrt(amount0 * amount1) - MINIMUM_LIQUIDITY
            require(liquidity > 0)
            // MINIMUM_LIQUIDITY is credited to lp_supply but to no account (burned)

re-genesis (reserve0 == 0 && reserve1 == 0 && lp_supply == MINIMUM_LIQUIDITY):
            // fully drained pair: restarts with the genesis formula
            liquidity = integer_sqrt(amount0 * amount1) - MINIMUM_LIQUIDITY
            require(liquidity > 0)
            // the already-burned MINIMUM_LIQUIDITY stays the only burned amount
            // both reserves 0 with lp_supply > MINIMUM_LIQUIDITY → Failed(ReGenesisGuard)
            // (guard: residual LP over null reserves would steal share from new
            //  depositors; they must first be burned via a zero-yield RemoveLiquidity,
            //  after which the pair restarts)

subsequent: liquidity = min(amount0 * lp_supply / reserve0,
                            amount1 * lp_supply / reserve1)
            require(liquidity > 0)

removal:    amount_i = lp_amount * reserve_i / lp_supply
            require(amount_i >= amount_i_min)
```

**Zero output forbidden (frozen)**: every swap hop requires `amount_out >= 1`; otherwise the
transaction is `Failed(ZeroOutput)` — no zero-yield swaps that only pay a fee to move dust.

**Explicit AMM validation rules** (runtime; `Failed` if violated):

- **`CreatePair`**: `token_a != token_b`; both exist (`NATIVE` or a `Token` record); neither is
  an `LpTokenId`; `fee_bps ∈ FEE_TIERS` (already static); the `PairId` does not exist yet.
- **`AddLiquidity`**: the pair exists; `amount0_desired > 0` and `amount1_desired > 0`. For
  pools with non-zero reserves the EFFECTIVE amounts follow Router02:

  ```
  a1_opt = ⌊amount0_desired × reserve1 / reserve0⌋
  if a1_opt ≤ amount1_desired:  (actual0, actual1) = (amount0_desired, a1_opt)
  else:                         a0_opt = ⌊amount1_desired × reserve0 / reserve1⌋
                                (actual0, actual1) = (a0_opt, amount1_desired)
  require(actual0 ≥ amount0_min && actual1 ≥ amount1_min)
  ```

  Only the **actual** amounts are debited (the excess `desired − actual` is NEVER touched);
  `reserve += actual`; minting uses the liquidity formula with the actual amounts. In the
  genesis and re-genesis branches the actuals equal the desired amounts. Debiting the desired
  amounts is an incorrect implementation.
- **`RemoveLiquidity`**: the pair exists; `lp_amount > 0`; `lp_amount ≤` the signer's LP
  balance.
- **`Swap*`**: non-empty path of length `≤ MAX_PATH_LEN` (already static); `token_in` belongs
  to the first pair; every hop exists and contains the current token; `amount_in > 0` /
  `amount_out > 0`.

The `k_after ≥ k_before` invariant holds **only for successful swap hops** — it does NOT apply
to `AddLiquidity`/`RemoveLiquidity`, which change `k` by definition. Pools with an LP token as
a side are forbidden (§4.1). `u128 × u128 < U256::MAX` always: no U256 overflow.

---

## 7. Economics

### 7.1 Fair launch and the foundation

- **No genesis allocation** (`GENESIS_SUPPLY = 0`): no premine, no primary sale, no faucet, no
  invites. Every native unit that will ever exist is born from emission (§7.2).
- `FOUNDATION` is an ordinary account (the operator's key, public movements) receiving the
  foundation share of emission. It is the economy's **bootstrap**: the first tokens in
  circulation are its share from block 1, distributed through grants, payments or pool
  liquidity so that others can transact and stake.
- **The foundation may stake** its funds and earn the staker share like anyone else: a
  deliberate choice, consistent with "ordinary account" — no special rule in the code, and the
  resulting concentration is public and readable on-chain by anyone.
- The secondary market for the native token (users trading it against external assets) is
  off-chain and outside the protocol.

### 7.2 Per-batch emission with halving (the only source of new supply)

```
emission_index    = height − 1                     // 0-based: block 1 → index 0
EMISSION(height)  = EMISSION_0 >> (emission_index / HALVING_INTERVAL)
// so each epoch contains EXACTLY HALVING_INTERVAL batches
// (blocks 1..=10_512_000 → epoch 0; from 10_512_001 → epoch 1)

staker_share     = EMISSION(height) * EMISSION_STAKER_BPS / 10_000
foundation_share = EMISSION(height) - staker_share

if total_staked == 0:
    // the staker share is NOT emitted: it is never born (not to the foundation,
    // not burned)
    native_emitted += foundation_share
else:
    native_emitted += EMISSION(height)
```

- `staker_share` enters the staking accumulator and reserve **whole** (§8 — no dust: flooring
  lives only on the user side); `foundation_share` is credited to `FOUNDATION`. Per-batch
  identity: effective emission = `staker_share + foundation_share` (with `total_staked == 0`:
  `foundation_share` alone).
- **Cap semantics (unambiguous)**: 15% is the EXACT cap on the **nominal share** of each
  emission — with no dust, the foundation receives exactly `EMISSION − staker_share`, never
  one unit more. In batches with `total_staked == 0` the staker share (85%) **is not born**:
  the foundation still receives only its nominal 15% — which happens to be **100% of that
  batch's effective emission**. "Capped at 15%" is true with respect to nominal emission, not
  with respect to the effective emission of staker-less batches: that is exactly the fair
  launch bootstrap, declared.
- **Timing (frozen)**: emission is applied exclusively at the **close** of batch `h`, after
  every transaction has executed, and is **not spendable by transactions of the same batch**.
- **`native_emitted` (frozen)**: counts only emission that has already entered economic state,
  in the batch where it happens — including shares credited to the staking reserve
  (`staking_reserved`) and not yet claimed. It is not "rewards already paid".
- The shift is integer over `u128`: `EMISSION = 0` once
  `emission_index / HALVING_INTERVAL ≥ 128` (or earlier, when the shift exhausts the bits of
  `EMISSION_0`). Total effective emission is the discrete sum batch by batch, recomputed by
  the verifier during replay.
- **Exact effective cap**:
  `GENESIS_SUPPLY + HALVING_INTERVAL × Σᵢ₌₀..₁₂₇ (EMISSION_0 >> i)`.
  Convenient upper bound: `GENESIS_SUPPLY + 2 × EMISSION_0 × HALVING_INTERVAL`. With the
  proposed parameters (genesis 0): < 21.03 M — reduced further by staker-less batches, whose
  staker share is never born.

### 7.3 CreateToken

- `name`: 16 bytes of printable ASCII, zero-padded; **no uniqueness** (identity is the id;
  name impersonation is part of the game).
- `supply ∈ 1..=MAX_SUPPLY`; all of it goes to the creator at execution.

### 7.4 Fees: total burn

Inspired by the deflationary mechanism of EIP-1559 — without base/priority fees: the fee is
flat.

- `FEE_TX` is flat per executed transaction (`Ok` and `Failed`) and **burned**:
  `native_burned += fee`. `rejected` transactions pay nothing. A multi-hop pays a single
  `FEE_TX`.
- Swap fees (`fee_bps`) remain a separate flow, entirely to the hop's LPs (never burned).
- Monetary dynamics: decreasing emission against a burn proportional to usage — circulating
  supply can turn deflationary at steady state.

### 7.5 Publish — the data board (user-carried oracles)

- `Publish { topic, data }` does not touch state: the data lives **only in the block**. The
  ledger acts as an ordered, timestamped board (the drand round is a cryptographic timestamp);
  state does not grow by a single byte.
- The publish fee is the canonical `tx_fee` (§5.2), entirely burned — identical for solvency,
  `Ok` and `Failed`.
- Native authentication: the publisher's ed25519 signature is the feed's identity, and its
  history is entirely on-chain.
- **Canonical feed order**: for publishes on the same `topic` within the same block, the order
  is the **execution position** in `txs` (the same order used by the auto-settlement scan of
  §7.6).
- **No on-chain logic consumes this data** (there is no VM): consumers are off-chain bots and
  services — coordination, price feeds, settlement by convention. Typical use: signed updates
  from an external provider (e.g. a Pyth-style feed) republished by anyone, with provider
  signature verification left to the consumer.

### 7.6 HTLC — user-carried atomic cross-chain swaps (frozen)

```rust
struct Htlc {
    id: [u8;32],            // blake3(0x04 || sender || LE64(payload.nonce))
    sender: AccountId,
    recipient: AccountId,   // fixed at lock time, immutable
    token: [u8;32],         // NATIVE | TokenId | LpTokenId
    amount: Amount,
    hashlock: [u8;32],      // sha256(preimage) — NOT blake3, see below
    expiry_round: u64,
}
```

**Semantics (frozen).**

- `HtlcLock` debits `amount` from the sender; the funds live in the state's `htlcs` table —
  **no account owns them, not even the node key can touch them**.
- `HtlcClaim` is valid iff `sha256(preimage) == hashlock` **and** `R <= expiry_round`; it
  credits `amount` to the `recipient`. Anyone may send it — what counts is the preimage, not
  the sender.
- `HtlcRefund` is valid iff `R > expiry_round`; it credits `amount` to the `sender` and is
  **invocable by anyone** (state garbage collection without depending on the sender).

The claim/refund boundary is sharp: `<=` against `>`, no overlap. A resolved HTLC is removed
from the table.

**Auto-settlement via Publish (frozen).** **After every transaction has executed** and
**before emission**, the node scans, in execution order, the `Publish` transactions that
executed `Ok` in the batch: for each `data` of **exactly 32 bytes**, if `sha256(data)` matches
the hashlock of an open HTLC with `expiry_round >= R`, that HTLC settles to its `recipient`
exactly like a claim (removal + credit).

Consequence of the pinned sequence: if an `HtlcClaim` settles the HTLC during execution in the
same batch, a `Publish` carrying the same preimage finds an empty index and is a no-op — the
claim wins, deterministically.

Lookup is O(1) over a `hashlock → htlc_id` **index**: a **derivable cache** built from the
`htlcs` table, **excluded from the state root** (§5.4) and deterministically rebuilt at
replay — it cannot diverge without the committed table diverging. It is updated on
lock/claim/refund/settle. Duplicate hashlocks: a later lock on an already-indexed hashlock is
`Failed(HtlcDuplicateHashlock)` — one hashlock, one HTLC.

Property: the preimage is **carrier-independent** — the recipient, a watchtower or any third
party can deliver it, even redundantly; the recipient may be offline; every settlement is
replay-verifiable (the preimage is in the block). `HtlcClaim` remains an equivalent direct
route.

**Mechanism, not policy**: the protocol knows neither the other chain nor the agreement
between the parties — it provides only the deterministic conditional lock. Swaps, bridges,
escrow and watchtowers are **user** protocols built on top.

**Why SHA-256**: this is the only point in the protocol that does not use blake3 —
deliberately. An atomic swap requires the **same hash on both sides**, and SHA-256 is the
standard of Bitcoin script, Lightning, EVM and Solana. The preimage is fixed at **exactly 32
bytes** (the Lightning standard), which closes the known preimage-length attacks on Bitcoin
script.

**Security — from the literature, declared.**

- **Claim censorship (MAD-HTLC class)**: on PoW/PoS chains the attack is bribing miners to
  ignore the claim until timeout; here the "miner" is the single operator. With
  auto-settlement the target is no longer "Bob's transaction": the preimage can arrive from
  anyone, inside any encrypted blob — to censor it the node must discard **blindly and en
  masse** blobs it only sees after decryption, leaving §9.2 receipts that public replay
  exposes.
  **Declared edge case**: inclusion remains the only gate (§1) — an operator willing to
  incriminate itself can discard everything until expiry. Long windows turn this into
  prolonged public sabotage rather than a quiet heist. The step beyond is not a cleverer
  mechanism: it is a second block producer, i.e. a different architecture.
- **Timeout staggering (normative for users)**: in a two-HTLC swap, the side claimed first
  must expire **well before** the other side's refund (T2 < T1), with margin for both chains'
  delays. Timeouts that are too short are the classic mistake.
- **Free option / sore loser**: an HTLC gives whoever knows the preimage a free option until
  expiry. This is structural, not a bug in our protocol; practical mitigation: split large
  swaps into small tranches.
- **Maximum lifetime**: `expiry_round <= R + HTLC_MAX_LIFETIME_ROUNDS` — state does not
  accumulate eternal locks.
- **Duplicate-hashlock griefing (threat model)**: "one hashlock, one HTLC" implies that
  whoever locks a known hashlock first blocks the others (at the cost of their own fee and
  locked capital). Standard user defence: a fresh hashlock per swap, never pre-announced in
  the clear — the timelock covers the lock until inclusion anyway.

The protocol stays bridge-free: HTLCs are the primitive **users** build their own swaps with,
against any chain that has hashlocks, with no custodian and without POPCORN ever touching
external assets.

---

## 8. Staking — O(1) distribution

A literal transcription of the audited Synthetix `StakingRewards` math.

```
PRECISION = 10^18

per batch (at close):
    acc_per_stake    += (staker_share * PRECISION) / total_staked   // floor
    staking_reserved += staker_share                                // WHOLE: the reserve holds
                                                                    // the entire reward pot,
                                                                    // like the Synthetix
                                                                    // contract. No dust.
    // if total_staked == 0 → staker_share is not emitted (§7.2); nothing is credited

pending(a) = (a.staked * (acc_per_stake - a.paid_acc)) / PRECISION  // ONE floor only,
             // over the difference (Synthetix "earned"): always ≤ the true entitlement

Stake / Unstake / ClaimRewards (frozen order):
    1. p = pending(a); PAY p AS A TRANSFER: staking_reserved -= p; balance += p
       // the total does not change: B + R = (B+p) + (R−p) — part of the property test
    2. update a.staked (after settlement, never before)
    3. a.paid_acc = acc_per_stake                                   // a snapshot, not an amount
```

**Solvency proof.** Every batch adds `staker_share` both to `staking_reserved` and to the true
entitlement pot `Σ sₐ·Δacc/P`; every settle pays `⌊s·(acc−paid)/P⌋ ≤` the entitlement accrued
by that account over the interval. Therefore

```
Σ paid + Σ pending ≤ Σ staker_share = cumulative staking_reserved
⇒ staking_reserved ≥ Σ pending ≥ 0, ALWAYS, by construction.
```

**Liquidity guard (frozen)**: `Stake` requires at runtime, after the fee has been burned,
`native_balance ≥ amount + FEE_TX` — at least one `FEE_TX` must stay liquid. Without this
guard, an account staking 100% would be **permanently stuck**: no transaction (not even
`Unstake`) would pass fee solvency, with value locked inside forever. Equivalent pre-fee
form: `balance ≥ tx_fee + amount + FEE_TX`. Violation ⇒ `Failed(StakeLiquidityGuard)`.

**Arithmetic (frozen)**: every `staked × acc_per_stake` and `staker_share × PRECISION`
multiplication happens in **U256 intermediates** (as in §6), with the result converted back to
`u128`. The bounds are unconditional thanks to finite supply: results (`pending`; `paid_acc`
is a value of `acc_per_stake` and inherits its ceiling) fit in `u128` (≤ ~1.9×10³² in the
pathological case), and **`acc_per_stake` itself is capped by halving** — even with
`total_staked = 1` in every batch forever, its maximum is the entire historical staker
emission × PRECISION ≈ 0.85 × 21.02M × 10⁹ × 10¹⁸ ≈ 1.8×10³⁴, four orders below `u128::MAX`.
No field can overflow, with no assumption about the chain's lifetime. U256 intermediates
remain mandatory: it is the *product* `staked × acc` (up to ~10⁵⁰) that does not fit in
`u128`, not its results.

**Zero-balance Unstake note**: an account doing a full `Unstake` receives stake + pending as
liquid balance, so it does not get stuck; anyone who still ends below `FEE_TX` stays inactive
until they receive funds — an external way out always exists, declared acceptable.

**The single residue**: with a whole reserve, accumulator dust no longer exists. What remains
is the **rounding residue** `staking_reserved − Σ pending ≥ 0`: the fractions that user-side
flooring leaves in the reserve. It is a protocol liability — not attributable without O(N)
work, hence neither burned nor gifted: it stays there, declared, and with the current design
it has the right sign.

**Mandatory pre-genesis gate**: a consensus-grade property test — millions of random
emission/stake/unstake/claim sequences with arbitrary distributions and extreme `PRECISION`
values, checking **after every single operation**:

1. the monetary invariant (exact equality; the staking gate exercises it without pools, and
   the differential scenarios of §10 exercise it with them);
2. total conservation at every settle;
3. **`staking_reserved ≥ Σ pending ≥ 0`** — assertion (3) is the one an earlier gate lacked,
   and the one that would have caught the earlier underflow bug.

Only `NATIVE_TOKEN` is stakeable. There is no unbonding. Stake is neither spendable nor
transferable while staked. Staking is the only way to take part in emission: it is the chain's
"mining".

---

## 9. Node API

### 9.1 Endpoints

| Method | Path | Function |
|---|---|---|
| `POST` | `/tx` | Submit `{blob: base64, target_round: u64}` → signed receipt |
| `GET` | `/head` | Latest block header |
| `GET` | `/block/{height}` | Full block (Borsh base64 + JSON) |
| `GET` | `/account/{id}` | Account state |
| `GET` | `/pair/{id}` | Reserves, fee_bps, lp_supply |
| `GET` | `/tokens`, `/pairs` | Listings (pairs grouped by token couple, all tiers) |
| `GET` | `/supply` | GENESIS, emitted, burned, circulating, staked |
| `GET` | `/topic/{topic}?from={h}` | Publishes on a topic (a convenience index over blocks, not state) |
| `GET` | `/blob/{hash}` | Manifested encrypted blob (accounting audit §5.1; also on the mirror) |
| `GET` | `/chain/export?from={h}` | Block stream for replay |
| `GET` | `/params` | Parameters + drand chain-info + node pubkey + foundation pubkey |
| `WS` | `/stream` | Block push |

### 9.2 Signed submission receipt + collection commitment

The response to `POST /tx` is the **canonical receipt defined below**:
`Ed25519(node_key, blake3(borsh(ReceiptPayload)))` — the ONLY normative preimage; any informal
concatenation is an incorrect implementation.

```rust
struct ReceiptPayload {
    domain: "popcorn-receipt-v1",
    blob_hash: [u8; 32],
    target_round: u64,
    timestamp_ms: u64,   // node-declared, NON-CONSENSUS
}

receipt_hash = blake3(borsh(ReceiptPayload))
```

`timestamp_ms` is declared by the node and is **non-consensus**: never used for ordering,
validity, or as cryptographic proof of time (§13).

With the collection commitment, the chain of responsibility becomes two-signature:

- **receipt issued, hash absent from `blob_manifest`** → two signatures by the same node
  contradicting each other: **censorship proven by the block itself**, no replay needed;
- **hash in the manifest** → it must resolve into `txs` / `rejected` / `unusable` (§5.1
  accounting); a false `unusable` claim is refutable by anyone holding blob + beacon, and
  refusing to serve a manifested blob (`GET /blob/{hash}`) is visible obstruction.

**Participation protocol (client policy, not consensus)**: if the receipt for a blob toward
`R` arrives **before the deadline the client set itself** (≤ `T(R) − ε`), the blob is to be
considered protected for `R`; otherwise the client MUST NOT rely on inclusion in `R` and may
re-encrypt toward a later round. Consensus knows only blobs, rounds, receipts and manifests:
reacting to a missing receipt is entirely the client's business.

**The limit, stated plainly**: receipt before the deadline → strong proof of omission if the
blob never appears; no receipt → the client can *claim* it sent something, but holds no
cryptographic proof.

Declared residue: **blind refusal at ingress** (the node issues no receipt) — blind by
construction of the timelock, since the node does not know what it is refusing.

DoS note (outside consensus): junk blobs pay no fee; the admission ceiling is
`MAX_TX_PER_BATCH` plus wire-level per-IP rate limiting — an accepted, declared surface.

---

## 10. Third-party verification

An independent verifier (the same binary, `--verify`):

1. Downloads `/chain/export` (or the mirror).
2. Per block, checks: `node_signature` over `block_hash`; `prev_hash`; the round's drand
   signature against the public quicknet chain-info; `drand_sig_hash` coherence; a
   **recomputation of shuffle + normalization** (§3.7, §5.3); recomputation of
   `txs_root` / `rejected_root` / `results_root`; and **collection commitment coherence**:
   `collection_root` = blake3 over the sorted `blob_manifest`, `unusable ⊆ manifest`, and —
   with blobs from the mirror — the complete manifest → txs/rejected/unusable accounting,
   re-decrypting with the beacon.
3. Replays from genesis: compares every `state_root` (§5.4), **verifies the emission formula**
   and the **monetary invariant** at every block.

Any divergence is cryptographic proof of incorrectness.

**The specification defines normative semantics; implementations of external primitives
(Ed25519, Borsh, age/tlock, U256) are accepted only in the version and profile frozen in
§13** and must pass the consensus-grade test vectors — that, not algorithmic purity, is what
makes verification reproducible in any language.

**Consensus-grade test vectors (mandatory)**: the repository maintains byte-for-byte
end-to-end fixtures — signed tx → age/tlock blob → real beacon → ordered batch → resulting
state → `state_root` → receipt — reproducible by an independent implementer. Alongside the
verifier, a **minimal independent reference executor** (AMM, fees, rewards, shuffle, state
root serialization) acts as a differential: two implementations of the same spec that diverge
mean a spec bug or a code bug, found before genesis.

**Two distinct properties.** **State replay** is self-contained (SignedTx + roots + genesis →
state_root; `/chain/export` suffices). The **collection audit** (manifest → tx/rejected/
unusable) is NOT: it needs the blobs (from the node or the mirror) and the beacon. These are
different guarantees and must be cited separately.

**Replay perimeter (frozen)**: replay starts from the **cleartext** `SignedTx` contained in
blocks — encrypted blobs do not live in the block. User signatures make transactions
unforgeable by the node (it can omit, it cannot invent). The property "the node did not see
the transactions before the round" is guaranteed by client-side encryption and tlock pinning,
and is **not re-verifiable under replay**: it is the only property of the system resting on
client behaviour rather than on blocks.

**Blob mirroring is an operational requirement of the operator** (outside consensus): without
it the collection audit (§9.2, the derivation of `unusable`) is not practicable by third
parties — an operator who does not publish manifested blobs is obstructing the audit,
visibly.

---

## 11. Protocol parameters

| Parameter | Proposed value | Notes |
|---|---|---|
| `BATCH_PERIOD` | 1 quicknet round (3 s) | aligned with the beacon |
| `MAX_TX_PER_ACCOUNT_PER_BATCH` | 8 | anti-spam budget |
| `MAX_TX_PER_BATCH` | 10 000 | resource ceiling |
| `MAX_PATH_LEN` | 4 | maximum hops |
| `FEE_TIERS` | {5, 30, 100} bps | the only admitted tiers |
| `MAX_SUPPLY` | 10³⁰ | supply ceiling per user token |
| `GENESIS_SUPPLY` | 0 | **fair launch**: no genesis allocation |
| `EMISSION_0` | 1 × 10⁹ | 1 native/batch initially (≈ 28,800/day) |
| `HALVING_INTERVAL` | 10 512 000 batches | ≈ 1 year at 3 s/batch |
| `EMISSION_STAKER_BPS` | 8 500 | 85% stakers / 15% foundation |
| → maximum supply | ≈ 21.02 M | upper bound; the effective figure is lower (staker-less batches: that staker share is never born) |
| `FEE_TX` | 5 000 | 0.000005 native (like Solana's 5,000 lamports), flat, burned |
| `MAX_PUBLISH_SIZE` | 512 B | maximum `Publish` payload |
| `PUBLISH_FREE_BYTES` | 128 B | threshold included in the flat fee |
| `PUBLISH_BYTE_FEE` | 50 | units per byte above the threshold (512 B ≈ 0.0000242 native) |
| `SIGN_DOMAIN` | `"popcorn-v1"` | signing domain, 10 ASCII bytes |
| `MINIMUM_LIQUIDITY` | 1 000 | burned at first mint |
| `PRECISION` | 10¹⁸ | staking accumulator |
| `MAX_BLOB_SIZE` | 2 KiB | encrypted payload |
| `HTLC_MAX_LIFETIME_ROUNDS` | 864 000 | ≈ 30 days: maximum lock lifetime (§7.6) |
| `BLOB_ROUND_HORIZON` | 200 | ≈ 10 min: the node accepts blobs only for nearby rounds (§3.5) |
| `CONSENSUS_VERSION` | `0x0000_0009_0002` | normative consensus version (§13) |
| `GENESIS_DRAND_ROUND` | fixed at genesis | normative mapping `round(h) = G + h − 1` (§3.4) |
| `DRAND_REMOTES` | api.drand.sh, drand.cloudflare.com | same chain hash, fallback in order |

**Operational requirements (outside consensus, frozen as an obligation)** — the collection
phase is blind (no fee before decryption: the signer is unknown), so node availability must be
defended at the wire level without touching the protocol:

- `MAX_TOTAL_INGRESS_PER_ROUND` (byte ceiling accepted per round, ≥ `MAX_TX_PER_BATCH` ×
  `MAX_BLOB_SIZE`)
- `MAX_BLOBS_PER_CONNECTION`
- `MAX_BYTES_PER_IP_WINDOW`
- `MAX_TLOCK_DECRYPT_WORK_PER_ROUND` (CPU budget with declared degradation: the chain waits,
  it never skips manifested blobs)

**Mandatory pre-genesis worst-case benchmark**: 10k valid ciphertexts, 10k invalid tlock
ciphertexts, 10k garbage blobs — the CPU worst case may not be the obvious one. This is a
*node availability* risk, not a ledger correctness risk: declared and kept separate.

Monetary values (GENESIS, EMISSION_0, HALVING, split) are proposals, tunable before genesis;
after genesis they are **immutable**.

---

## 12. Declared out of scope

- No VM, no user code (door left open: `Action` is extensible).
- No concentrated liquidity, hooks, native limit orders, flash loans, or on-chain TWAP.
- No consensus, no P2P, no hard censorship resistance.
- No oracle beyond drand. **No protocol bridge**: POPCORN neither custodies nor verifies
  external assets. HTLCs (§7.6) are the primitive users build their own atomic cross-chain
  swaps with; any custodial bridge is third-party activity, off-chain and outside the
  protocol, with the corresponding responsibilities on whoever runs it.

---

## 13. POPCORN-CONSENSUS — normative definition

POPCORN's determinism is **normative, not emergent**: the chain is not deterministic "because
Rust + Borsh + blake3 are", but because this document explicitly defines every semantics that
can influence the state root. `Cargo.lock` is not a consensus specification; this table is.

`CONSENSUS_VERSION = 0x0000_0009_0002`, stamped into genesis and `/params`. The form is three
16-bit fields, `0x{reserved}_{minor}_{patch}`: here minor = 9, patch = 2. The patch level
tracks spec revisions; the freeze fixes the definitive value.

| Component | Normative definition |
|---|---|
| Hash | BLAKE3-256 (plus XOF for the shuffle); SHA-256 ONLY for HTLC hashlocks (§7.6) |
| Serialization | Borsh, EXACT version in CONSENSUS-LOCK (crate + derive + features; collection decoding with strictly increasing order mandatory — the `de_strict_order` feature or an equivalent check); canonical structures only (§2.3) |
| Signature | Ed25519, `ed25519-dalek` pinned version, `verify_strict` semantics (§3.1) |
| Integers | u128 with U256 intermediates (`primitive-types` pinned); overflow ⇒ deterministic `Failed`, never a panic |
| Ordering | sort by tx_id → Fisher-Yates over BLAKE3-XOF(sig ‖ LE64(height)) with the defined rejection sampling (§3.7) → nonce normalization (§5.3) |
| AMM | POPCORN-V2-MATH (§6): formulas, rounding and lifecycle exactly as written, floor everywhere, rounding in favour of the pool |
| Fees | canonical `tx_fee` (§5.2), single phase, total burn |
| Emission / rewards | §7.2 (0-based index, no-staker rule) + §8 (accumulator, reserve) |
| Timelock | tlock over drand quicknet, `DRAND_SCHEME` and `DRAND_CHAIN_HASH` pinned, age blob format, beacon policy (§3.3) — behind `TimelockProvider`, substitutable |
| State commitment | sequential BLAKE3 hash over tagged tables (§5.4) |

**CONSENSUS-LOCK (genesis annex).** At freeze time, the EXACT version (crate, derive, feature
flags, commit for vendored code) of each of `borsh` + `borsh-derive`, `ed25519-dalek`,
`primitive-types`, `blake3`, `sha2`, `age`, `tlock`, `tlock_age`, `drand_core` is recorded.
"Pinned version" without a number contradicts this very section: the numbers live in the
annex, stamped into genesis next to `CONSENSUS_VERSION`.

### 13.1 Normative discriminants

Borsh discriminant values for consensus enums are tabulated. Adding, removing or reordering
variants is consensus-breaking (`results_root` and `rejected_root` depend on discriminants).

**`RejectReason`**

| # | Variant | # | Variant |
|---|---|---|---|
| 0 | `Malformed` | 6 | `DuplicateNonce` |
| 1 | `BadSignature` | 7 | `NonceGap` |
| 2 | `WrongRound` | 8 | `OverBudget` |
| 3 | `UnknownAccount` | 9 | `FeeInsolvent` |
| 4 | `PubkeyMismatch` | 10 | `NonceExhausted` |
| 5 | `FieldOutOfRange` | | |

**`FailReason`**

| # | Variant | # | Variant |
|---|---|---|---|
| 0 | `InsufficientBalance` | 10 | `StakeLiquidityGuard` |
| 1 | `SlippageExceeded` | 11 | `Overflow` |
| 2 | `UnknownToken` | 12 | `SupplyOutOfRange` *(reserved: unreachable — static validation catches it first with `FieldOutOfRange`)* |
| 3 | `UnknownPair` | 13 | `SelfTransferNoop` |
| 4 | `PairAlreadyExists` | 14 | `HtlcNotFound` |
| 5 | `LpTokenAsPairSide` | 15 | `HtlcBadPreimage` |
| 6 | `ZeroOutput` | 16 | `HtlcExpired` |
| 7 | `LiquidityTooSmall` | 17 | `HtlcNotExpired` |
| 8 | `ReGenesisGuard` | 18 | `HtlcDuplicateHashlock` |
| 9 | `BadPath` | | |

`ExecStatus`: 0 `Ok`, 1 `Failed(FailReason)`. `Action`: discriminants follow the order in
§4.3, tabulated in the annex.

### 13.2 Per-case overflow

"Overflow ⇒ Failed" is too generic; the frozen semantics are:

- U256 intermediates in AMM/staking that do not fit on the way back to `u128` ⇒
  `Failed(Overflow)`;
- `supply` out of range ⇒ static validation (`FieldOutOfRange`);
- `account.nonce == u64::MAX` ⇒ the account is **terminal**: every transaction of its own
  ⇒ `rejected: NonceExhausted` (never a wrapping increment);
- `native_emitted`, `native_burned`, `acc_per_stake` and `staking_reserved` are bounded by
  finite supply (§8) and overflowing them is unreachable by construction — an implementer
  still treats them with checked arithmetic, and an overflow there is a fatal bug (halt),
  never a silent wrap.

### 13.3 Change classification

**NOT consensus-breaking** (free): HTTP/WS implementation, logging, metrics, database
compaction, RPC, CLI, mirroring, wire-level rate limiting.

**Consensus-breaking** (require a new `CONSENSUS_VERSION` and, post-genesis, are effectively a
new chain): signature verification semantics, serialization, hash algorithms, state traversal
order, AMM rounding, fee computation, emission/reward formulas, the shuffle algorithm, the
round→block mapping, beacon policy.

**No cryptographic dependency upgrade enters consensus automatically**: upgrading a pinned
version in this table is a declared consensus change, never a side effect of `cargo update`.

### 13.4 Declared risks and roadmap

**drand**: the network is operational but is external infrastructure whose continuity is not
under POPCORN's control. Mitigations: the short timelock horizon (§3.5), a substitutable
`TimelockProvider`, and the fastnet precedent as a reminder that a sunset affects availability,
not funds (no long-term ciphertext exists, by construction).

**Post-freeze roadmap (P3, not security)**: migrating `primitive-types` →
`alloy-primitives`/`ruint` ONLY after the freeze, with byte-for-byte differential tests against
the current implementation — changing the arithmetic of a working consensus engine is a risk,
not a fix.

---

## 14. Implementation clarifications

Points the frozen text left implicit and the reference implementation had to decide. They are
**consensus rules** (they affect the state root or an execution outcome) and are pinned here
rather than left to the implementer.

1. **Zero-balance canonicalization.** A `balances` entry reaching `0` is **removed** from the
   map; a zero-amount entry is never written. Without this, two logically identical states
   would produce different `state_root`s. Accounts themselves are never removed once created —
   the nonce must survive.
2. **Evaluation point of the `Stake` liquidity guard.** The guard is evaluated **after** the
   pending settlement of step 1 in §8, over the balance that includes the credited `pending`.
   This matches the guard's stated purpose ("at least one `FEE_TX` stays liquid after the
   stake"), since the payout lands in the same action. On failure the whole transaction rolls
   back, settlement included.
3. **Identifier collisions are unreachable, not handled.** `TokenId` and `HtlcId` derive from
   `(account, nonce)`, and a nonce executes at most once for an account, so a collision cannot
   occur. An implementation MUST NOT silently overwrite the existing record: it returns
   `Failed(Overflow)` as a defensive catch-all and flags an internal invariant violation.
4. **`account_count`** is the cardinality of the accounts table, incremented on the creation of
   an implicit account (§4.2) and never decremented.
5. **Ordering of `blob_manifest`, `unusable` and `rejected`.** All three are sorted
   lexicographically by their 32-byte hash (`tx_id` for `rejected`). `txs` and `results` follow
   execution order and are index-aligned.
6. **Hashlock index visibility.** The `hashlock → htlc_id` index is a derived cache (§7.6):
   never serialized, never in the state root, rebuilt from the `htlcs` table at startup and at
   replay.
7. **`CreatePair` check order.** The LP-token check runs **before** the existence check. An LP
   token has no `Token` record, so checking existence first would report `UnknownToken` for
   every LP side and leave `LpTokenAsPairSide` unreachable — a dead discriminant in a committed
   enum.
8. **`CreatePair` with `token_a == token_b`** ⇒ `Failed(BadPath)`. The condition is a pure field
   check, but §6 places it at runtime, and `BadPath` is the reason for a structurally
   impossible route.
9. **Canonical token-name padding.** The 16 name bytes are printable ASCII followed by zero
   padding; a printable byte **after** a zero byte is `FieldOutOfRange`. Without this, one
   visible name would have several encodings and therefore several `TokenId`s.
10. **Manifest normalization is structural.** A batch executor sorts and deduplicates
    `blob_manifest` and `unusable` before committing to them, rather than trusting its caller's
    ordering. `collection_root` is defined over the set in lexicographic order, so a multiset
    manifest must be unrepresentable, not merely forbidden.
11. **Half-empty pools.** A pair with exactly one zero reserve has no defined price:
    `AddLiquidity` against it is `Failed(ReGenesisGuard)`, the same outcome as stranded LP over
    two zero reserves. It must be drained and restarted through the genesis branch.
12. **Grease stanzas are tolerated.** See the amendment in §3.6: "exactly one recipient stanza"
    was unimplementable against the pinned `age` version, which greases every header. The
    profile now admits one `tlock` stanza plus at most one `-grease` stanza, and refuses every
    other stanza type.
13. **Blob decryption needs no network.** Decryption requires only the chain hash and the
    round's BLS signature, and a block carries its own signature — so the collection audit of
    §10 runs offline from `/chain/export` plus a blob mirror. Only *producing* a block needs a
    live beacon.
14. **JavaScript clients must de-armor.** `tlock-js` emits armored age files; the profile
    forbids armor. See the note in §3.2: this is a client requirement, not a consensus change,
    and without it every browser-submitted blob would be `unusable`.
15. **Implementations disagree on grease, and the profile absorbs it.** Measured, not assumed:
    the Rust stack writes one `tlock` stanza plus one grease stanza, while drand's Go tlock
    writes the `tlock` stanza alone. "Exactly one `tlock` stanza, at most one grease" is the
    only rule that accepts both — which is why §3.6 reads the way it does.
