// Drive a running node through the browser bundle (SPEC.md §3.2, §9.2).
//
// `browser-path.sh` proves the page and the node agree about bytes; this proves the rest of
// the flow actually works against a live chain — nonce, target round, blind submission,
// signed receipt, and inclusion checked against the block itself rather than against the
// node's word for it.
//
//   node live-node.mjs <node-url> <secret-key-hex> [action] [key=value ...]
//
// It is not part of CI: it needs a node that is producing blocks, which needs drand.

import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

// The page's randomness source, not Node's — see `browser-path.mjs`.
globalThis.window ??= { crypto: globalThis.crypto };

const here = dirname(fileURLToPath(import.meta.url));
const {
  LocalSigner, NodeClient, accountId, awaitInclusion, coerce, fromHex, sendAction, toHex,
} = await import(join(here, "..", "dist", "popcorn.mjs"));

const [url, secretHex, kind = "Stake", ...pairs] = process.argv.slice(2);
if (!url || !secretHex) {
  console.error("usage: live-node.mjs <node-url> <secret-key-hex> [action] [key=value ...]");
  process.exit(1);
}

const raw = Object.fromEntries(pairs.map((entry) => {
  const index = entry.indexOf("=");
  return [entry.slice(0, index), entry.slice(index + 1)];
}));

const client = new NodeClient(url);
const signer = new LocalSigner(fromHex(secretHex));
const account = toHex(accountId(signer.pubkey));

console.log(`account      ${account}`);
const before = await client.account(account).catch(() => null);
console.log(`nonce        ${before?.nonce ?? "(new account)"}`);
console.log(`balances     ${(before?.balances ?? []).map((b) => `${b.token.slice(0, 8)}…=${b.amount}`).join(" ") || "none"}`);

const head = await client.head();
const notes = [];
const action = coerce(kind, raw, notes);
for (const [label, value] of notes) console.log(`${label.padEnd(12)} ${value}`);
console.log(`\nsending      ${kind} at height ${head.height}`);

const submission = await sendAction(client, signer, action, {
  accountId: account,
  signerBytes: fromHex(account),
  lead: 8n,
  onStage: (stage) => console.log(`  … ${stage}`),
});

console.log(`nonce        ${submission.nonce}`);
console.log(`target round ${submission.targetRound}`);
console.log(`signing hash ${submission.signingHash}`);
console.log(`tx id        ${submission.txId}`);
console.log(`blob         ${submission.blobBytes} bytes, hash ${submission.blobHash}`);
console.log(`receipt      ${submission.receipt.signature}`);
for (const [label, value] of submission.derived) console.log(`${label.padEnd(12)} ${value}`);

console.log("\nwaiting for the target round…");
const inclusion = await awaitInclusion(client, submission, { fromHeight: Number(head.height) });
console.log(`status       ${inclusion.status}`);
if (inclusion.height) console.log(`block        ${inclusion.height}`);
if (inclusion.result) console.log(`result       ${inclusion.result}`);
if (inclusion.reason) console.log(`reason       ${inclusion.reason}`);

const after = await client.account(account).catch(() => null);
console.log(`\nnonce now    ${after?.nonce}`);
console.log(`staked now   ${after?.staked}`);
console.log(`balances now ${(after?.balances ?? []).map((b) => `${b.token.slice(0, 8)}…=${b.amount}`).join(" ") || "none"}`);

process.exit(inclusion.status === "executed" ? 0 : 2);
