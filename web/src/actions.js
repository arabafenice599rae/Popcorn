// Action descriptors (SPEC.md §4.3).
//
// One table drives both the wallet form and the headless test, so the field list a user sees
// and the field list the browser path is tested against cannot drift apart.

import { blake3Hash, fromHex, id32, sha256, toHex } from "./popcorn.js";

/** A token name is a fixed `[u8; 16]`, zero-padded — not a length-prefixed string. */
export function nameBytes(text) {
  const encoded = new TextEncoder().encode(text);
  if (encoded.length > 16) throw new Error("a token name is at most 16 bytes");
  const out = new Uint8Array(16);
  out.set(encoded);
  return out;
}

export function nameText(bytes) {
  return new TextDecoder().decode(bytes).replace(/\0+$/, "");
}

/** Amounts are u128: BigInt end to end, so nothing is rounded on the way to a signature. */
export function amount(text) {
  const clean = String(text).trim().replace(/[_\s]/g, "");
  if (!/^[0-9]+$/.test(clean)) throw new Error("an amount is a whole number of base units");
  return BigInt(clean);
}

export function integer(text) {
  const clean = String(text).trim();
  if (!/^[0-9]+$/.test(clean)) throw new Error("expected a whole number");
  return BigInt(clean);
}

/** `data` for a publish: hex when it looks like hex, otherwise the UTF-8 of what was typed. */
export function dataBytes(text, asHex) {
  return asHex ? fromHex(text) : new TextEncoder().encode(text);
}

const field = (name, kind, label, hint = "") => ({ name, kind, label, hint });

/** Checkbox values arrive as booleans from the form and as strings from the command line. */
function isTrue(value) {
  return value === true || value === "true" || value === "1" || value === "hex";
}

/**
 * Every action, in the declaration order of §4.3 — which is also the discriminant order, so
 * this list and `ACTIONS` in borsh.js are two views of the same normative sequence.
 */
