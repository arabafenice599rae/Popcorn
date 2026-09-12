"""Borsh encoding and decoding for the POPCORN consensus types (SPEC.md §4.3, §13.1).

Written from the specification rather than ported from the Rust, because that is the only
version of this exercise that can find anything. Borsh is simple enough to state completely:
integers little-endian and fixed width, `Vec<T>` a u32 length then the items, `Option<T>` a
0/1 tag, `String` a u32 byte length then UTF-8, `BTreeMap` a u32 length then key/value pairs
in ascending key order, an enum a u8 discriminant then the variant's payload, a struct its
fields in declaration order with no framing at all.

The discriminant numbers are the ones tabulated in §13.1 — `results_root` and `rejected_root`
commit to them, so they are data here, not an implementation detail.
"""

from typing import Any


# ---------------------------------------------------------------------------------------
# Primitives
# ---------------------------------------------------------------------------------------

def u8(value: int) -> bytes:
    return int(value).to_bytes(1, "little")


def u16(value: int) -> bytes:
    return int(value).to_bytes(2, "little")


def u32(value: int) -> bytes:
    return int(value).to_bytes(4, "little")


def u64(value: int) -> bytes:
    return int(value).to_bytes(8, "little")


def u128(value: int) -> bytes:
    return int(value).to_bytes(16, "little")


def fixed(data: bytes) -> bytes:
    """A fixed-size array: the bytes themselves, with no length prefix."""
    return bytes(data)


def vec(items, encode_item) -> bytes:
    out = u32(len(items))
    for item in items:
        out += encode_item(item)
    return out


def byte_vec(data: bytes) -> bytes:
    return u32(len(data)) + bytes(data)


def option(value, encode_inner) -> bytes:
    return b"\x00" if value is None else b"\x01" + encode_inner(value)


def string(text: str) -> bytes:
    encoded = text.encode("utf-8")
    return u32(len(encoded)) + encoded


def btree_map(mapping: dict, encode_key, encode_value) -> bytes:
    """A map in ascending key order.

    The ordering is not a convenience: §2.3 forbids unordered collections anywhere near a
    commitment precisely so that two states with the same contents encode to the same bytes.
    """
    out = u32(len(mapping))
    for key in sorted(mapping.keys()):
        out += encode_key(key) + encode_value(mapping[key])
    return out


# ---------------------------------------------------------------------------------------
# Consensus enums (§13.1) — the numbers are normative
# ---------------------------------------------------------------------------------------

REJECT_REASONS = {
    "Malformed": 0,
    "BadSignature": 1,
    "WrongRound": 2,
    "UnknownAccount": 3,
    "PubkeyMismatch": 4,
    "FieldOutOfRange": 5,
    "DuplicateNonce": 6,
    "NonceGap": 7,
    "OverBudget": 8,
    "FeeInsolvent": 9,
    "NonceExhausted": 10,
}

FAIL_REASONS = {
    "InsufficientBalance": 0,
    "SlippageExceeded": 1,
    "UnknownToken": 2,
    "UnknownPair": 3,
    "PairAlreadyExists": 4,
    "LpTokenAsPairSide": 5,
    "ZeroOutput": 6,
    "LiquidityTooSmall": 7,
    "ReGenesisGuard": 8,
    "BadPath": 9,
    "StakeLiquidityGuard": 10,
    "Overflow": 11,
    "SupplyOutOfRange": 12,
    "SelfTransferNoop": 13,
    "HtlcNotFound": 14,
    "HtlcBadPreimage": 15,
    "HtlcExpired": 16,
    "HtlcNotExpired": 17,
    "HtlcDuplicateHashlock": 18,
}

# Action discriminants follow declaration order in §4.3.
ACTIONS = [
    "Transfer",
    "CreateToken",
    "CreatePair",
    "AddLiquidity",
    "RemoveLiquidity",
    "SwapExactIn",
    "SwapExactOut",
    "Publish",
    "HtlcLock",
    "HtlcClaim",
    "HtlcRefund",
    "Stake",
    "Unstake",
    "ClaimRewards",
]


def reject_reason(name: str) -> bytes:
    return u8(REJECT_REASONS[name])


def exec_status(status) -> bytes:
    """``Ok`` is a bare 0; ``Failed`` is 1 followed by the reason."""
    if status == "Ok":
        return u8(0)
    return u8(1) + u8(FAIL_REASONS[status[1]])


