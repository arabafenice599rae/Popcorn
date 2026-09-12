[← Docs](README.md) · [Overview](overview.md) · [Architecture](architecture.md) · [DEX](dex.md) · [HTLC](htlc.md) · [Publish](publish.md) · [Staking](staking.md) · **Security** · [API](api.md) · [Web](web.md) · [Node](running-a-node.md) · [Verification](verification.md)

# Security

> This page is mostly about what POPCORN **cannot** do. A system that only documents its
> guarantees is asking to be trusted about the part nobody checked.
> *Specification: [§1](../SPEC.md#1-trust-model), [§9.2](../SPEC.md#92-signed-submission-receipt--collection-commitment), [§11](../SPEC.md#11-protocol-parameters)*

## Threat model

One operator runs the only node. Assume they are willing to lie whenever lying is profitable
and undetectable. The design's whole job is to make the second half false.

| The operator can | The operator cannot |
|---|---|
| Refuse to include a transaction | Include one you did not sign |
| Stop producing blocks | Produce a block on an unverified beacon |
| Choose what to serve over the API | Alter a past block without breaking the header chain |
| Spend the foundation's funds — they hold that key | Spend yours, or touch funds escrowed in an HTLC |
| Wait for the beacon before closing collection | Deny having received a blob it gave a receipt for |
| | Emit outside the formula, or change the supply, without diverging under replay |

## Two keys, and why

| Key | Holds | Signs | Kept |
|---|---|---|---|
| **Node key** | nothing | blocks | hot, on the server |
| **Foundation key** | value | no blocks | cold, offline |

They are distinct ed25519 keys, both stamped into genesis, and the node process only ever loads
the first. The consequences of losing each are different and worth stating separately:

- **Node key compromised.** No funds move. But an attacker can produce a **validly signed
  fork**. The divergence is publicly detectable; choosing which history is canonical is *not*
  something a single-operator model can settle by itself. Mitigations are operational —
  timestamped external mirrors, observers archiving headers — and they are mitigations, not a
  solution.
- **Foundation key compromised.** The foundation's funds are gone. The ledger is untouched and
  every other account is unaffected.

## Censorship: provable, not prevented

The chain cannot stop an operator from dropping your transaction. What it can do is make the
drop leave evidence:

```
1. you submit a blob        → the node signs a RECEIPT for it
2. the node freezes the set → collection_root commits to the blobs it received
3. the block is published   → every manifest entry must resolve into
                              txs, rejected, or unusable — no fourth option
```

- **Receipt issued, hash missing from the manifest** → two signatures by the same node that
  contradict each other. Censorship proven by the block itself, without any replay.
- **A false `unusable` claim** → anyone holding the blob and the public beacon can decrypt it
  and show the claim is false.
- **Refusing to serve a manifested blob** → visible obstruction. `GET /blob/{hash}` answering
  for manifested blobs is an operational obligation precisely so that its absence is an event.

The residue, stated rather than hidden: **blind refusal at ingress**. If the node never
receipts you, you have no proof you sent anything. It is blind by construction — the timelock
means the node does not know what it is refusing — and that is the accepted limit.

There is one more, and it is structural: consensus does **not** force collection to close
before the beacon. A node may wait, decrypt, and then choose what to manifest, producing a
formally valid block. Your protection against that window is a receipt obtained before your own
deadline, which is client policy, not a consensus rule.

## Signature semantics are pinned

RFC 8032 is under-specified at the edges and implementations disagree there. POPCORN pins one:
`ed25519-dalek`'s `verify_strict` — canonical `s`, no small-order `A` or `R`, cofactorless
equation. A verifier that accepts a signature the node rejected disagrees about which
transactions exist, so the repository carries **26 borderline vectors** (small-order points,
`s` at and above the group order, non-canonical encodings) and an independent pure-Python
verifier that must agree on every one.

The signing domain `"popcorn-v1"` prefixes every signed preimage, which is what stops a
signature produced for a Solana wallet from being replayed here and vice versa.

## Availability: the measured cost of a flood

Collection is blind, so no fee can be charged before decryption — the signer is not known until
then. That makes CPU the only thing standing between the node and a flood, and the numbers are
not the ones intuition suggests. Measured at the 10,000-blob batch ceiling, one core:

| Population | Per blob | One full round |
|---|---|---|
| Garbage | ~0 ms | **0.1%** of a round |
| Corrupt tlock stanza | 0.08 ms | 26% |
| Headers padded to the 1 KiB limit | 0.70 ms | 235% |
| Valid ciphertexts | 2.52 ms | 839% |
| Corrupt payload | 2.51 ms | 837% |
| Decrypts but does not decode | 2.55 ms | **850%** |

**Garbage is free to refuse.** What costs is anything well-formed enough to reach the timelock
unwrap, because the pairing work happens before the AEAD or the decoder can object.

Two things follow:

1. `MAX_TX_PER_BATCH = 10_000` is **not reachable** at 3 s per round on one core — a full batch
   of the worst population takes 25.5 s. Decryption runs across cores (safe: the derived set
   does not depend on the order it is computed in), which gives 6.4 s on four. Roughly **nine
   cores** are needed. An operator provisions for that or sets
   `MAX_TLOCK_DECRYPT_WORK_PER_ROUND` to a ceiling they can meet, and the declared degradation
   applies: the chain **waits**, never skipping a manifested blob.
2. **The attacker's cost is not the node's cost.** A blob with a corrupt payload costs the node
   the same 2.51 ms as an honest one and costs its sender nothing — no fee, no valid signature,
   no key, no account. That asymmetry *is* the attack, and it is why the defences are
   wire-level admission limits rather than anything the protocol can charge for.

Run it yourself: `cargo run --release -p popcorn-timelock --example dos_benchmark -- 10000`.

## External dependencies

| Dependency | Risk | Why it is bounded |
|---|---|---|
| **drand** | External infrastructure whose continuity nobody here controls | Blobs are accepted only for rounds within `BLOB_ROUND_HORIZON` (≈ 10 min). The timelock is fair ordering over minutes, never long-term storage — so a drand sunset would affect availability, not the decryptability of anything, because distant ciphertexts do not exist |
| **tlock / age** | Vendored, and `tlock` itself is **not audited** | The scheme is peer-reviewed and the format is shared with drand's Go and JS implementations; the cross-language gate proves all three agree, including on what to reject |
| **Everything in a hash** | A version bump could silently change bytes | Pinned exactly in [`CONSENSUS-LOCK.md`](../CONSENSUS-LOCK.md). Upgrading one is a declared consensus change, never a side effect of `cargo update` |

## Out of scope, deliberately

No VM and no user code. No concentrated liquidity, hooks, native limit orders, flash loans or
on-chain TWAP. No P2P and no distributed consensus. No oracle beyond drand. **No protocol
bridge**: HTLCs are a primitive users build swaps with, and any custodial bridge is third-party
activity with the responsibilities that come with it.

---

[← Staking](staking.md) · Next: [API →](api.md)
