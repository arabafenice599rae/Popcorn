"""Check the pure-Python verifier against the pinned semantics (SPEC.md §3.1).

`vectors/signatures.json` records what `ed25519-dalek`'s `verify_strict` does with each
borderline case. This script asks the independent implementation in `ed25519_strict.py` the
same questions. Every answer must match: a verifier that accepts a signature the node
rejected disagrees with it about which transactions exist.

    python3 reference/signatures.py vectors/signatures.json
"""

import json
import sys

sys.path.insert(0, __file__.rsplit("/", 1)[0])

import ed25519_strict as ed  # noqa: E402


def main():
    path = sys.argv[1] if len(sys.argv) > 1 else "vectors/signatures.json"
    document = json.load(open(path))

    mismatches = []
    for case in document["cases"]:
        got = ed.verify_strict(
            bytes.fromhex(case["public_key"]),
            bytes.fromhex(case["message"]),
            bytes.fromhex(case["signature"]),
        )
        if got != case["accepted"]:
            mismatches.append(
                f"  {case['name']}: pinned says {case['accepted']}, reference says {got}"
                f"  ({case['note']})"
            )

    print(f"borderline signature cases: {len(document['cases'])}")
    print(f"  accepted by the pinned implementation: {document['accepted_count']}")
    print(f"  rejected: {document['rejected_count']}")

    if mismatches:
        print(f"\n{len(mismatches)} DISAGREEMENTS on pinned signature semantics:\n")
        print("\n".join(mismatches))
        return 1

    print("\nthe independent verifier agrees on every case")
    return 0


if __name__ == "__main__":
    sys.exit(main())
