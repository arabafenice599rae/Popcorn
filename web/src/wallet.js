// The client flow of SPEC.md §3.2, end to end: nonce, target round, signature, tlock
// encryption, blind submission, receipt.
//
// The headless test drives exactly this function against a real node, so the wallet path in
// the browser is not a separately written second version of it.

import { htlcId, pairId, prepareSubmission, toBase64, toHex, tokenId } from "./popcorn.js";
import { derivedIds } from "./actions.js";

const DERIVE = { tokenId, pairId, htlcId };

/**
 * How far ahead of the head round to target, by default.
 *
 * Rounds are three seconds apart, and everything between reading the head and the node
 * receiving the blob has to fit inside the lead: a wallet prompt the user has to look at, a
 * BLS encryption that takes a second or two on a phone, and the request itself. Two rounds is
 * enough for a command-line client and not for a browser — the first run of this page against
 * a live node lost every transaction to `round is closed`. Eight rounds is twenty-four
 * seconds, which is slack a person can spend reading what they are signing.
 */
export const DEFAULT_LEAD = 8n;

/** A second, longer lead, used once if the first target round closed while we were preparing. */
const RETRY_LEAD = 20n;

/**
 * The next usable nonce: what the chain says was last executed, plus one.
 *
 * An account that has never received funds does not exist yet (§4.2), and the node says so
 * with a 404 rather than inventing an empty one. That is not an error here — a first
 * transaction from a funded-but-unseen account starts at nonce 1.
 */
export async function nextNonce(client, accountIdHex) {
  try {
    const account = await client.account(accountIdHex);
    return BigInt(account.nonce ?? 0) + 1n;
  } catch (error) {
    if (error.status === 404) return 1n;
    throw error;
  }
}

/**
 * Build, sign, encrypt and submit one action.
 *
 * `onStage` reports progress because the encryption step is the slow one — a pairing over
 * BLS12-381 in a tab — and a page that looks frozen invites a second click, which would
 * burn a nonce.
 */
export async function sendAction(client, signer, action, options = {}) {
  const { lead = DEFAULT_LEAD, onStage = () => {} } = options;
  const signerAccount = options.accountId ?? toHex(options.account ?? new Uint8Array(32));

  onStage("reading the chain head");
  const [head, nonce] = await Promise.all([
    client.head(),
    options.nonce !== undefined
      ? Promise.resolve(BigInt(options.nonce))
      : nextNonce(client, signerAccount),
  ]);

  let attempt = 0;
  let targetRound;
  let payload;
  let prepared;
  let receipt;
  for (;;) {
    const current = attempt === 0
      ? BigInt(head.drand_round ?? 0)
      : BigInt((await client.head()).drand_round ?? 0);
    targetRound = current + (attempt === 0 ? BigInt(lead) : RETRY_LEAD);
    payload = { nonce, targetRound, action };

    onStage(attempt === 0 ? "waiting for the signature" : "the round closed — signing again");
    prepared = await prepareSubmission(signer, payload);

    onStage("submitting");
    try {
      receipt = await client.submit(toBase64(prepared.blob), targetRound);
      break;
    } catch (error) {
      // The target round is part of what was signed, so a round that closed while the user
      // was reading the prompt cannot be fixed by resending the same blob: it has to be
      // built again against a later round. Once — a second failure is not a timing problem.
      if (attempt > 0 || !/closed/i.test(error.message)) throw error;
      attempt += 1;
    }
  }

  return {
    payload,
    nonce,
    targetRound,
    txId: toHex(prepared.txId),
    blobHash: toHex(prepared.blobHash),
    blobBytes: prepared.blob.length,
    signingHash: toHex(prepared.signingHash),
    receipt,
    derived: derivedIds(action, options.signerBytes ?? hexToBytes(signerAccount), nonce, DERIVE),
  };
}

function hexToBytes(text) {
  const out = new Uint8Array(text.length / 2);
  for (let i = 0; i < out.length; i += 1) {
    out[i] = Number.parseInt(text.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
}

/**
 * Watch for the transaction to be executed, without trusting the node's word for it: the
 * blob hash must appear in a block manifest, and the tx id among that block's executed ids.
 * A blob that never appears in the manifest of its target round is censorship, and the
 * signed receipt is the evidence (§9.2) — so this reports that case rather than spinning.
 */
export async function awaitInclusion(client, submission, options = {}) {
  const { timeoutMs = 90_000, pollMs = 1000 } = options;
  const deadline = Date.now() + timeoutMs;
  let height = Number(options.fromHeight ?? 0);

  while (Date.now() < deadline) {
    const head = await client.head();
    while (height < Number(head.height ?? 0)) {
      height += 1;
      const block = await client.block(height);
      const manifested = block.blob_manifest?.includes(submission.blobHash);
      const unusable = block.unusable?.includes(submission.blobHash);
      if (!manifested && !unusable) continue;
      if (unusable) {
        return { status: "unusable", height, block };
      }
      if (block.tx_ids?.includes(submission.txId)) {
        const index = block.tx_ids.indexOf(submission.txId);
        return { status: "executed", height, block, result: block.results?.[index] };
      }
      const rejection = block.rejected?.find((entry) => entry.tx_id === submission.txId);
      if (rejection) return { status: "rejected", height, block, reason: rejection.reason };
      // Manifested but neither executed nor rejected: the blob decrypted to something that
      // was not a transaction for this round, which the block records as unusable elsewhere.
      return { status: "manifested", height, block };
    }
    await new Promise((resolve) => setTimeout(resolve, pollMs));
  }
  return { status: "timeout" };
}
