// POPCORN protocol primitives for the browser (SPEC.md §3.1, §3.2, §3.6, §4.1).
//
// Everything here is consensus-relevant even though it runs in a tab: a page that derives an
// account id, a signing hash or a blob differently from the node produces transactions the
// node will not execute — or worse, signatures over bytes the user did not read. So this
// file is written from the specification, and `test/browser-path.mjs` checks its output
// against a live node rather than against itself.

import { blake3 } from "@noble/hashes/blake3.js";
import { sha256 as nobleSha256 } from "@noble/hashes/sha256.js";
import { ed25519 } from "@noble/curves/ed25519.js";
import { timelockEncrypt, defaultChainInfo, Buffer } from "tlock-js";

import { encodePayload, encodeSignedTx } from "./borsh.js";

/** `SIGN_DOMAIN` (§3.1). The prefix is what keeps a Solana signature from replaying here. */
export const SIGN_DOMAIN = new TextEncoder().encode("popcorn-v1");

/** The pinned drand quicknet chain hash (§2.4). */
export const DRAND_CHAIN_HASH =
  "52db9ba70e0cc0f6eaf7803dd07447a1f5477735fd3f661792ba94600c84e971";

/** The native token id is the all-zero id (§4.1). */
export const NATIVE_TOKEN = new Uint8Array(32);

const ARMOR_BEGIN = "-----BEGIN AGE ENCRYPTED FILE-----";

// --------------------------------------------------------------------------------------
// Encodings
// --------------------------------------------------------------------------------------

export function toHex(bytes) {
  let out = "";
  for (const byte of bytes) out += byte.toString(16).padStart(2, "0");
  return out;
}

