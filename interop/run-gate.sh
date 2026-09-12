#!/usr/bin/env bash
# The cross-language gate of SPEC.md §2.4.
#
# Three independent implementations of the same format — Rust (tlock_age over the `age`
# crate), Go (drand's own tlock over filippo.io/age), JavaScript (tlock-js) — must agree on
# two things:
#
#   1. every blob one of them produces, the other two can read;
#   2. every blob outside POPCORN-TLOCK-AGE-V1, all three reject for the same reason.
#
# The second half is the one that is easy to skip and expensive to get wrong: the acceptance
# policy is normative (§3.6), so two implementations that disagree about which blobs are
# `unusable` disagree about which transactions exist.
#
# No network. Encryption needs the chain's public key, decryption only a round signature, and
# both are pinned here.
set -uo pipefail

cd "$(dirname "$0")/.."
ROOT=$(pwd)

ROUND=1000
CHAIN=52db9ba70e0cc0f6eaf7803dd07447a1f5477735fd3f661792ba94600c84e971
# The real quicknet signature for round 1000.
SIG=b44679b9a59af2ec876b1a6b1ad52ea9b1615fc3982b19576350f93447cb1125e342b73a8dd2bacbe47e4b6b63ed5e39
PLAINTEXT=504f50434f524e2d63726f73732d6c616e67756167652d766563746f72

RUST="$ROOT/target/release/examples/interop"
GO="$ROOT/interop/go/popcorn-interop"
JS="node $ROOT/interop/js/interop.mjs"

failures=0
note() { printf '  %-28s %s\n' "$1" "$2"; }
fail() { printf '  %-28s FAIL: %s\n' "$1" "$2"; failures=$((failures + 1)); }

echo "building the three implementations"
cargo build --release -p popcorn-timelock --example interop >/dev/null 2>&1 || {
    echo "cannot build the Rust tool"; exit 1; }
(cd interop/go && GOFLAGS=-mod=mod go build -o popcorn-interop . >/dev/null 2>&1) || {
    echo "cannot build the Go tool"; exit 1; }
[ -d interop/js/node_modules ] || (cd interop/js && npm install --silent >/dev/null 2>&1) || {
    echo "cannot install the JS dependencies"; exit 1; }

run() {
    case "$1" in
        rust) shift; $RUST "$@" ;;
        go)   shift; $GO "$@" ;;
        js)   shift; $JS "$@" ;;
    esac
}

# ---------------------------------------------------------------------------------------
echo
echo "1. round trips — every producer against every consumer"
# ---------------------------------------------------------------------------------------
declare -A BLOBS
for producer in rust go js; do
    blob=$(run "$producer" encrypt "$ROUND" "$PLAINTEXT" 2>/dev/null)
    if [ -z "$blob" ]; then
        fail "$producer encrypt" "produced nothing"
        continue
    fi
    BLOBS[$producer]=$blob
done

for producer in rust go js; do
    blob=${BLOBS[$producer]:-}
    [ -z "$blob" ] && continue
    for consumer in rust go js; do
        got=$(run "$consumer" decrypt "$blob" "$SIG" 2>/dev/null | tail -1)
        if [ "$got" = "$PLAINTEXT" ]; then
            note "$producer -> $consumer" "ok"
        else
            fail "$producer -> $consumer" "got '${got:0:40}'"
        fi
    done
done

# Ciphertexts are not byte-identical across producers, and must not be expected to be: tlock
# encryption is randomized, and `age` greases its headers. What has to match is the plaintext.
echo
echo "   (ciphertexts differ by construction — tlock is randomized; plaintexts must match)"
for producer in rust go js; do
    blob=${BLOBS[$producer]:-}
    [ -n "$blob" ] && note "$producer blob" "${#blob} hex chars"
done

# ---------------------------------------------------------------------------------------
echo
echo "2. profile parity — identical verdicts on the shared vectors (§3.6)"
# ---------------------------------------------------------------------------------------
expected_file=vectors/profile/expected.json
for case_file in vectors/profile/*.hex; do
    name=$(basename "$case_file" .hex)
    blob=$(cat "$case_file")
    expected=$(python3 -c "
import json,sys
print(json.load(open('$expected_file'))['cases']['$name'])")

    verdicts=()
    for implementation in rust go js; do
        verdict=$(run "$implementation" profile "$blob" "$ROUND" "$CHAIN" 2>/dev/null | tail -1)
        verdicts+=("$verdict")
    done

    if [ "${verdicts[0]}" = "$expected" ] && [ "${verdicts[1]}" = "$expected" ] && [ "${verdicts[2]}" = "$expected" ]; then
        note "$name" "$expected (all three agree)"
    else
        fail "$name" "expected $expected, got rust=${verdicts[0]} go=${verdicts[1]} js=${verdicts[2]}"
    fi
done

# ---------------------------------------------------------------------------------------
echo
echo "3. the committed end-to-end vector, read by the other two implementations (§10)"
# ---------------------------------------------------------------------------------------
if [ -f vectors/end_to_end.json ]; then
    vector_blob=$(python3 -c "import json;print(json.load(open('vectors/end_to_end.json'))['blob'])")
    vector_sig=$(python3 -c "import json;print(json.load(open('vectors/end_to_end.json'))['beacon_signature'])")
    vector_tx=$(python3 -c "import json;print(json.load(open('vectors/end_to_end.json'))['tx_borsh'])")
    for implementation in rust go js; do
        got=$(run "$implementation" decrypt "$vector_blob" "$vector_sig" 2>/dev/null | tail -1)
        if [ "$got" = "$vector_tx" ]; then
            note "$implementation reads the vector" "ok — exact transaction bytes"
        else
            fail "$implementation reads the vector" "got '${got:0:40}'"
        fi
    done
else
    fail "end-to-end vector" "vectors/end_to_end.json is missing"
fi

echo
if [ "$failures" -eq 0 ]; then
    echo "cross-language gate: PASS"
    exit 0
fi
echo "cross-language gate: $failures FAILURES"
exit 1
