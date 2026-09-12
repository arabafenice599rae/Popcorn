"""An independent reference executor for POPCORN (SPEC.md §10).

Two implementations of one specification that diverge mean a bug in the spec or a bug in the
code, found before genesis rather than after. This one is written in Python, from the
specification text, and shares nothing with the Rust node but the numbers in `SPEC.md`.

It deliberately does *not* reimplement BLAKE3 or SHA-256 — §13 accepts primitives in their
pinned form, and the differential value lives in the logic those primitives are wired into:
the validation pipeline, the ordering, the AMM rounding, the fee phase, the staking
accumulator, the emission formula, and above all the state root traversal.

Ed25519 verification *is* reimplemented (`ed25519_strict.py`), because §3.1 pins a semantics
that implementations genuinely disagree about.
"""

import sys
from hashlib import sha256

import blake3

sys.path.insert(0, __file__.rsplit("/", 1)[0])

import borsh  # noqa: E402
import ed25519_strict as ed  # noqa: E402

# ---------------------------------------------------------------------------------------
# Parameters (§11)
# ---------------------------------------------------------------------------------------

SIGN_DOMAIN = b"popcorn-v1"
RECEIPT_DOMAIN = "popcorn-receipt-v1"
NATIVE_TOKEN = bytes(32)

MAX_TX_PER_ACCOUNT_PER_BATCH = 8
MAX_PATH_LEN = 4
MAX_PUBLISH_SIZE = 512
PUBLISH_FREE_BYTES = 128
PUBLISH_BYTE_FEE = 50
FEE_TX = 5_000
FEE_TIERS = (5, 30, 100)
MAX_SUPPLY = 10**30
MINIMUM_LIQUIDITY = 1_000
GENESIS_SUPPLY = 0
EMISSION_0 = 1_000_000_000
HALVING_INTERVAL = 10_512_000
EMISSION_STAKER_BPS = 8_500
PRECISION = 10**18
HTLC_MAX_LIFETIME_ROUNDS = 864_000
U64_MAX = 2**64 - 1
U128_MAX = 2**128 - 1


def blake3_hash(data: bytes) -> bytes:
    return blake3.blake3(data).digest()


# ---------------------------------------------------------------------------------------
# Identifiers (§4.1)
# ---------------------------------------------------------------------------------------

def token_id(creator: bytes, nonce: int) -> bytes:
    return blake3_hash(b"\x01" + creator + nonce.to_bytes(8, "little"))


def lp_token_id(pair: bytes) -> bytes:
    return blake3_hash(b"\x02" + pair)


def sort_pair(a: bytes, b: bytes):
    return (a, b) if a <= b else (b, a)


def pair_id(token_a: bytes, token_b: bytes, fee_bps: int) -> bytes:
    token0, token1 = sort_pair(token_a, token_b)
    return blake3_hash(b"\x03" + token0 + token1 + fee_bps.to_bytes(2, "little"))


def htlc_id(sender: bytes, nonce: int) -> bytes:
    return blake3_hash(b"\x04" + sender + nonce.to_bytes(8, "little"))


def tx_id(tx: dict) -> bytes:
    return blake3_hash(borsh.encode_signed_tx(tx))


def signer_of(tx: dict) -> bytes:
    return blake3_hash(tx["signer_pubkey"])


def signing_hash(payload: dict) -> bytes:
    return blake3_hash(SIGN_DOMAIN + borsh.encode_payload(payload))


# ---------------------------------------------------------------------------------------
# State (§5.4)
# ---------------------------------------------------------------------------------------

# Consensus identity (§13), written into `global` at genesis and never touched again.
CONSENSUS_VERSION = 0x0000_0009_0003

LOCK_DOMAIN = b"popcorn-consensus-lock-v1"

# The pinned dependency list of §13, in its frozen order.
CONSENSUS_LOCK = [
    ("borsh", "1.8.1"),
    ("borsh-derive", "1.8.1"),
    ("ed25519-dalek", "2.2.0"),
    ("curve25519-dalek", "4.1.3"),
    ("blake3", "1.8.7"),
    ("sha2", "0.10.9"),
    ("primitive-types", "0.14.0"),
    ("tlock", "0.0.10"),
    ("tlock_age", "0.0.10"),
    ("age", "0.11.5"),
    ("age-core", "0.11.0"),
    ("drand_core", "0.0.19"),
]


