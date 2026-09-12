[← Docs](README.md) · [Overview](overview.md) · [Architecture](architecture.md) · [DEX](dex.md) · [HTLC](htlc.md) · [Publish](publish.md) · [Staking](staking.md) · [Security](security.md) · [API](api.md) · **Web** · [Node](running-a-node.md) · [Verification](verification.md)

# Explorer & wallet

> The node serves its own front end. Not for convenience: the client flow puts a **signature**
> and a **timelock encryption** in the browser, and a page fetched from somewhere else is a
> page that can be swapped for one that signs something else.
> *Specification: [§3.2](../SPEC.md#32-client-flows), [§3.6](../SPEC.md#36-popcorn-tlock-age-v1-consensus-rule), [§9.1](../SPEC.md#91-endpoints)*

Start a node and open it:

```
popcorn node --data ./chain --node-key ./node.key --listen 127.0.0.1:8080
# explorer and wallet: http://127.0.0.1:8080/
```

`--no-web` serves the endpoints only. That is an operational choice, not a consensus one
([§13.3](../SPEC.md#133-change-classification)): a node with no page still serves everything a
verifier needs.

## What the page is

| | |
|---|---|
| **Explorer** | Live blocks over the websocket, block detail with manifest and rejections, accounts, tokens, pairs, the data board, and the five-bucket supply with its invariant |
| **Wallet** | Connect a Solana browser wallet or a key held in the tab, then build, sign, encrypt and submit any of the fourteen actions |
| **Served from** | The node itself — same origin as the API, so there is no CORS surface and one address to hand out |
| **Fetched from elsewhere** | Nothing. The page loads no script, style, font or image from any other origin, and the node sends a `Content-Security-Policy` that says so |

## The wallet flow

Every step is [§3.2](../SPEC.md#32-client-flows), in order:

```
       payload              blake3("popcorn-v1" || borsh(payload))
  ┌──────────────┐                       │
  │ nonce        │                       ▼
  │ target_round │        wallet.signMessage(32 bytes)  ── Phantom, Solflare,
  │ action       │                       │                 or a key in the tab
  └──────────────┘                       ▼
                              SignedTx (Borsh)
                                         │
                          tlock toward the target round
                                         │
                                    de-armor  ◀── mandatory for JavaScript
                                         │
                                  POST /tx (blind)
                                         │
                              signed receipt (§9.2)
```

Four things about it are worth stating plainly.

**No Solana RPC is emulated.** The wallet is used for exactly one thing: an ed25519 signature
over 32 bytes. No Solana transaction is ever constructed, and the `popcorn-v1` domain prefix
is what keeps a signature made here from being replayed anywhere else
([§3.1](../SPEC.md#31-identity-and-signatures)).

**De-armoring is not optional.** `tlock-js` returns an armored age file, and the profile
forbids armor ([§3.6](../SPEC.md#36-popcorn-tlock-age-v1-consensus-rule)) — armor is a second
encoding of the same ciphertext, so accepting it would give one transaction two blob hashes,
when both the manifest and the receipt key on that hash. The page strips the wrapper before
submitting, and the gate below fails if that ever regresses.

**The target round is part of what you sign.** The page targets eight rounds ahead — about
twenty-four seconds — because everything between reading the head and the node receiving the
blob has to fit inside that: a wallet prompt someone has to read, a BLS encryption that takes a
second or two on a phone, and the request. If the round closes anyway, the transaction cannot
simply be resent: it is rebuilt against a later round and signed again, once.

**Inclusion is checked against the block, not against the node.** After submitting, the page
polls until the blob hash appears in a block manifest, then looks for the tx id among that
block's executed ids. A blob that never appears in the manifest of its target round is exactly
the case the signed receipt exists for ([§9.2](../SPEC.md#92-signed-submission-receipt--collection-commitment)).

## Encryption needs no network

The drand chain's public key is pinned in the page, so encrypting toward a future round is a
local operation. A wallet therefore works against a node with no route to drand at all, and a
compromised API response cannot steer the page into encrypting toward someone else's chain.

## Building it

The bundle lives in `web/dist/` and is **committed**, because the node compiles it in —
building a node needs nothing but `cargo`, and there is no directory of assets to lose.

```
cd web
./build.sh          # bundles src/ into dist/, and records the digest of every input
```

`dist/build.json` holds the SHA-256 of each source file that went into the bundle. `node
digest.mjs --check` compares the committed bundle against the current sources, which is how a
"changed the page but forgot to rebuild" ends up failing CI rather than shipping.

## The gate

```
./web/test/browser-path.sh
```

The page decides what bytes a user's key signs, which makes it consensus-relevant code even
though it runs in a tab — and a browser that encodes a payload differently does not fail
loudly, it produces a valid signature over something nobody asked for. So the gate builds one
transaction of **every one of the fourteen action kinds** through the page's own bundle, and
then asks the other implementations:

1. the Rust inspector parses each transaction, re-encodes it to identical bytes, and confirms
   the account id, the tx id, the signing hash and `verify_strict` all agree with what the page
   computed;
2. Rust and Go both accept the blob under POPCORN-TLOCK-AGE-V1, and Rust decrypts it back to
   exactly the bytes the page signed;
3. an armored blob is refused, which is the one JavaScript-specific hazard.

It runs offline: the target round is 1000, and the repository carries that round's real
quicknet signature.

There is a second script, `web/test/live-node.mjs`, which drives the same bundle against a
running node — nonce, target round, blind submission, receipt, inclusion. It is not in CI
because it needs a chain that is producing blocks, which needs drand.

## What the page cannot do for you

It is a client. It can show you that the invariant holds and that your transaction was
executed, but believing either is still a choice — the [verification](verification.md) page is
how you stop having to. The footer links straight to `/params` and `/chain/export` for that
reason.
