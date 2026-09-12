// Build one transaction of every kind through the shipped browser bundle (SPEC.md §4.3).
//
// The point of running this outside a browser is that the Rust and Go tools can then be
// pointed at the result: a page that encodes a payload differently from the node does not
// fail visibly, it signs bytes the user never saw. `browser-path.sh` is the harness; this
// file is the producer.
//
//   node browser-path.mjs cases            -> JSON, one entry per action kind
//
// The target round is fixed at 1000 because that is the round whose real quicknet signature
// the repository carries, so the harness can decrypt what this produces with no network.

import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

// Take the browser branch, not Node's. `tlock-js` picks its randomness source by asking
// whether `window.crypto` exists, so without this the bundle would run a code path the page
// never runs — and the point of this file is to exercise the one it does.
globalThis.window ??= { crypto: globalThis.crypto };

const here = dirname(fileURLToPath(import.meta.url));
const bundle = await import(join(here, "..", "dist", "popcorn.mjs"));
const {
  LocalSigner, accountId, coerce, encodeSignedTx, buildSignedTx, encryptBlob,
  blake3Hash, toHex, NATIVE_TOKEN,
} = bundle;

const TARGET_ROUND = 1000n;

// A fixed secret, so the vector is the same on every run and a diff means something changed.
const SECRET = Uint8Array.from({ length: 32 }, (_, index) => (index * 7 + 13) & 0xff);

const signer = new LocalSigner(SECRET);
const account = toHex(accountId(signer.pubkey));
const native = toHex(NATIVE_TOKEN);
const someId = "ab".repeat(32);
const someOther = "cd".repeat(32);

/** One raw form-input set per action kind: exactly what the wallet form would hand `coerce`. */
const CASES = [
  ["Transfer", { token: native, to: someId, amount: "250000" }],
  ["CreateToken", { name: "POPTEST", supply: "1000000000" }],
  ["CreatePair", { tokenA: native, tokenB: someId, feeBps: "30" }],
  ["AddLiquidity", {
    pair: someId, amount0Desired: "1000000", amount1Desired: "2000000",
    amount0Min: "990000", amount1Min: "1980000",
  }],
  ["RemoveLiquidity", { pair: someId, lpAmount: "1000", amount0Min: "1", amount1Min: "1" }],
  ["SwapExactIn", { path: `${someId}\n${someOther}`, tokenIn: native, amountIn: "500", minAmountOut: "1" }],
  ["SwapExactOut", { path: someId, tokenIn: native, amountOut: "500", maxAmountIn: "100000" }],
  ["Publish", { topic: someId, data: "popcorn browser path" }],
  ["HtlcLock", {
    to: someId, token: native, amount: "77", hashlock: "secret", expiryRound: "1200",
  }],
  ["HtlcClaim", { htlcId: someId, preimage: someOther }],
  ["HtlcRefund", { htlcId: someId }],
  ["Stake", { amount: "123456789" }],
  ["Unstake", { amount: "1" }],
  ["ClaimRewards", {}],
];

const results = [];
for (const [index, [kind, raw]] of CASES.entries()) {
  const action = coerce(kind, raw);
  const payload = { nonce: BigInt(index + 1), targetRound: TARGET_ROUND, action };
  const built = await buildSignedTx(signer, payload);
  const blob = await encryptBlob(built.bytes, TARGET_ROUND);
  results.push({
    kind,
    nonce: Number(payload.nonce),
    target_round: Number(TARGET_ROUND),
    account,
    signer_pubkey: toHex(signer.pubkey),
    signing_hash: toHex(built.signingHash),
    tx_id: toHex(blake3Hash(built.bytes)),
    tx_hex: toHex(encodeSignedTx(built.tx)),
    blob_hex: toHex(blob),
    blob_hash: toHex(blake3Hash(blob)),
  });
}

process.stdout.write(`${JSON.stringify(results, null, 2)}\n`);
