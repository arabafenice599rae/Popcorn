"""Run the reference executor against the scenarios the Rust node produced (SPEC.md §10).

Every scenario carries a pre-state, a batch, and what the Rust implementation computed. This
script rebuilds the pre-state, executes the batch with `popcorn_ref`, and compares: the
execution order, every result, every rejection and its reason, all five roots, and the
monetary invariant.

A mismatch is not a test failure to paper over — it is a bug in the specification or in one
of the two implementations, found before genesis. The seed identifies it exactly.

    python3 reference/differential.py vectors/differential.json
"""

import json
import sys

sys.path.insert(0, __file__.rsplit("/", 1)[0])

import borsh  # noqa: E402
import popcorn_ref as ref  # noqa: E402


def unhex(text):
    return bytes.fromhex(text)


def rebuild_state(pre: dict) -> dict:
    state = ref.new_state()
    for account in pre["accounts"]:
        state["accounts"][unhex(account["id"])] = {
            "pubkey": unhex(account["pubkey"]) if account["pubkey"] else None,
            "nonce": account["nonce"],
            "balances": {unhex(token): int(amount) for token, amount in account["balances"]},
            "staked": int(account["staked"]),
            "paid_acc": int(account["paid_acc"]),
        }
    for token in pre["tokens"]:
        state["tokens"][unhex(token["id"])] = {
            "id": unhex(token["id"]),
            "creator": unhex(token["creator"]),
            "name": unhex(token["name"]),
            "total_supply": int(token["total_supply"]),
        }
    for pair in pre["pairs"]:
        state["pairs"][unhex(pair["id"])] = {
            "id": unhex(pair["id"]),
            "token0": unhex(pair["token0"]),
            "token1": unhex(pair["token1"]),
            "fee_bps": pair["fee_bps"],
            "reserve0": int(pair["reserve0"]),
            "reserve1": int(pair["reserve1"]),
            "lp_supply": int(pair["lp_supply"]),
        }
    for htlc in pre["htlcs"]:
        state["htlcs"][unhex(htlc["id"])] = {
            "id": unhex(htlc["id"]),
            "sender": unhex(htlc["sender"]),
            "recipient": unhex(htlc["recipient"]),
            "token": unhex(htlc["token"]),
            "amount": int(htlc["amount"]),
            "hashlock": unhex(htlc["hashlock"]),
            "expiry_round": htlc["expiry_round"],
        }
    g = pre["global"]
    # Read from the vector rather than assumed, so the root is computed over what the other
    # implementation says it committed to — then checked against what this one expects.
    if g["consensus_version"] != ref.CONSENSUS_VERSION:
        raise SystemExit(
            f"consensus version mismatch: vector says {g['consensus_version']:#018x}, "
            f"this reference implements {ref.CONSENSUS_VERSION:#018x}")
    if unhex(g["lock_digest"]) != ref.consensus_lock_digest():
        raise SystemExit("consensus-lock digest mismatch: the pinned dependency lists differ")
    state["global"] = {
        "consensus_version": g["consensus_version"],
        "lock_digest": unhex(g["lock_digest"]),
        "height": g["height"],
        "total_staked": int(g["total_staked"]),
        "acc_per_stake": int(g["acc_per_stake"]),
        "staking_reserved": int(g["staking_reserved"]),
        "native_emitted": int(g["native_emitted"]),
        "native_burned": int(g["native_burned"]),
        "account_count": g["account_count"],
    }
    return state


def status_name(status) -> str:
    return "Ok" if status == "Ok" else f"Failed:{status[1]}"


