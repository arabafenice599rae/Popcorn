#!/usr/bin/env bash
# The browser-path gate (SPEC.md §3.2, §3.6, §4.3).
#
# A wallet that runs in a tab is consensus-relevant code: it decides what bytes the user's key
# signs. A browser that encodes a payload differently from the node does not fail loudly — it
# produces a valid signature over something nobody asked for. So the page's own bundle builds
# one transaction of every kind, and the Rust and Go tools then say whether they agree:
#
#   1. the Borsh bytes parse as a SignedTx, re-encode identically, and carry the signature,
#      account id and signing hash the page computed;
#   2. the blob the page submits conforms to POPCORN-TLOCK-AGE-V1 in all three
#      implementations — de-armored, as §3.2 requires of JavaScript clients;
#   3. decrypting that blob returns exactly the bytes the page signed.
#
# No network: the round is 1000 and the repository carries that round's real quicknet
# signature, the same one the cross-language gate uses.
set -uo pipefail

cd "$(dirname "$0")/.."
WEB=$(pwd)
ROOT=$(cd .. && pwd)

ROUND=1000
CHAIN=52db9ba70e0cc0f6eaf7803dd07447a1f5477735fd3f661792ba94600c84e971
SIG=b44679b9a59af2ec876b1a6b1ad52ea9b1615fc3982b19576350f93447cb1125e342b73a8dd2bacbe47e4b6b63ed5e39

failures=0
note() { printf '  %-28s %s\n' "$1" "$2"; }
fail() { printf '  %-28s FAIL: %s\n' "$1" "$2"; failures=$((failures + 1)); }

echo "building"
[ -d node_modules ] || npm ci --silent || { echo "cannot install the web dependencies"; exit 1; }
# Before rebuilding: is the committed bundle the one these sources produce? The node compiles
# `dist/` in, so a stale bundle would ship a page nobody tested.
stale=""
node digest.mjs --check >/dev/null 2>&1 || stale="yes"
./build.sh >/dev/null || { echo "cannot build the browser bundle"; exit 1; }
(cd "$ROOT" && cargo build --release -p popcorn-timelock --example interop >/dev/null 2>&1) || {
    echo "cannot build the Rust interop tool"; exit 1; }
(cd "$ROOT" && cargo build --release -p popcorn-core --example inspect_tx >/dev/null 2>&1) || {
    echo "cannot build the Rust transaction inspector"; exit 1; }
GO_TOOL=""
if command -v go >/dev/null 2>&1; then
    (cd "$ROOT/interop/go" && GOFLAGS=-mod=mod go build -o popcorn-interop . >/dev/null 2>&1) \
        && GO_TOOL="$ROOT/interop/go/popcorn-interop"
fi
[ -n "$GO_TOOL" ] || echo "  (go not available: skipping the Go half of the profile check)"

RUST="$ROOT/target/release/examples/interop"
INSPECT="$ROOT/target/release/examples/inspect_tx"
CASES=$(mktemp)
trap 'rm -f "$CASES"' EXIT

# ---------------------------------------------------------------------------------------
echo
echo "0. the bundle is current"
# ---------------------------------------------------------------------------------------
# `dist/` is committed so that building the node needs nothing but cargo. That is only safe
# if a source change without a rebuild is detectable, which is what the digest is for.
if [ -n "$stale" ]; then
    node digest.mjs --check 2>&1 | sed 's/^/  /'
    fail "committed bundle" "run web/build.sh and commit dist/"
else
    note "committed bundle" "matches its sources ($(node digest.mjs | cut -c1-16)…)"
fi

# ---------------------------------------------------------------------------------------
echo
echo "1. the page builds one transaction of every kind"
# ---------------------------------------------------------------------------------------
if ! node test/browser-path.mjs > "$CASES" 2>/tmp/browser-path.err; then
    echo "  the browser bundle failed to produce transactions:"
    sed 's/^/    /' /tmp/browser-path.err
    exit 1