export function fromHex(text) {
  const clean = text.trim().replace(/^0x/, "");
  if (clean.length % 2 !== 0 || /[^0-9a-fA-F]/.test(clean)) {
    throw new Error("not hexadecimal");
  }
  const out = new Uint8Array(clean.length / 2);
  for (let i = 0; i < out.length; i += 1) {
    out[i] = Number.parseInt(clean.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
}

/** A 32-byte id from hex, rejecting anything else — ids are never truncated or padded. */
export function id32(text) {
  const bytes = fromHex(text);
  if (bytes.length !== 32) throw new Error("an id is exactly 32 bytes");
  return bytes;
}

export function toBase64(bytes) {
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
}

export function fromBase64(text) {
  const binary = atob(text);
  const out = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) out[i] = binary.charCodeAt(i);
  return out;
}

/** Base58, for Solana public keys as wallets present them. */
const B58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

export function fromBase58(text) {
  let value = 0n;
  for (const character of text) {
    const digit = B58.indexOf(character);
    if (digit < 0) throw new Error("not base58");
    value = value * 58n + BigInt(digit);
  }
  const digits = [];
  while (value > 0n) {
    digits.unshift(Number(value & 0xffn));
    value >>= 8n;
  }
  // Leading '1's are leading zero bytes, and dropping them would change the key.
  let leading = 0;
  while (leading < text.length && text[leading] === "1") leading += 1;
  return Uint8Array.from([...new Array(leading).fill(0), ...digits]);
}

export function toBase58(bytes) {
  let value = 0n;
  for (const byte of bytes) value = (value << 8n) | BigInt(byte);
  let out = "";
  while (value > 0n) {
    out = B58[Number(value % 58n)] + out;
    value /= 58n;
  }
  let leading = 0;
  while (leading < bytes.length && bytes[leading] === 0) leading += 1;
  return "1".repeat(leading) + out;
}

// --------------------------------------------------------------------------------------
// Hashes and identifiers (§3.1, §4.1)
// --------------------------------------------------------------------------------------

export function blake3Hash(...parts) {
  const hasher = blake3.create({});
  for (const part of parts) hasher.update(part);
  return hasher.digest();
}

/** SHA-256, used only for HTLC hashlocks (§7.6). */
export function sha256(bytes) {
  return nobleSha256(bytes);
}

/** `AccountId = blake3(verifying_key)` (§3.1). */
export function accountId(pubkey) {
  return blake3Hash(pubkey);
}

/** The message a wallet signs: `blake3(SIGN_DOMAIN || borsh(payload))` (§3.1). */
export function signingHash(payload) {
  return blake3Hash(SIGN_DOMAIN, encodePayload(payload));
}

const TAG_TOKEN = Uint8Array.of(0x01);
const TAG_LP_TOKEN = Uint8Array.of(0x02);
const TAG_PAIR = Uint8Array.of(0x03);
const TAG_HTLC = Uint8Array.of(0x04);

function le64(value) {
  const out = new Uint8Array(8);
  let v = BigInt(value);
  for (let i = 0; i < 8; i += 1) {
    out[i] = Number(v & 0xffn);
    v >>= 8n;
  }
  return out;
}

function le16(value) {
  return Uint8Array.of(Number(value) & 0xff, (Number(value) >> 8) & 0xff);
}

/** `TokenId = blake3(0x01 || creator || LE64(nonce))`. */
export function tokenId(creator, nonce) {
  return blake3Hash(TAG_TOKEN, creator, le64(nonce));
}

/** `LpTokenId = blake3(0x02 || PairId)`. */
export function lpTokenId(pair) {
  return blake3Hash(TAG_LP_TOKEN, pair);
}

/** Canonical side ordering: lexicographically smaller token first (§4.1). */
export function sortPair(tokenA, tokenB) {
  for (let i = 0; i < 32; i += 1) {
    if (tokenA[i] !== tokenB[i]) return tokenA[i] < tokenB[i] ? [tokenA, tokenB] : [tokenB, tokenA];
  }
  return [tokenA, tokenB];
}

/** `PairId = blake3(0x03 || token0 || token1 || LE16(fee_bps))` with `token0 < token1`. */
export function pairId(tokenA, tokenB, feeBps) {
  const [token0, token1] = sortPair(tokenA, tokenB);
  return blake3Hash(TAG_PAIR, token0, token1, le16(feeBps));
}

/** `HtlcId = blake3(0x04 || sender || LE64(nonce))`. */
export function htlcId(sender, nonce) {
  return blake3Hash(TAG_HTLC, sender, le64(nonce));
}

/** `tx_id = blake3(borsh(SignedTx))` (§4.3). */
export function txId(tx) {
  return blake3Hash(encodeSignedTx(tx));
}

// --------------------------------------------------------------------------------------
// Signers
// --------------------------------------------------------------------------------------

/**
 * A key held in this tab. Useful without a browser extension, and it is the signer the
 * headless test drives — the wallet path differs only in where the 64 bytes come from.
 */
export class LocalSigner {
  constructor(secret) {
    this.secret = secret ?? ed25519.utils.randomSecretKey();
    this.pubkey = ed25519.getPublicKey(this.secret);
  }

  get label() {
    return "local key";
  }

  async signMessage(message) {
    return ed25519.sign(message, this.secret);
  }
}

/**
 * A Solana browser wallet (Phantom, Solflare, ...). `signMessage` is ed25519 over the bytes
 * we hand it, which is exactly the 32-byte signing hash of §3.2 — no emulated Solana RPC is
 * involved, and no Solana transaction is ever constructed.
 */
export class WalletSigner {
  constructor(provider, pubkey) {
    this.provider = provider;
    this.pubkey = pubkey;
  }

  get label() {
    return this.provider.isPhantom ? "Phantom" : "browser wallet";
  }

  static available() {
    const anyWindow = globalThis.window;
    return Boolean(anyWindow?.solana ?? anyWindow?.solflare);
  }

  static async connect() {
    const anyWindow = globalThis.window;
    const provider = anyWindow?.solana ?? anyWindow?.solflare;
    if (!provider) throw new Error("no Solana wallet found in this browser");
    const response = await provider.connect();
    const key = response?.publicKey ?? provider.publicKey;
    const bytes = key?.toBytes ? key.toBytes() : fromBase58(String(key));
    return new WalletSigner(provider, bytes);
  }

  async signMessage(message) {
    const result = await this.provider.signMessage(message, "utf8");
    const signature = result?.signature ?? result;
    return signature instanceof Uint8Array ? signature : new Uint8Array(signature);
  }
}

// --------------------------------------------------------------------------------------
// Transaction construction (§4.3, §3.2)
// --------------------------------------------------------------------------------------

/** Sign a payload with any signer and return the `SignedTx` shape plus its Borsh bytes. */
export async function buildSignedTx(signer, payload) {
  const message = signingHash(payload);
  const signature = await signer.signMessage(message);
  if (signature.length !== 64) throw new Error("the wallet returned a malformed signature");
  const tx = { payload, signerPubkey: signer.pubkey, signature };
  return { tx, bytes: encodeSignedTx(tx), signingHash: message };
}

/**
 * Strip the age armor `tlock-js` insists on producing.
 *
 * Mandatory, not cosmetic: POPCORN-TLOCK-AGE-V1 forbids armor (§3.6), and an armored blob
 * would be a second encoding of the same ciphertext — one transaction with two blob hashes,
 * when both the manifest and the submission receipt key on that hash.
 */
export function dearmor(armored) {
  if (!armored.startsWith(ARMOR_BEGIN)) {
    throw new Error("expected an armored age file from tlock-js");
  }
  const body = armored
    .split("\n")
    .filter((line) => line.length > 0 && !line.startsWith("-----"))
    .join("");
  return fromBase64(body);
}

/** A drand client that never touches the network: encryption only needs the chain's key. */
function pinnedChainClient() {
  if (defaultChainInfo.hash !== DRAND_CHAIN_HASH) {
    throw new Error("tlock-js default chain is not the pinned quicknet chain");
  }
  return {
    chain: () => ({ info: async () => defaultChainInfo }),
    get: async () => {
      throw new Error("this client encrypts only");
    },
    options: { disableBeaconVerification: false },
  };
}

/**
 * Encrypt transaction bytes toward `round` and return the binary blob to submit.
 *
 * No network call: the chain public key is pinned, so a wallet works against a node that has
 * no route to drand at all — and the page cannot be made to encrypt toward someone else's
 * chain by a compromised API response.
 */
export async function encryptBlob(bytes, round) {
  const armored = await timelockEncrypt(Number(round), Buffer.from(bytes), pinnedChainClient());
  return dearmor(armored);
}

/** The whole client flow of §3.2, from payload to the bytes that go into `POST /tx`. */
export async function prepareSubmission(signer, payload) {
  const built = await buildSignedTx(signer, payload);
  const blob = await encryptBlob(built.bytes, payload.targetRound);
  return {
    ...built,
    blob,
    blobHash: blake3Hash(blob),
    txId: blake3Hash(built.bytes),
  };
}