def check_scenario(scenario: dict):
    """Return the list of divergences for one scenario."""
    problems = []
    seed = scenario["seed"]
    expected = scenario["expected"]

    state = rebuild_state(scenario["pre"])
    txs = [borsh.decode_signed_tx(unhex(raw)) for raw in scenario["txs"]]

    # Every transaction must survive a Borsh round trip, or the encoders disagree before the
    # executor has done anything at all.
    for raw, tx in zip(scenario["txs"], txs):
        if borsh.encode_signed_tx(tx).hex() != raw:
            problems.append(f"seed {seed}: Borsh round trip differs for a transaction")

    out = ref.execute_batch(
        state,
        height=scenario["height"],
        round_number=scenario["round"],
        drand_signature=unhex(scenario["drand_signature"]),
        txs=txs,
        foundation=unhex(scenario["foundation"]),
        blob_manifest=[unhex(entry) for entry in scenario["manifest"]],
    )

    order = [ref.tx_id(tx).hex() for tx in out["txs"]]
    if order != expected["order"]:
        problems.append(
            f"seed {seed}: execution order differs\n"
            f"  rust: {expected['order']}\n  ref:  {order}"
        )

    results = [status_name(status) for status in out["results"]]
    if results != expected["results"]:
        problems.append(
            f"seed {seed}: results differ\n"
            f"  rust: {expected['results']}\n  ref:  {results}"
        )

    rejected = [[identifier.hex(), reason] for identifier, reason in out["rejected"]]
    if rejected != expected["rejected"]:
        problems.append(
            f"seed {seed}: rejections differ\n"
            f"  rust: {expected['rejected']}\n  ref:  {rejected}"
        )

    for name in ("collection_root", "txs_root", "rejected_root", "results_root", "state_root"):
        if out[name].hex() != expected[name]:
            problems.append(
                f"seed {seed}: {name} differs\n"
                f"  rust: {expected[name]}\n  ref:  {out[name].hex()}"
            )

    if ref.monetary_invariant(state) != expected["invariant_holds"]:
        problems.append(f"seed {seed}: the two disagree about the monetary invariant")
    if not ref.monetary_invariant(state):
        problems.append(
            f"seed {seed}: the monetary invariant of §5.5 is broken after execution"
        )

    return problems


def main():
    path = sys.argv[1] if len(sys.argv) > 1 else "vectors/differential.json"
    document = json.load(open(path))
    scenarios = document["scenarios"]

    failures = []
    covered_results = set()
    covered_rejections = set()
    for scenario in scenarios:
        failures.extend(check_scenario(scenario))
        for status in scenario["expected"]["results"]:
            covered_results.add(status)
        for _, reason in scenario["expected"]["rejected"]:
            covered_rejections.add(reason)

    print(f"scenarios replayed: {len(scenarios)}")
    print(f"execution outcomes covered ({len(covered_results)}): "
          f"{', '.join(sorted(covered_results))}")
    print(f"rejection reasons covered ({len(covered_rejections)}): "
          f"{', '.join(sorted(covered_rejections))}")

    # Coverage stated plainly, including what is missing. A differential that quietly skips a
    # path proves nothing about that path, and saying so is cheaper than being asked later.
    import borsh as _borsh  # noqa: PLC0415  (imported here to keep the header tidy)

    all_fail = set(_borsh.FAIL_REASONS)
    all_reject = set(_borsh.REJECT_REASONS)
    seen_fail = {status.split(":", 1)[1] for status in covered_results if status != "Ok"}
    missing_fail = all_fail - seen_fail
    missing_reject = all_reject - covered_rejections

    expected_unreachable = {
        "Overflow": "a defensive catch-all; every field it guards is bounded by finite "
                    "supply (§13.2), so reaching it would itself be the bug",
        "SupplyOutOfRange": "declared unreachable in §13.1 — static validation catches it "
                            "first with FieldOutOfRange",
        "Malformed": "a Borsh decode failure, which makes a blob `unusable` rather than "
                     "rejected (§5.1); covered by the timelock profile tests instead",
    }
    for name in sorted(missing_fail | missing_reject):
        reason = expected_unreachable.get(name)
        if reason:
            print(f"  not covered, by design — {name}: {reason}")
        else:
            print(f"  NOT COVERED: {name} (no scenario reached it)")

    if failures:
        print(f"\n{len(failures)} DIVERGENCES:\n")
        for problem in failures[:20]:
            print(f"  {problem}")
        if len(failures) > 20:
            print(f"  ... and {len(failures) - 20} more")
        return 1

    print("\nthe two implementations agree on every scenario")
    return 0


if __name__ == "__main__":
    sys.exit(main())