export const ACTION_FORMS = [
  {
    kind: "Transfer",
    title: "Transfer",
    blurb: "Move a token balance to another account. The fee is burned, never paid to anyone.",
    fields: [
      field("token", "token", "Token", "the native token, or any created token id"),
      field("to", "id", "Recipient account id", "32 bytes of hex — an account id, not a public key"),
      field("amount", "amount", "Amount", "in base units"),
    ],
    build: (v) => ({ kind: "Transfer", token: v.token, to: v.to, amount: v.amount }),
  },
  {
    kind: "CreateToken",
    title: "Create token",
    blurb: "Mint a fixed supply to yourself. The id derives from your account and this nonce.",
    fields: [
      field("name", "name", "Name", "at most 16 bytes of UTF-8"),
      field("supply", "amount", "Total supply", "minted once, never again"),
    ],
    build: (v) => ({ kind: "CreateToken", name: v.name, supply: v.supply }),
  },
  {
    kind: "CreatePair",
    title: "Create pair",
    blurb: "Open an AMM pair at one of the fee tiers. Sides are ordered canonically for you.",
    fields: [
      field("tokenA", "token", "Token A"),
      field("tokenB", "token", "Token B"),
      field("feeBps", "fee", "Fee tier", "basis points"),
    ],
    build: (v) => ({ kind: "CreatePair", tokenA: v.tokenA, tokenB: v.tokenB, feeBps: v.feeBps }),
  },
  {
    kind: "AddLiquidity",
    title: "Add liquidity",
    blurb: "Router02 proportional deposit: the minimums are your slippage bound, in token0/token1 order.",
    fields: [
      field("pair", "id", "Pair id"),
      field("amount0Desired", "amount", "Amount 0 desired"),
      field("amount1Desired", "amount", "Amount 1 desired"),
      field("amount0Min", "amount", "Amount 0 minimum"),
      field("amount1Min", "amount", "Amount 1 minimum"),
    ],
    build: (v) => ({
      kind: "AddLiquidity",
      pair: v.pair,
      amount0Desired: v.amount0Desired,
      amount1Desired: v.amount1Desired,
      amount0Min: v.amount0Min,
      amount1Min: v.amount1Min,
    }),
  },
  {
    kind: "RemoveLiquidity",
    title: "Remove liquidity",
    blurb: "Burn LP tokens for the underlying reserves, floor-rounded in the pool's favour.",
    fields: [
      field("pair", "id", "Pair id"),
      field("lpAmount", "amount", "LP amount to burn"),
      field("amount0Min", "amount", "Amount 0 minimum"),
      field("amount1Min", "amount", "Amount 1 minimum"),
    ],
    build: (v) => ({
      kind: "RemoveLiquidity",
      pair: v.pair,
      lpAmount: v.lpAmount,
      amount0Min: v.amount0Min,
      amount1Min: v.amount1Min,
    }),
  },
  {
    kind: "SwapExactIn",
    title: "Swap exact in",
    blurb: "Spend exactly this much, refuse anything below the minimum out.",
    fields: [
      field("path", "path", "Pair path", "one pair id per line, in hop order"),
      field("tokenIn", "token", "Token in"),
      field("amountIn", "amount", "Amount in"),
      field("minAmountOut", "amount", "Minimum amount out"),
    ],
    build: (v) => ({
      kind: "SwapExactIn",
      path: v.path,
      tokenIn: v.tokenIn,
      amountIn: v.amountIn,
      minAmountOut: v.minAmountOut,
    }),
  },
  {
    kind: "SwapExactOut",
    title: "Swap exact out",
    blurb: "Receive exactly this much, refuse to spend above the maximum in.",
    fields: [
      field("path", "path", "Pair path", "one pair id per line, in hop order"),
      field("tokenIn", "token", "Token in"),
      field("amountOut", "amount", "Amount out"),
      field("maxAmountIn", "amount", "Maximum amount in"),
    ],
    build: (v) => ({
      kind: "SwapExactOut",
      path: v.path,
      tokenIn: v.tokenIn,
      amountOut: v.amountOut,
      maxAmountIn: v.maxAmountIn,
    }),
  },
  {
    kind: "Publish",
    title: "Publish",
    blurb: "Write to the data board. No state effect: the entry lives in the block, and it is what settles an HTLC.",
    fields: [
      field("topic", "id", "Topic", "32 bytes of hex"),
      field("data", "data", "Data", "tick \u201craw bytes\u201d for hex: a 32-byte preimage published here settles a matching HTLC"),
    ],
    build: (v) => ({ kind: "Publish", topic: v.topic, data: v.data }),
  },
  {
    kind: "HtlcLock",
    title: "HTLC lock",
    blurb: "Escrow to a hashlock. The hashlock is SHA-256 — the one non-BLAKE3 point, for cross-chain use.",
    fields: [
      field("to", "id", "Recipient account id"),
      field("token", "token", "Token"),
      field("amount", "amount", "Amount"),
      field("hashlock", "hashlock", "Hashlock", "32 bytes of hex from your counterparty, or a passphrase"),
      field("expiryRound", "u64", "Expiry round", "absolute drand round"),
    ],
    build: (v) => ({
      kind: "HtlcLock",
      to: v.to,
      token: v.token,
      amount: v.amount,
      hashlock: v.hashlock,
      expiryRound: v.expiryRound,
    }),
  },
  {
    kind: "HtlcClaim",
    title: "HTLC claim",
    blurb: "Claim with the preimage. Usually unnecessary: a matching publish settles it for you.",
    fields: [
      field("htlcId", "id", "HTLC id"),
      field("preimage", "id", "Preimage", "the 32 bytes whose SHA-256 is the hashlock"),
    ],
    build: (v) => ({ kind: "HtlcClaim", htlcId: v.htlcId, preimage: v.preimage }),
  },
  {
    kind: "HtlcRefund",
    title: "HTLC refund",
    blurb: "Return an expired escrow to its sender. Anyone may invoke it — it is state garbage collection.",
    fields: [field("htlcId", "id", "HTLC id")],
    build: (v) => ({ kind: "HtlcRefund", htlcId: v.htlcId }),
  },
  {
    kind: "Stake",
    title: "Stake",
    blurb: "Stake native tokens for a share of each batch's emission.",
    fields: [field("amount", "amount", "Amount")],
    build: (v) => ({ kind: "Stake", amount: v.amount }),
  },
  {
    kind: "Unstake",
    title: "Unstake",
    blurb: "Return stake to your balance. Pending rewards are settled first.",
    fields: [field("amount", "amount", "Amount")],
    build: (v) => ({ kind: "Unstake", amount: v.amount }),
  },
  {
    kind: "ClaimRewards",
    title: "Claim rewards",
    blurb: "Move accrued staking rewards into your balance. No parameters.",
    fields: [],
    build: () => ({ kind: "ClaimRewards" }),
  },
];