def consensus_lock_digest() -> bytes:
    """blake3(LOCK_DOMAIN || LE32(count) || per entry: LE32|name| name LE32|ver| ver)."""
    hasher = blake3.blake3()
    hasher.update(LOCK_DOMAIN)
    hasher.update(len(CONSENSUS_LOCK).to_bytes(4, "little"))
    for name, version in CONSENSUS_LOCK:
        hasher.update(len(name).to_bytes(4, "little"))
        hasher.update(name.encode())
        hasher.update(len(version).to_bytes(4, "little"))
        hasher.update(version.encode())
    return hasher.digest()


def new_state() -> dict:
    return {
        "accounts": {},
        "tokens": {},
        "pairs": {},
        "htlcs": {},
        "global": {
            "consensus_version": CONSENSUS_VERSION,
            "lock_digest": consensus_lock_digest(),
            "height": 0,
            "total_staked": 0,
            "acc_per_stake": 0,
            "staking_reserved": 0,
            "native_emitted": 0,
            "native_burned": 0,
            "account_count": 0,
        },
    }


def new_account() -> dict:
    return {"pubkey": None, "nonce": 0, "balances": {}, "staked": 0, "paid_acc": 0}


def state_root(state: dict) -> bytes:
    """Canonical state root (§5.4).

    Tables in tag order; keys in lexicographic byte order of their Borsh encoding; each key
    and value length-prefixed with LE32 so no concatenation is ambiguous.
    """
    hasher = blake3.blake3()

    def table(tag: int, mapping: dict, encode_value):
        hasher.update(bytes([tag]))
        for key in sorted(mapping.keys()):
            encoded_key = key  # every table key is a 32-byte array; Borsh encodes it as-is
            encoded_value = encode_value(mapping[key])
            hasher.update(len(encoded_key).to_bytes(4, "little"))
            hasher.update(encoded_key)
            hasher.update(len(encoded_value).to_bytes(4, "little"))
            hasher.update(encoded_value)

    table(0x01, state["accounts"], borsh.encode_account)
    table(0x02, state["tokens"], borsh.encode_token)
    table(0x03, state["pairs"], borsh.encode_pair)
    table(0x04, state["htlcs"], borsh.encode_htlc)

    # The singleton is one entry with an empty key, stated explicitly so nothing is inferred.
    hasher.update(bytes([0x05]))
    encoded_global = borsh.encode_global(state["global"])
    hasher.update((0).to_bytes(4, "little"))
    hasher.update(len(encoded_global).to_bytes(4, "little"))
    hasher.update(encoded_global)

    return hasher.digest()


def balance_of(state: dict, account_id: bytes, token: bytes) -> int:
    account = state["accounts"].get(account_id)
    return account["balances"].get(token, 0) if account else 0


def ensure_account(state: dict, account_id: bytes) -> dict:
    if account_id not in state["accounts"]:
        state["accounts"][account_id] = new_account()
        state["global"]["account_count"] += 1
    return state["accounts"][account_id]


def credit(state: dict, account_id: bytes, token: bytes, amount: int):
    if amount == 0:
        return
    account = ensure_account(state, account_id)
    new_balance = account["balances"].get(token, 0) + amount
    if new_balance > U128_MAX:
        raise Overflow()
    account["balances"][token] = new_balance


def debit(state: dict, account_id: bytes, token: bytes, amount: int):
    if amount == 0:
        return
    account = state["accounts"].get(account_id)
    if account is None or account["balances"].get(token, 0) < amount:
        raise Insufficient()
    remaining = account["balances"][token] - amount
    if remaining == 0:
        # A zero entry is removed, not stored (§14.1): otherwise two logically identical
        # states would hash differently.
        del account["balances"][token]
    else:
        account["balances"][token] = remaining


def monetary_invariant(state: dict) -> bool:
    """The five-bucket equality of §5.5.

    The AMM bucket is the one an earlier draft of the specification left out: pool reserves
    hold native units that left somebody's balance, so a chain with any native liquidity
    would have failed its own verification. This executor is what found it.
    """
    liquid = sum(a["balances"].get(NATIVE_TOKEN, 0) for a in state["accounts"].values())
    staked = sum(a["staked"] for a in state["accounts"].values())
    escrow = sum(h["amount"] for h in state["htlcs"].values() if h["token"] == NATIVE_TOKEN)
    pools = sum(
        (p["reserve0"] if p["token0"] == NATIVE_TOKEN else 0)
        + (p["reserve1"] if p["token1"] == NATIVE_TOKEN else 0)
        for p in state["pairs"].values()
    )
    g = state["global"]
    return liquid + staked + escrow + pools + g["staking_reserved"] == (
        GENESIS_SUPPLY + g["native_emitted"] - g["native_burned"]
    )