fi
cases_field() { CASES_FILE="$CASES" node -e '
  const cases = JSON.parse(require("fs").readFileSync(process.env.CASES_FILE, "utf8"));
  const index = process.env.INDEX;
  process.stdout.write(index === undefined
    ? String(cases.length)
    : process.env.FIELDS.split(",").map((field) => cases[index][field]).join(" "));
'; }
count=$(cases_field)
note "cases" "$count"
[ "$count" -eq 14 ] || fail "cases" "expected one per action kind, got $count"

# ---------------------------------------------------------------------------------------
echo
echo "2. Rust agrees about the bytes, the identity and the signature"
# ---------------------------------------------------------------------------------------
for index in $(seq 0 $((count - 1))); do
    read -r kind tx_hex account tx_id signing_hash <<<"$(
        INDEX=$index FIELDS=kind,tx_hex,account,tx_id,signing_hash cases_field)"

    verdict=$("$INSPECT" "$tx_hex" 2>&1)
    if [ $? -ne 0 ]; then
        fail "$kind" "the node cannot parse what the page signed: $verdict"
        continue
    fi
    read -r got_canonical got_valid got_account got_tx_id got_hash got_action <<<"$(
        VERDICT="$verdict" node -e '
          const v = JSON.parse(process.env.VERDICT);
          process.stdout.write([v.canonical, v.signature_valid, v.account, v.tx_id,
                                v.signing_hash, v.action].join(" "));')"

    [ "$got_canonical" = "true" ] || fail "$kind" "the bytes do not re-encode identically"
    [ "$got_valid" = "true" ] || fail "$kind" "verify_strict rejects the signature"
    [ "$got_account" = "$account" ] || fail "$kind" "account id disagrees"
    [ "$got_tx_id" = "$tx_id" ] || fail "$kind" "tx id disagrees"
    [ "$got_hash" = "$signing_hash" ] || fail "$kind" "signing hash disagrees"
    [ "$got_action" = "$kind" ] || fail "$kind" "decoded as $got_action"
    note "$kind" "parsed, canonical, signature valid"
done

# ---------------------------------------------------------------------------------------
echo
echo "3. the blob conforms to POPCORN-TLOCK-AGE-V1 and decrypts to those bytes"
# ---------------------------------------------------------------------------------------
for index in $(seq 0 $((count - 1))); do
    read -r kind tx_hex blob_hex <<<"$(
        INDEX=$index FIELDS=kind,tx_hex,blob_hex cases_field)"

    verdict=$("$RUST" profile "$blob_hex" "$ROUND" "$CHAIN" 2>&1)
    [ "$verdict" = "OK" ] || fail "$kind" "Rust rejects the blob: $verdict"

    if [ -n "$GO_TOOL" ]; then
        verdict=$("$GO_TOOL" profile "$blob_hex" "$ROUND" "$CHAIN" 2>&1)
        [ "$verdict" = "OK" ] || fail "$kind" "Go rejects the blob: $verdict"
    fi

    plaintext=$("$RUST" decrypt "$blob_hex" "$SIG" 2>/dev/null)
    if [ "$plaintext" != "$tx_hex" ]; then
        fail "$kind" "decryption does not return the signed bytes"
    else
        note "$kind" "conforming, round-trips through Rust"
    fi
done

# ---------------------------------------------------------------------------------------
echo
echo "4. armored output is refused"
# ---------------------------------------------------------------------------------------
# The one JavaScript-specific hazard: tlock-js emits armor, the profile forbids it, and an
# armored blob would give one transaction two blob hashes. If de-armoring ever regresses,
# this is what catches it.
armored_hex=$(node --input-type=module -e '
  const begin = "-----BEGIN AGE ENCRYPTED FILE-----";
  process.stdout.write(Buffer.from(begin + "\n", "utf8").toString("hex"));')
verdict=$("$RUST" profile "$armored_hex" "$ROUND" "$CHAIN" 2>&1)
if [ "$verdict" = "Armored" ]; then
    note "armored blob" "refused, as the profile requires"
else
    fail "armored blob" "expected Armored, got $verdict"
fi

echo
if [ "$failures" -eq 0 ]; then
    echo "browser path: OK"
else
    echo "browser path: $failures failure(s)"
    exit 1
fi