# ---------------------------------------------------------------------------------------
# Consensus structures
# ---------------------------------------------------------------------------------------

def encode_action(action: dict) -> bytes:
    kind = action["kind"]
    out = u8(ACTIONS.index(kind))

    if kind == "Transfer":
        return out + fixed(action["token"]) + fixed(action["to"]) + u128(action["amount"])
    if kind == "CreateToken":
        return out + fixed(action["name"]) + u128(action["supply"])
    if kind == "CreatePair":
        return out + fixed(action["token_a"]) + fixed(action["token_b"]) + u16(action["fee_bps"])
    if kind == "AddLiquidity":
        return (out + fixed(action["pair"]) + u128(action["amount0_desired"])
                + u128(action["amount1_desired"]) + u128(action["amount0_min"])
                + u128(action["amount1_min"]))
    if kind == "RemoveLiquidity":
        return (out + fixed(action["pair"]) + u128(action["lp_amount"])
                + u128(action["amount0_min"]) + u128(action["amount1_min"]))
    if kind == "SwapExactIn":
        return (out + vec(action["path"], fixed) + fixed(action["token_in"])
                + u128(action["amount_in"]) + u128(action["min_amount_out"]))
    if kind == "SwapExactOut":
        return (out + vec(action["path"], fixed) + fixed(action["token_in"])
                + u128(action["amount_out"]) + u128(action["max_amount_in"]))
    if kind == "Publish":
        return out + fixed(action["topic"]) + byte_vec(action["data"])
    if kind == "HtlcLock":
        return (out + fixed(action["to"]) + fixed(action["token"]) + u128(action["amount"])
                + fixed(action["hashlock"]) + u64(action["expiry_round"]))
    if kind == "HtlcClaim":
        return out + fixed(action["htlc_id"]) + fixed(action["preimage"])
    if kind == "HtlcRefund":
        return out + fixed(action["htlc_id"])
    if kind in ("Stake", "Unstake"):
        return out + u128(action["amount"])
    if kind == "ClaimRewards":
        return out
    raise ValueError(f"unknown action {kind}")


def encode_payload(payload: dict) -> bytes:
    return u64(payload["nonce"]) + u64(payload["target_round"]) + encode_action(payload["action"])


def encode_signed_tx(tx: dict) -> bytes:
    return encode_payload(tx["payload"]) + fixed(tx["signer_pubkey"]) + fixed(tx["signature"])


def encode_account(account: dict) -> bytes:
    return (option(account["pubkey"], fixed)
            + u64(account["nonce"])
            + btree_map(account["balances"], fixed, u128)
            + u128(account["staked"])
            + u128(account["paid_acc"]))


def encode_token(token: dict) -> bytes:
    return fixed(token["id"]) + fixed(token["creator"]) + fixed(token["name"]) + u128(token["total_supply"])


def encode_pair(pair: dict) -> bytes:
    return (fixed(pair["id"]) + fixed(pair["token0"]) + fixed(pair["token1"])
            + u16(pair["fee_bps"]) + u128(pair["reserve0"]) + u128(pair["reserve1"])
            + u128(pair["lp_supply"]))


def encode_htlc(htlc: dict) -> bytes:
    return (fixed(htlc["id"]) + fixed(htlc["sender"]) + fixed(htlc["recipient"])
            + fixed(htlc["token"]) + u128(htlc["amount"]) + fixed(htlc["hashlock"])
            + u64(htlc["expiry_round"]))


def encode_global(state_global: dict) -> bytes:
    """Field order is frozen (§5.4): it is hashed as written.

    The identity fields lead, as in the specification: they are written at genesis and never
    change, so every state root commits to the rules the chain was created under.
    """
    return (u64(state_global["consensus_version"])
            + fixed(state_global["lock_digest"])
            + u64(state_global["height"])
            + u128(state_global["total_staked"])
            + u128(state_global["acc_per_stake"])
            + u128(state_global["staking_reserved"])
            + u128(state_global["native_emitted"])
            + u128(state_global["native_burned"])
            + u64(state_global["account_count"]))


