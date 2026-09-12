[← Docs](README.md) · [Overview](overview.md) · [Architecture](architecture.md) · [DEX](dex.md) · **HTLC** · [Publish](publish.md) · [Staking](staking.md) · [Security](security.md) · [API](api.md) · [Node](running-a-node.md) · [Verification](verification.md)

# HTLC

> A deterministic conditional lock, and nothing more. The protocol knows neither the other
> chain nor the agreement between the parties — swaps, bridges, escrow and watchtowers are
> protocols **users** build on top. *Specification: [§7.6](../SPEC.md#76-htlc--user-carried-atomic-cross-chain-swaps-frozen)*

POPCORN has no bridge and custodies no foreign asset. What it has is a hashlock with a
deadline, which is the primitive an atomic cross-chain swap actually needs.

## The three actions

| Action | Valid when | Effect |
|---|---|---|
| `HtlcLock` | `amount > 0`, `R < expiry ≤ R + 864_000` | Debits the sender. The funds live in the `htlcs` table — **no account owns them, not even the node key** |
| `HtlcClaim` | `sha256(preimage) == hashlock` **and** `R ≤ expiry` | Credits the **recipient**. Anyone may send it: what counts is the secret, not the sender |
| `HtlcRefund` | `R > expiry` | Credits the **sender**. Invocable by anyone — garbage collection that does not depend on the sender being around |

The claim/refund boundary is sharp: `≤` against `>`, no overlap, no round where both work or
neither does.

## Settlement without a courier

Delivering the preimage should not require the recipient to be online, or to have an account,
or to exist. So at the close of every batch — **after execution, before emission** — the node
scans the `Publish` transactions that executed `Ok`, and for any `data` of exactly 32 bytes
whose `sha256` opens an unexpired lock, that lock settles to its recipient.

```
Alice locks 0.5 native to Bob, hashlock = sha256(secret)
        │
        │   Bob is offline. Bob has no transaction to send. Bob does nothing.
        ▼
Carol publishes the 32-byte secret   ← Carol is neither sender nor recipient
        │
        ▼
the lock settles to Bob, and the secret is in the block for anyone to replay
```

This has been driven on a live chain: the sender published the secret, the recipient was paid
without acting, and the preimage sits in the block on its topic.

Two consequences:

- **The preimage is carrier-independent.** Anyone can deliver it, redundantly, from anywhere.
  Censoring a settlement stops being "drop Bob's transaction" and becomes "drop every blob,
  blindly, and leave the receipts that prove you did".
- **A claim in the same batch wins.** Auto-settlement runs after execution, so a `Publish`
  carrying an already-used secret finds an empty index and is a no-op. Deterministically, with
  no race.

## Why SHA-256

It is the only place in the protocol that is not BLAKE3, and deliberately so. An atomic swap
requires the **same hash on both sides**, and SHA-256 is the lingua franca of Bitcoin Script,
Lightning, EVM and Solana. The preimage is fixed at exactly 32 bytes, the Lightning
convention, which closes the known preimage-length attacks on Bitcoin Script.

Interoperability beat internal consistency here, and that is the right trade for a primitive
whose entire purpose is to be understood by a chain that has never heard of POPCORN.

## Attacks the design assumes

| Attack | Status |
|---|---|
| **Claim censorship** (MAD-HTLC class) | On PoW/PoS you bribe miners; here the "miner" is the single operator. Auto-settlement forces mass blind censorship, which the receipts of [§9.2](../SPEC.md#92-signed-submission-receipt--collection-commitment) expose. An operator willing to incriminate itself publicly can still stall until expiry — **declared**, not solved. The step beyond is a second block producer, i.e. a different chain |
| **Timeout staggering** | Normative *for users*: the side claimed first must expire well before the other side's refund (T2 < T1), with margin for both chains. Timeouts that are too short are the classic way to lose money here |
| **Free option** | Whoever knows the preimage holds a free option until expiry. Structural to HTLCs everywhere, not specific to POPCORN. Practical mitigation: split large swaps into tranches |
| **Duplicate-hashlock griefing** | One hashlock, one HTLC — so locking a *known* hashlock first blocks others, at the cost of your own fee and locked capital. Defence: a fresh hashlock per swap, never pre-announced in the clear |
| **Eternal locks** | Impossible: `expiry ≤ R + HTLC_MAX_LIFETIME_ROUNDS` (≈ 30 days), and refund is invocable by anyone |

## Using it

```bash
# Lock, with the tool hashing your secret for you
popcorn submit --key alice.key --node http://127.0.0.1:8080 \
    htlc-lock --to <BOB> --amount 500000000 \
    --preimage <32-BYTE-HEX> --expiry-in 40          # prints the htlc id

# Or supply the hashlock directly, when the secret belongs to a counterparty on another chain
popcorn submit --key alice.key --node http://127.0.0.1:8080 \
    htlc-lock --to <BOB> --amount 500000000 --hashlock <SHA256-HEX> --expiry-in 40

# Claim directly...
popcorn submit --key bob.key --node http://127.0.0.1:8080 \
    htlc-claim --htlc-id <ID> --preimage <32-BYTE-HEX>

# ...or let anyone settle it by publishing the secret
popcorn submit --key carol.key --node http://127.0.0.1:8080 \
    publish --topic <TOPIC-HEX> --data <32-BYTE-HEX>

# After expiry, anyone can return the funds to the sender
popcorn submit --key anyone.key --node http://127.0.0.1:8080 \
    htlc-refund --htlc-id <ID>
```

`--expiry-in` counts rounds from the target round, because the client picks that round and the
caller does not know it in advance.

---

[← DEX](dex.md) · Next: [Publish & oracles →](publish.md)
