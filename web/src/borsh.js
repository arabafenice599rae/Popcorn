// Borsh encoding for the POPCORN consensus types (SPEC.md §4.3, §13.1).
//
// Written from the specification, like the Python reference executor and the Go profile
// validator: a browser that encodes a payload differently from the node produces a signature
// over different bytes, and the transaction is simply rejected. So this is consensus-relevant
// code even though it runs in a tab.
//
// Amounts are u128 and nonces u64, so they are BigInt throughout — a JS number silently loses
// precision above 2^53, which here would mean signing an amount you did not intend.

/** Growable little-endian byte writer. */
export class Writer {
  constructor() {
    this.parts = [];
  }

  u8(value) {
    this.parts.push(Uint8Array.of(Number(value) & 0xff));
    return this;
  }

  #uint(value, bytes) {
    const out = new Uint8Array(bytes);
    let v = BigInt(value);
    if (v < 0n) throw new Error("unsigned value cannot be negative");
    for (let i = 0; i < bytes; i += 1) {
      out[i] = Number(v & 0xffn);
      v >>= 8n;
    }
    if (v !== 0n) throw new Error(`value does not fit in ${bytes} bytes`);
    this.parts.push(out);
    return this;
  }

  u16(value) { return this.#uint(value, 2); }
  u32(value) { return this.#uint(value, 4); }
  u64(value) { return this.#uint(value, 8); }
  u128(value) { return this.#uint(value, 16); }

  /** A fixed-size array: the bytes themselves, no length prefix. */
  fixed(bytes) {
    if (!(bytes instanceof Uint8Array)) throw new Error("expected bytes");
    this.parts.push(bytes);
    return this;
  }

  /** A Vec<u8>: u32 length, then the bytes. */
  bytes(data) {
    return this.u32(data.length).fixed(data);
  }

  /** A Vec<[u8; 32]>: u32 length, then each element unprefixed. */
  vecFixed(items) {
    this.u32(items.length);
    for (const item of items) this.fixed(item);
    return this;
  }

  finish() {
    const total = this.parts.reduce((n, part) => n + part.length, 0);
    const out = new Uint8Array(total);
    let offset = 0;
    for (const part of this.parts) {
      out.set(part, offset);
      offset += part.length;
    }
    return out;
  }
}

/** Action discriminants, in the declaration order of §4.3. The numbers are normative. */
export const ACTIONS = [
  "Transfer", "CreateToken", "CreatePair", "AddLiquidity", "RemoveLiquidity",
  "SwapExactIn", "SwapExactOut", "Publish", "HtlcLock", "HtlcClaim", "HtlcRefund",
  "Stake", "Unstake", "ClaimRewards",
];

export function encodeAction(action, w = new Writer()) {
  const index = ACTIONS.indexOf(action.kind);
  if (index < 0) throw new Error(`unknown action ${action.kind}`);
  w.u8(index);

  switch (action.kind) {
    case "Transfer":
      return w.fixed(action.token).fixed(action.to).u128(action.amount);
    case "CreateToken":
      return w.fixed(action.name).u128(action.supply);
    case "CreatePair":
      return w.fixed(action.tokenA).fixed(action.tokenB).u16(action.feeBps);
    case "AddLiquidity":
      return w.fixed(action.pair)
        .u128(action.amount0Desired).u128(action.amount1Desired)
        .u128(action.amount0Min).u128(action.amount1Min);
    case "RemoveLiquidity":
      return w.fixed(action.pair).u128(action.lpAmount)
        .u128(action.amount0Min).u128(action.amount1Min);
    case "SwapExactIn":
      return w.vecFixed(action.path).fixed(action.tokenIn)
        .u128(action.amountIn).u128(action.minAmountOut);
    case "SwapExactOut":
      return w.vecFixed(action.path).fixed(action.tokenIn)
        .u128(action.amountOut).u128(action.maxAmountIn);
    case "Publish":
      return w.fixed(action.topic).bytes(action.data);
    case "HtlcLock":
      return w.fixed(action.to).fixed(action.token).u128(action.amount)
        .fixed(action.hashlock).u64(action.expiryRound);
    case "HtlcClaim":
      return w.fixed(action.htlcId).fixed(action.preimage);
    case "HtlcRefund":
      return w.fixed(action.htlcId);
    case "Stake":
    case "Unstake":
      return w.u128(action.amount);
    case "ClaimRewards":
      return w;
    default:
      throw new Error(`unhandled action ${action.kind}`);
  }
}

export function encodePayload(payload) {
  const w = new Writer().u64(payload.nonce).u64(payload.targetRound);
  return encodeAction(payload.action, w).finish();
}

export function encodeSignedTx(tx) {
  const w = new Writer().u64(tx.payload.nonce).u64(tx.payload.targetRound);
  encodeAction(tx.payload.action, w);
  return w.fixed(tx.signerPubkey).fixed(tx.signature).finish();
}