class Fail(Exception):
    """A runtime failure that rolls the transaction back and consumes the nonce."""

    def __init__(self, reason: str):
        super().__init__(reason)
        self.reason = reason


class Overflow(Fail):
    def __init__(self):
        super().__init__("Overflow")


class Insufficient(Fail):
    def __init__(self):
        super().__init__("InsufficientBalance")


# ---------------------------------------------------------------------------------------
# AMM (§6)
# ---------------------------------------------------------------------------------------

def check_u128(value: int) -> int:
    if value > U128_MAX:
        raise Overflow()
    return value


def amount_out_exact_in(amount_in, reserve_in, reserve_out, fee_bps) -> int:
    fee_num = 10_000 - fee_bps
    amount_in_with_fee = amount_in * fee_num
    denominator = reserve_in * 10_000 + amount_in_with_fee
    if denominator == 0:
        raise Fail("ZeroOutput")
    return check_u128((amount_in_with_fee * reserve_out) // denominator)


def amount_in_exact_out(amount_out, reserve_in, reserve_out, fee_bps) -> int:
    if amount_out >= reserve_out:
        # Asking for the whole reserve has no finite price (§6).
        raise Fail("SlippageExceeded")
    fee_num = 10_000 - fee_bps
    numerator = reserve_in * amount_out * 10_000
    denominator = (reserve_out - amount_out) * fee_num
    return check_u128(numerator // denominator + 1)


def integer_sqrt(value: int) -> int:
    if value < 2:
        return value
    x = value
    y = (x + 1) // 2
    while y < x:
        x = y
        y = (x + value // x) // 2
    return x


def initial_liquidity(amount0, amount1) -> int:
    root = check_u128(integer_sqrt(amount0 * amount1))
    if root <= MINIMUM_LIQUIDITY:
        raise Fail("LiquidityTooSmall")
    return root - MINIMUM_LIQUIDITY


def subsequent_liquidity(amount0, amount1, reserve0, reserve1, lp_supply) -> int:
    if reserve0 == 0 or reserve1 == 0:
        raise Fail("ReGenesisGuard")
    minted = check_u128(min(amount0 * lp_supply // reserve0, amount1 * lp_supply // reserve1))
    if minted == 0:
        raise Fail("LiquidityTooSmall")
    return minted


def actual_deposit(amount0_desired, amount1_desired, reserve0, reserve1):
    """Router02 proportional deposit (§6): only these amounts are ever debited."""
    a1_opt = check_u128(amount0_desired * reserve1 // reserve0)
    if a1_opt <= amount1_desired:
        return amount0_desired, a1_opt
    a0_opt = check_u128(amount1_desired * reserve0 // reserve1)
    return a0_opt, amount1_desired


# ---------------------------------------------------------------------------------------
# Emission (§7.2) and staking (§8)
# ---------------------------------------------------------------------------------------

def emission_at(height: int) -> int:
    if height == 0:
        return 0
    epoch = (height - 1) // HALVING_INTERVAL
    if epoch >= 128:
        return 0
    return EMISSION_0 >> epoch


def emission_split(emission: int):
    staker_share = emission * EMISSION_STAKER_BPS // 10_000
    return staker_share, emission - staker_share


def pending(account: dict, acc_per_stake: int) -> int:
    """One floor, over the accumulator difference — Synthetix `earned` (§8)."""
    if account["staked"] == 0:
        return 0
    delta = acc_per_stake - account["paid_acc"]
    if delta <= 0:
        return 0
    return account["staked"] * delta // PRECISION


def settle_rewards(state: dict, account_id: bytes) -> int:
    """Move a claim out of the reserve and into the balance. A transfer, never an emission."""
    acc = state["global"]["acc_per_stake"]
    account = state["accounts"].get(account_id, new_account())
    payout = pending(account, acc)
    if payout > state["global"]["staking_reserved"]:
        # Unreachable by the solvency proof in §8; a failure here beats a silent wrap.
        raise Overflow()
    if payout > 0:
        state["global"]["staking_reserved"] -= payout
        credit(state, account_id, NATIVE_TOKEN, payout)
    if account_id in state["accounts"]:
        state["accounts"][account_id]["paid_acc"] = acc
    return payout


# ---------------------------------------------------------------------------------------
# Fees (§5.2)
# ---------------------------------------------------------------------------------------

def tx_fee(tx: dict) -> int:
    action = tx["payload"]["action"]
    if action["kind"] == "Publish":
        billable = max(0, len(action["data"]) - PUBLISH_FREE_BYTES)
        return FEE_TX + PUBLISH_BYTE_FEE * billable
    return FEE_TX


# ---------------------------------------------------------------------------------------
# Ordering (§3.7, §5.3)
# ---------------------------------------------------------------------------------------

class BeaconRng:
    """The XOF stream seeded by blake3(drand_signature ‖ LE64(height)).

    The stream only ever advances: a rejected sample consumes its eight bytes and the next
    draw reads the following ones. Re-reading would yield a different permutation, so the
    seek offset here is the whole contract.
    """

    def __init__(self, drand_signature: bytes, height: int):
        hasher = blake3.blake3()
        hasher.update(drand_signature)
        hasher.update(height.to_bytes(8, "little"))
        self.hasher = hasher
        self.offset = 0

    def next_u64(self) -> int:
        chunk = self.hasher.digest(length=8, seek=self.offset)
        self.offset += 8
        return int.from_bytes(chunk, "little")

    def uniform(self, n: int) -> int:
        limit = U64_MAX - (U64_MAX % n)
        while True:
            x = self.next_u64()
            if x < limit:
                return x % n


def shuffle(items: list, rng: BeaconRng):
    for i in range(len(items) - 1, 0, -1):
        j = rng.uniform(i + 1)
        items[i], items[j] = items[j], items[i]


def normalize_nonces(txs: list):
    """Each account's transactions execute in ascending nonce order, in its own slots (§5.3)."""
    positions = {}
    for index, tx in enumerate(txs):
        positions.setdefault(signer_of(tx), []).append(index)
    for slots in positions.values():
        if len(slots) < 2:
            continue
        owned = sorted((txs[i] for i in slots), key=lambda tx: tx["payload"]["nonce"])
        for slot, tx in zip(slots, owned):
            txs[slot] = tx


# ---------------------------------------------------------------------------------------
# Static validation (§5.2)
# ---------------------------------------------------------------------------------------

def printable_name(name: bytes) -> bool:
    padding = False
    for byte in name:
        if byte == 0:
            padding = True
        elif 0x20 <= byte <= 0x7E:
            if padding:
                return False
        else:
            return False
    return True


def fields_in_range(tx: dict, round_number: int) -> bool:
    action = tx["payload"]["action"]
    kind = action["kind"]
    if kind == "Transfer":
        return action["amount"] > 0
    if kind == "CreateToken":
        return 1 <= action["supply"] <= MAX_SUPPLY and printable_name(action["name"])
    if kind == "CreatePair":
        return action["fee_bps"] in FEE_TIERS
    if kind in ("AddLiquidity", "RemoveLiquidity", "HtlcClaim", "HtlcRefund", "ClaimRewards"):
        return True
    if kind in ("SwapExactIn", "SwapExactOut"):
        return 1 <= len(action["path"]) <= MAX_PATH_LEN
    if kind == "Publish":
        return len(action["data"]) <= MAX_PUBLISH_SIZE
    if kind == "HtlcLock":
        return (action["amount"] > 0
                and action["expiry_round"] > round_number
                and action["expiry_round"] <= round_number + HTLC_MAX_LIFETIME_ROUNDS)
    if kind in ("Stake", "Unstake"):
        return action["amount"] > 0
    return False


def validate_batch(state: dict, txs: list, round_number: int):
    """The ordered pipeline of §5.2. Returns (valid, rejected)."""
    rejected = []
    survivors = []

    for tx in txs:
        identifier = tx_id(tx)

        # 2. signature, and with it the signer's identity
        if not ed.verify_strict(tx["signer_pubkey"], signing_hash(tx["payload"]), tx["signature"]):
            rejected.append((identifier, "BadSignature"))
            continue
        signer = signer_of(tx)

        # 3. the transaction must target this round
        if tx["payload"]["target_round"] != round_number:
            rejected.append((identifier, "WrongRound"))
            continue

        # 4. existence, then pubkey coherence, then terminal nonce — the sub-order is pinned
        account = state["accounts"].get(signer)
        if account is None:
            rejected.append((identifier, "UnknownAccount"))
            continue
        if account["pubkey"] is not None and account["pubkey"] != tx["signer_pubkey"]:
            rejected.append((identifier, "PubkeyMismatch"))
            continue
        if account["nonce"] == U64_MAX:
            rejected.append((identifier, "NonceExhausted"))
            continue

        # 5. field ranges
        if not fields_in_range(tx, round_number):
            rejected.append((identifier, "FieldOutOfRange"))
            continue

        survivors.append((tx, identifier, signer))

    survivors.sort(key=lambda entry: entry[1])
    by_account = {}
    for tx, identifier, signer in survivors:
        by_account.setdefault(signer, []).append((tx, identifier))

    valid = []
    for signer in sorted(by_account.keys()):
        entries = by_account[signer]

        # 6. nonce dedup: entries are tx_id-ordered, so the first use of a nonce wins
        kept = []
        seen = set()
        for tx, identifier in entries:
            nonce = tx["payload"]["nonce"]
            if nonce in seen:
                rejected.append((identifier, "DuplicateNonce"))
            else:
                seen.add(nonce)
                kept.append((tx, identifier))

        # 7. contiguity from account.nonce + 1
        kept.sort(key=lambda entry: entry[0]["payload"]["nonce"])
        expected = state["accounts"][signer]["nonce"] + 1
        contiguous = []
        gap_reached = False
        for tx, identifier in kept:
            if gap_reached or tx["payload"]["nonce"] != expected:
                gap_reached = True
                rejected.append((identifier, "NonceGap"))
                continue
            expected += 1
            contiguous.append((tx, identifier))

        # 8. per-account budget, keeping the lowest nonces
        if len(contiguous) > MAX_TX_PER_ACCOUNT_PER_BATCH:
            for tx, identifier in contiguous[MAX_TX_PER_ACCOUNT_PER_BATCH:]:
                rejected.append((identifier, "OverBudget"))
            contiguous = contiguous[:MAX_TX_PER_ACCOUNT_PER_BATCH]

        # 9. fee solvency against the PRE-batch balance, dropping from the highest nonce down
        balance = balance_of(state, signer, NATIVE_TOKEN)
        owed = sum(tx_fee(tx) for tx, _ in contiguous)
        while owed > balance and contiguous:
            tx, identifier = contiguous.pop()
            owed -= tx_fee(tx)
            rejected.append((identifier, "FeeInsolvent"))

        valid.extend(tx for tx, _ in contiguous)

    valid.sort(key=tx_id)
    rejected.sort(key=lambda entry: entry[0])
    return valid, rejected


# ---------------------------------------------------------------------------------------
# Execution (§5.2, §6, §7.6, §8)
# ---------------------------------------------------------------------------------------

def resolve_path(state: dict, path: list, token_in: bytes):
    hops = []
    current = token_in
    for pair_key in path:
        pair = state["pairs"].get(pair_key)
        if pair is None:
            raise Fail("UnknownPair")
        if pair["token0"] == current:
            nxt = pair["token1"]
        elif pair["token1"] == current:
            nxt = pair["token0"]
        else:
            raise Fail("BadPath")
        hops.append((pair_key, current, nxt))
        current = nxt
    return hops


def oriented_reserves(pair: dict, token_in: bytes):
    if pair["token0"] == token_in:
        return pair["reserve0"], pair["reserve1"]
    return pair["reserve1"], pair["reserve0"]


def apply_hop(state: dict, pair_key: bytes, token_in: bytes, amount_in: int, amount_out: int):
    pair = state["pairs"][pair_key]
    if pair["token0"] == token_in:
        pair["reserve0"] = check_u128(pair["reserve0"] + amount_in)
        if pair["reserve1"] < amount_out:
            raise Overflow()
        pair["reserve1"] -= amount_out
    else:
        pair["reserve1"] = check_u128(pair["reserve1"] + amount_in)
        if pair["reserve0"] < amount_out:
            raise Overflow()
        pair["reserve0"] -= amount_out


def execute_action(state: dict, signer: bytes, tx: dict, round_number: int):
    action = tx["payload"]["action"]
    kind = action["kind"]
    nonce = tx["payload"]["nonce"]

    if kind == "Transfer":
        if action["to"] == signer:
            raise Fail("SelfTransferNoop")
        debit(state, signer, action["token"], action["amount"])
        credit(state, action["to"], action["token"], action["amount"])

    elif kind == "CreateToken":
        identifier = token_id(signer, nonce)
        if identifier in state["tokens"]:
            raise Overflow()  # unreachable by nonce monotonicity (§14.3)
        state["tokens"][identifier] = {
            "id": identifier,
            "creator": signer,
            "name": action["name"],
            "total_supply": action["supply"],
        }
        credit(state, signer, identifier, action["supply"])

    elif kind == "CreatePair":
        if action["token_a"] == action["token_b"]:
            raise Fail("BadPath")
        for token in (action["token_a"], action["token_b"]):
            # The LP check precedes the existence check, or LpTokenAsPairSide is unreachable
            # and its discriminant is dead in a committed enum (§14.7).
            if any(lp_token_id(p) == token for p in state["pairs"]):
                raise Fail("LpTokenAsPairSide")
            if token != NATIVE_TOKEN and token not in state["tokens"]:
                raise Fail("UnknownToken")
        identifier = pair_id(action["token_a"], action["token_b"], action["fee_bps"])
        if identifier in state["pairs"]:
            raise Fail("PairAlreadyExists")
        token0, token1 = sort_pair(action["token_a"], action["token_b"])
        state["pairs"][identifier] = {
            "id": identifier, "token0": token0, "token1": token1,
            "fee_bps": action["fee_bps"], "reserve0": 0, "reserve1": 0, "lp_supply": 0,
        }

    elif kind == "AddLiquidity":
        pair = state["pairs"].get(action["pair"])
        if pair is None:
            raise Fail("UnknownPair")
        if action["amount0_desired"] == 0 or action["amount1_desired"] == 0:
            raise Fail("LiquidityTooSmall")

        empty = pair["reserve0"] == 0 and pair["reserve1"] == 0
        if empty:
            if pair["lp_supply"] not in (0, MINIMUM_LIQUIDITY):
                raise Fail("ReGenesisGuard")
            minted = initial_liquidity(action["amount0_desired"], action["amount1_desired"])
            actual0, actual1 = action["amount0_desired"], action["amount1_desired"]
            new_supply = minted + MINIMUM_LIQUIDITY if pair["lp_supply"] == 0 else pair["lp_supply"] + minted
        else:
            if pair["reserve0"] == 0 or pair["reserve1"] == 0:
                raise Fail("ReGenesisGuard")
            actual0, actual1 = actual_deposit(
                action["amount0_desired"], action["amount1_desired"],
                pair["reserve0"], pair["reserve1"])
            minted = subsequent_liquidity(actual0, actual1, pair["reserve0"], pair["reserve1"],
                                          pair["lp_supply"])
            new_supply = pair["lp_supply"] + minted

        if actual0 < action["amount0_min"] or actual1 < action["amount1_min"]:
            raise Fail("SlippageExceeded")

        debit(state, signer, pair["token0"], actual0)
        debit(state, signer, pair["token1"], actual1)
        credit(state, signer, lp_token_id(pair["id"]), minted)
        pair["reserve0"] += actual0
        pair["reserve1"] += actual1
        pair["lp_supply"] = new_supply

    elif kind == "RemoveLiquidity":
        pair = state["pairs"].get(action["pair"])
        if pair is None:
            raise Fail("UnknownPair")
        if action["lp_amount"] == 0:
            raise Fail("LiquidityTooSmall")
        if pair["lp_supply"] == 0:
            raise Fail("UnknownPair")
        amount0 = check_u128(action["lp_amount"] * pair["reserve0"] // pair["lp_supply"])
        amount1 = check_u128(action["lp_amount"] * pair["reserve1"] // pair["lp_supply"])
        if amount0 < action["amount0_min"] or amount1 < action["amount1_min"]:
            raise Fail("SlippageExceeded")
        debit(state, signer, lp_token_id(pair["id"]), action["lp_amount"])
        credit(state, signer, pair["token0"], amount0)
        credit(state, signer, pair["token1"], amount1)
        pair["reserve0"] -= amount0
        pair["reserve1"] -= amount1
        pair["lp_supply"] -= action["lp_amount"]

    elif kind == "SwapExactIn":
        if action["amount_in"] == 0:
            raise Fail("ZeroOutput")
        hops = resolve_path(state, action["path"], action["token_in"])
        debit(state, signer, action["token_in"], action["amount_in"])
        amount = action["amount_in"]
        token = action["token_in"]
        for pair_key, hop_in, hop_out in hops:
            pair = state["pairs"][pair_key]
            reserve_in, reserve_out = oriented_reserves(pair, hop_in)
            out = amount_out_exact_in(amount, reserve_in, reserve_out, pair["fee_bps"])
            if out == 0:
                raise Fail("ZeroOutput")
            apply_hop(state, pair_key, hop_in, amount, out)
            amount, token = out, hop_out
        if amount < action["min_amount_out"]:
            raise Fail("SlippageExceeded")
        credit(state, signer, token, amount)

    elif kind == "SwapExactOut":
        if action["amount_out"] == 0:
            raise Fail("ZeroOutput")
        hops = resolve_path(state, action["path"], action["token_in"])
        required = [0] * len(hops)
        needed = action["amount_out"]
        for index in range(len(hops) - 1, -1, -1):
            pair_key, hop_in, _ = hops[index]
            pair = state["pairs"][pair_key]
            reserve_in, reserve_out = oriented_reserves(pair, hop_in)
            value = amount_in_exact_out(needed, reserve_in, reserve_out, pair["fee_bps"])
            required[index] = value
            needed = value
        if required[0] > action["max_amount_in"]:
            raise Fail("SlippageExceeded")
        debit(state, signer, action["token_in"], required[0])
        for index, (pair_key, hop_in, _) in enumerate(hops):
            out = required[index + 1] if index + 1 < len(required) else action["amount_out"]
            if out == 0:
                raise Fail("ZeroOutput")
            apply_hop(state, pair_key, hop_in, required[index], out)
        credit(state, signer, hops[-1][2], action["amount_out"])

    elif kind == "Publish":
        pass  # the data lives in the block and nowhere else (§7.5)

    elif kind == "HtlcLock":
        if any(h["hashlock"] == action["hashlock"] for h in state["htlcs"].values()):
            raise Fail("HtlcDuplicateHashlock")
        identifier = htlc_id(signer, nonce)
        if identifier in state["htlcs"]:
            raise Overflow()
        debit(state, signer, action["token"], action["amount"])
        state["htlcs"][identifier] = {
            "id": identifier, "sender": signer, "recipient": action["to"],
            "token": action["token"], "amount": action["amount"],
            "hashlock": action["hashlock"], "expiry_round": action["expiry_round"],
        }

    elif kind == "HtlcClaim":
        htlc = state["htlcs"].get(action["htlc_id"])
        if htlc is None:
            raise Fail("HtlcNotFound")
        if sha256(action["preimage"]).digest() != htlc["hashlock"]:
            raise Fail("HtlcBadPreimage")
        if round_number > htlc["expiry_round"]:
            raise Fail("HtlcExpired")
        del state["htlcs"][action["htlc_id"]]
        credit(state, htlc["recipient"], htlc["token"], htlc["amount"])

    elif kind == "HtlcRefund":
        htlc = state["htlcs"].get(action["htlc_id"])
        if htlc is None:
            raise Fail("HtlcNotFound")
        if round_number <= htlc["expiry_round"]:
            raise Fail("HtlcNotExpired")
        del state["htlcs"][action["htlc_id"]]
        credit(state, htlc["sender"], htlc["token"], htlc["amount"])

    elif kind == "Stake":
        settle_rewards(state, signer)
        balance = balance_of(state, signer, NATIVE_TOKEN)
        if balance < action["amount"] + FEE_TX:
            # At least one fee must stay liquid, or the account could never unstake (§8).
            raise Fail("StakeLiquidityGuard")
        debit(state, signer, NATIVE_TOKEN, action["amount"])
        account = state["accounts"][signer]
        account["staked"] = check_u128(account["staked"] + action["amount"])
        account["paid_acc"] = state["global"]["acc_per_stake"]
        state["global"]["total_staked"] = check_u128(
            state["global"]["total_staked"] + action["amount"])

    elif kind == "Unstake":
        settle_rewards(state, signer)
        account = state["accounts"].get(signer)
        staked = account["staked"] if account else 0
        if staked < action["amount"]:
            raise Insufficient()
        account["staked"] -= action["amount"]
        account["paid_acc"] = state["global"]["acc_per_stake"]
        state["global"]["total_staked"] -= action["amount"]
        credit(state, signer, NATIVE_TOKEN, action["amount"])

    elif kind == "ClaimRewards":
        settle_rewards(state, signer)

    else:
        raise Fail("BadPath")


def deep_copy(state: dict) -> dict:
    """A snapshot for rollback.

    The Rust node journals what it touched; copying wholesale is slower and simpler, which is
    the right trade for a reference implementation whose job is to be obviously correct.
    """
    return {
        "accounts": {k: {"pubkey": v["pubkey"], "nonce": v["nonce"],
                         "balances": dict(v["balances"]), "staked": v["staked"],
                         "paid_acc": v["paid_acc"]} for k, v in state["accounts"].items()},
        "tokens": {k: dict(v) for k, v in state["tokens"].items()},
        "pairs": {k: dict(v) for k, v in state["pairs"].items()},
        "htlcs": {k: dict(v) for k, v in state["htlcs"].items()},
        "global": dict(state["global"]),
    }


def auto_settle(state: dict, txs: list, results: list, round_number: int):
    """Settle HTLCs whose preimage was published, after execution and before emission (§7.6)."""
    index = {h["hashlock"]: h["id"] for h in state["htlcs"].values()}
    for tx, status in zip(txs, results):
        if status != "Ok":
            continue
        action = tx["payload"]["action"]
        if action["kind"] != "Publish" or len(action["data"]) != 32:
            continue
        digest = sha256(action["data"]).digest()
        identifier = index.get(digest)
        if identifier is None:
            continue
        htlc = state["htlcs"].get(identifier)
        if htlc is None:
            index.pop(digest, None)
            continue
        if htlc["expiry_round"] < round_number:
            continue
        del state["htlcs"][identifier]
        credit(state, htlc["recipient"], htlc["token"], htlc["amount"])
        index.pop(digest, None)


def apply_emission(state: dict, height: int, foundation: bytes):
    nominal = emission_at(height)
    if nominal == 0:
        return
    staker_share, foundation_share = emission_split(nominal)
    g = state["global"]
    if g["total_staked"] == 0:
        # The staker share is not born at all in this case (§7.2).
        g["native_emitted"] += foundation_share
    else:
        g["acc_per_stake"] += staker_share * PRECISION // g["total_staked"]
        g["staking_reserved"] += staker_share
        g["native_emitted"] += nominal
    credit(state, foundation, NATIVE_TOKEN, foundation_share)


def execute_batch(state: dict, height: int, round_number: int, drand_signature: bytes,
                  txs: list, foundation: bytes, blob_manifest=None, unusable=None) -> dict:
    """One batch, in the phase order of §5.1."""
    valid, rejected = validate_batch(state, txs, round_number)

    rng = BeaconRng(drand_signature, height)
    ordered = list(valid)
    shuffle(ordered, rng)
    normalize_nonces(ordered)

    # Fees in a single phase, before any action runs: no transaction can spend the units a
    # later one owes (§5.2).
    for tx in ordered:
        fee = tx_fee(tx)
        debit(state, signer_of(tx), NATIVE_TOKEN, fee)
        state["global"]["native_burned"] += fee

    results = []
    for tx in ordered:
        signer = signer_of(tx)
        snapshot = deep_copy(state)
        try:
            execute_action(state, signer, tx, round_number)
            status = "Ok"
        except Fail as failure:
            state.clear()
            state.update(snapshot)
            status = ("Failed", failure.reason)

        # The nonce is consumed and the key materializes on every executed transaction,
        # outside the rollback (§4.2, §5.2).
        account = state["accounts"][signer]
        account["nonce"] = tx["payload"]["nonce"]
        if account["pubkey"] is None:
            account["pubkey"] = tx["signer_pubkey"]
        results.append(status)

    auto_settle(state, ordered, results, round_number)
    apply_emission(state, height, foundation)
    state["global"]["height"] = height

    manifest = sorted(set(blob_manifest or []))
    return {
        "txs": ordered,
        "results": results,
        "rejected": rejected,
        "collection_root": collection_root(manifest),
        "txs_root": txs_root(ordered),
        "rejected_root": rejected_root(rejected),
        "results_root": results_root(results),
        "state_root": state_root(state),
        "unusable": sorted(set(unusable or [])),
    }


# ---------------------------------------------------------------------------------------
# Roots (§4.3)
# ---------------------------------------------------------------------------------------

def collection_root(manifest: list) -> bytes:
    hasher = blake3.blake3()
    for entry in manifest:
        hasher.update(entry)
    return hasher.digest()


def txs_root(txs: list) -> bytes:
    hasher = blake3.blake3()
    for tx in txs:
        hasher.update(tx_id(tx))
    return hasher.digest()


def rejected_root(rejected: list) -> bytes:
    hasher = blake3.blake3()
    for identifier, reason in rejected:
        hasher.update(identifier)
        hasher.update(borsh.reject_reason(reason))
    return hasher.digest()


def results_root(results: list) -> bytes:
    encoded = borsh.u32(len(results))
    for status in results:
        encoded += borsh.exec_status(status)
    return blake3_hash(encoded)


def block_hash(header: dict) -> bytes:
    return blake3_hash(borsh.encode_header(header))