def encode_header(header: dict) -> bytes:
    return (u64(header["height"]) + fixed(header["prev_hash"]) + u64(header["drand_round"])
            + fixed(header["drand_sig_hash"]) + fixed(header["collection_root"])
            + fixed(header["txs_root"]) + fixed(header["rejected_root"])
            + fixed(header["results_root"]) + fixed(header["state_root"]))


def encode_receipt_payload(receipt: dict) -> bytes:
    return (string(receipt["domain"]) + fixed(receipt["blob_hash"])
            + u64(receipt["target_round"]) + u64(receipt["timestamp_ms"]))


# ---------------------------------------------------------------------------------------
# Decoding — only what the executor needs to read a transaction off the wire
# ---------------------------------------------------------------------------------------

class Reader:
    def __init__(self, data: bytes):
        self.data = data
        self.offset = 0

    def take(self, count: int) -> bytes:
        if self.offset + count > len(self.data):
            raise ValueError("truncated input")
        chunk = self.data[self.offset:self.offset + count]
        self.offset += count
        return chunk

    def u8(self) -> int:
        return self.take(1)[0]

    def u16(self) -> int:
        return int.from_bytes(self.take(2), "little")

    def u32(self) -> int:
        return int.from_bytes(self.take(4), "little")

    def u64(self) -> int:
        return int.from_bytes(self.take(8), "little")

    def u128(self) -> int:
        return int.from_bytes(self.take(16), "little")

    def finished(self) -> bool:
        return self.offset == len(self.data)


def decode_action(reader: Reader) -> dict:
    kind = ACTIONS[reader.u8()]
    if kind == "Transfer":
        return {"kind": kind, "token": reader.take(32), "to": reader.take(32), "amount": reader.u128()}
    if kind == "CreateToken":
        return {"kind": kind, "name": reader.take(16), "supply": reader.u128()}
    if kind == "CreatePair":
        return {"kind": kind, "token_a": reader.take(32), "token_b": reader.take(32), "fee_bps": reader.u16()}
    if kind == "AddLiquidity":
        return {"kind": kind, "pair": reader.take(32), "amount0_desired": reader.u128(),
                "amount1_desired": reader.u128(), "amount0_min": reader.u128(),
                "amount1_min": reader.u128()}
    if kind == "RemoveLiquidity":
        return {"kind": kind, "pair": reader.take(32), "lp_amount": reader.u128(),
                "amount0_min": reader.u128(), "amount1_min": reader.u128()}
    if kind == "SwapExactIn":
        count = reader.u32()
        path = [reader.take(32) for _ in range(count)]
        return {"kind": kind, "path": path, "token_in": reader.take(32),
                "amount_in": reader.u128(), "min_amount_out": reader.u128()}
    if kind == "SwapExactOut":
        count = reader.u32()
        path = [reader.take(32) for _ in range(count)]
        return {"kind": kind, "path": path, "token_in": reader.take(32),
                "amount_out": reader.u128(), "max_amount_in": reader.u128()}
    if kind == "Publish":
        topic = reader.take(32)
        length = reader.u32()
        return {"kind": kind, "topic": topic, "data": reader.take(length)}
    if kind == "HtlcLock":
        return {"kind": kind, "to": reader.take(32), "token": reader.take(32),
                "amount": reader.u128(), "hashlock": reader.take(32), "expiry_round": reader.u64()}
    if kind == "HtlcClaim":
        return {"kind": kind, "htlc_id": reader.take(32), "preimage": reader.take(32)}
    if kind == "HtlcRefund":
        return {"kind": kind, "htlc_id": reader.take(32)}
    if kind in ("Stake", "Unstake"):
        return {"kind": kind, "amount": reader.u128()}
    if kind == "ClaimRewards":
        return {"kind": kind}
    raise ValueError(f"unknown action discriminant for {kind}")


def decode_signed_tx(data: bytes) -> dict:
    """Decode a transaction, rejecting trailing bytes.

    Borsh is a canonical encoding: trailing data means the blob is not a transaction, which
    makes it `unusable` rather than rejected (§5.1) — there is no tx_id to reject.
    """
    reader = Reader(data)
    payload = {"nonce": reader.u64(), "target_round": reader.u64(), "action": decode_action(reader)}
    tx = {"payload": payload, "signer_pubkey": reader.take(32), "signature": reader.take(64)}
    if not reader.finished():
        raise ValueError("trailing bytes after the transaction")
    return tx
