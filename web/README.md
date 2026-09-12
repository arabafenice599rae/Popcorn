<div align="center">
  <img src="../assets/popcorn-logo.jpg" alt="POPCORN" width="220">
  <h1>POPCORN — explorer & wallet</h1>
  <p><em>The front end the node serves, and the modules you can build your own with</em></p>
</div>

---

This directory is two things at once. It is the page a node serves at `/` — an explorer and a
wallet — and it is a small set of MIT-licensed modules that implement the client side of
[`SPEC.md`](../SPEC.md) §3.2, which you are welcome to build something else with.

Documentation for the page itself is [`docs/web.md`](../docs/web.md). This file is for people
writing their own client.

## Quick start

```bash
cd web
npm ci
./build.sh                  # bundles src/ into dist/, records the digest of every input
./test/browser-path.sh      # the gate: 14 action kinds, checked against Rust and Go
```

The node compiles `dist/` in, so after a change to `src/` you must rebuild **and** rebuild the
node. `./build.sh` without a following `cargo build` leaves the binary serving the old page —
which is why `dist/build.json` exists and why CI fails on a stale bundle.

## The modules

Nothing here depends on the DOM except `app.js` and `ui.js`. `build.sh` emits the rest as
`dist/popcorn.mjs`, a single ES module you can import from a page, a bundler or Node.

| Module | What it gives you |
|---|---|
| `borsh.js` | Canonical Borsh for the consensus types (§4.3), with the normative action discriminants. Amounts are `BigInt`: a JS number silently loses precision above 2⁵³, which here would mean signing an amount you did not intend |
| `popcorn.js` | BLAKE3, SHA-256, ed25519, the id derivations of §4.1, `signingHash`, `encryptBlob` (tlock + the mandatory de-armor), `LocalSigner` and `WalletSigner`, hex/base64/base58 |
| `actions.js` | One descriptor per action: fields, types, and `coerce` from raw form strings to typed values |
| `wallet.js` | The whole flow: next nonce, target round, sign, encrypt, submit, and `awaitInclusion` |
| `api.js` | `NodeClient` over the endpoints of §9.1, including the block websocket with reconnection |
| `ui.js`, `app.js` | The page. Yours to ignore |

```js
import {
  LocalSigner, NodeClient, accountId, coerce, fromHex, sendAction, toHex,
} from "./popcorn.mjs";

const client = new NodeClient("https://a-popcorn-node.example");
const signer = new LocalSigner(fromHex(secretKeyHex));   // or WalletSigner.connect()
const account = toHex(accountId(signer.pubkey));

const receipt = await sendAction(client, signer, coerce("Stake", { amount: "1000000" }), {
  accountId: account,
  signerBytes: fromHex(account),
});
```

Three things that are easy to get wrong, and are handled here:

- **De-armoring.** `tlock-js` returns an armored age file; the profile forbids armor (§3.6).
  An armored blob would give one transaction two blob hashes, when both the manifest and the
  receipt key on that hash. `encryptBlob` strips it.
- **The target round is signed.** It is part of the payload, so a round that closes while the
  user is reading a wallet prompt cannot be fixed by resending — the transaction has to be
  rebuilt and signed again. `sendAction` targets eight rounds ahead and retries once.
- **Amounts and nonces are `BigInt` end to end.** `u128` and `u64` do not fit in a JS number.

Encryption needs no network: the drand chain's public key is pinned, so a client works against
a node with no route to drand, and a compromised API response cannot steer it into encrypting
toward a different chain.

## Talking to a node from another origin

A node sends **no** `Access-Control-Allow-Origin` by default, because the page it serves is
same-origin and needs none. A browser front end hosted anywhere else is therefore blocked
until the operator opts in:

```bash
popcorn node --data ./chain --node-key node.key --cors 'https://your-frontend.example'
popcorn node --data ./chain --node-key node.key --cors '*'          # a public node
```

This is not a security boundary and the flag's documentation says so plainly: the API has no
cookies, no sessions and no authorization, every endpoint answers the same to everyone, and
`POST /tx` accepts bytes from anyone by design. A page that cannot reach the API from
JavaScript can still reach it from its own backend, so the header decides whether third-party
pages need a proxy — not who can read the chain. Credentials are never allowed, because there
are none.

Server-side clients (CLI, bot, backend) are unaffected and need no flag. `/stream` is
unaffected either way: WebSocket handshakes are not subject to CORS.

## A note on pools

There is no such thing as "your" pool. A `Pair` carries `id`, `token0`, `token1`, `fee_bps`,
`reserve0`, `reserve1`, `lp_supply` — and no creator. `PairId = blake3(0x03 || token0 ||
token1 || LE16(fee_bps))` is derivable by anyone who knows the two tokens and the tier, the
same couple can exist at all three tiers as separate pools, and a second `CreatePair` on an
existing id is `Failed(PairAlreadyExists)`. What is owned is an LP token balance.

So "a front end for my pools" is a front end that filters on a list of pair ids. That is a
client-side choice; nothing on-chain makes those pools exclusive to it.

## Agreeing with the node

The page decides what bytes a key signs, which makes this consensus-relevant code even though
it runs in a tab — and a client that encodes a payload differently does not fail loudly, it
produces a valid signature over something nobody asked for. If you write your own encoder,
check it the way this one is checked:

```bash
./test/browser-path.sh
```

One transaction of every one of the fourteen action kinds, built through the bundle, then:
the Rust inspector parses each one, re-encodes it to identical bytes, and confirms the account
id, tx id, signing hash and `verify_strict` all agree; Rust and Go both accept the blob under
POPCORN-TLOCK-AGE-V1; Rust decrypts it back to the signed bytes; and an armored blob is still
refused. It runs offline against drand round 1000, whose real signature the repository carries.

`test/live-node.mjs` drives the same modules against a running node — nonce, target round,
blind submission, receipt, inclusion — and is not in CI because it needs a chain producing
blocks:

```bash
node test/live-node.mjs http://127.0.0.1:8080 <secret-key-hex> Stake amount=1000000
```

## Dependencies

`tlock-js` (the age/tlock blob), `@noble/hashes` (BLAKE3, SHA-256), `@noble/curves` (ed25519,
for keys held in the tab), `esbuild` (bundling). Pinned by `package-lock.json`. There is no
framework: §2 treats the dependency list as part of the audit surface, and that discipline
does not stop at the browser.

## License

MIT, like the rest of the repository.