export const FORM_BY_KIND = new Map(ACTION_FORMS.map((form) => [form.kind, form]));

/**
 * A passphrase turned into a usable secret.
 *
 * A preimage is a fixed `[u8; 32]` (§7.6), so hashing the passphrase directly into the
 * hashlock would produce an escrow nobody can ever claim — the preimage would be the wrong
 * length, and the only way out would be to wait for expiry and refund. The passphrase
 * therefore becomes the preimage, and the hashlock is SHA-256 of that.
 */
export function preimageFromPassphrase(text) {
  return blake3Hash(new TextEncoder().encode(text));
}

/**
 * Turn raw strings from a form into the typed values an action wants.
 *
 * `notes` collects anything the caller has to show the user afterwards — today just the
 * preimage derived from a passphrase, which is unrecoverable from the transaction itself.
 */
export function coerce(kind, raw, notes = []) {
  const form = FORM_BY_KIND.get(kind);
  if (!form) throw new Error(`unknown action ${kind}`);
  const values = {};
  for (const spec of form.fields) {
    const input = raw[spec.name];
    switch (spec.kind) {
      case "id":
      case "token":
        values[spec.name] = id32(input);
        break;
      case "amount":
        values[spec.name] = amount(input);
        break;
      case "u64":
        values[spec.name] = integer(input);
        break;
      case "fee":
        values[spec.name] = Number(integer(input));
        break;
      case "name":
        values[spec.name] = nameBytes(input);
        break;
      case "path":
        values[spec.name] = String(input)
          .split(/[\s,]+/)
          .filter((entry) => entry.length > 0)
          .map(id32);
        break;
      case "data":
        // Hex or text is not guessable from the bytes — "deadbeef" is a word as well as a
        // number — so the form carries an explicit flag rather than sniffing.
        values[spec.name] = input instanceof Uint8Array
          ? input
          : dataBytes(String(input), isTrue(raw[`${spec.name}Hex`]));
        break;
      case "hashlock": {
        // 32 bytes of hex is the hashlock itself — what a counterparty hands you. Anything
        // else is a passphrase: it becomes the preimage, and the hashlock is SHA-256 of it.
        if (/^[0-9a-fA-F]{64}$/.test(String(input).trim())) {
          values[spec.name] = id32(input);
          break;
        }
        const preimage = preimageFromPassphrase(String(input));
        values[spec.name] = sha256(preimage);
        notes.push(["preimage", toHex(preimage)]);
        break;
      }
      default:
        throw new Error(`unhandled field kind ${spec.kind}`);
    }
  }
  return form.build(values);
}

/** Ids a transaction will create, derivable before it executes (§4.1). */
export function derivedIds(action, signer, nonce, derive) {
  switch (action.kind) {
    case "CreateToken":
      return [["token id", toHex(derive.tokenId(signer, nonce))]];
    case "CreatePair":
      return [["pair id", toHex(derive.pairId(action.tokenA, action.tokenB, action.feeBps))]];
    case "HtlcLock":
      return [["HTLC id", toHex(derive.htlcId(signer, nonce))]];
    default:
      return [];
  }
}
