#!/usr/bin/env bash
# Structural gates from SPEC.md that a test cannot express, because they are about what the
# source may contain rather than about what it computes.
#
#   1. §2.3 — zero unordered collections anywhere near a commitment. This is an architectural
#      policy, independent of the borsh/hashbrown version: RUSTSEC-2024-0402 was one instance
#      of non-canonical encoding, not the whole risk.
#   2. The naming rule — the chain name enters signed preimages, so any ARENA occurrence is
#      stale by construction.
#
# Run from the repository root.
set -euo pipefail

status=0
fail() { echo "FAIL: $*" >&2; status=1; }
pass() { echo "ok: $*"; }

# ---------------------------------------------------------------------------------------
# 1. No unordered collections in the consensus crate
# ---------------------------------------------------------------------------------------
forbidden=$(grep -rnE '\b(HashMap|HashSet)\b' crates/popcorn-core/src || true)
if [ -n "$forbidden" ]; then
    fail "unordered collections in the consensus crate (SPEC.md §2.3):"
    echo "$forbidden" >&2
else
    pass "no HashMap/HashSet in popcorn-core/src"
fi

# Unordered iteration is just as fatal as unordered storage: a values() over a HashMap would
# be caught above, but an explicit sort-free iteration of anything else should be reviewed.
loose=$(grep -rnE 'iter\(\)\.collect::<(HashSet|HashMap)' crates/ || true)
if [ -n "$loose" ]; then
    fail "collection into an unordered container:"
    echo "$loose" >&2
else
    pass "no collection into unordered containers"
fi

# ---------------------------------------------------------------------------------------
# 2. The old chain name is stale by construction
# ---------------------------------------------------------------------------------------
# SPEC.md and CHANGELOG.md carry the rename note itself, which is the one legitimate mention.
# This script names the old identifier in order to hunt it, so it is excluded from its own
# scan; SPEC.md and CHANGELOG.md carry the rename note, which is the legitimate mention.
stale=$(grep -rniE 'arena[-_ ]?chain|\bARENA\b' \
          --include='*.rs' --include='*.toml' --include='*.sh' --include='*.yml' \
          --exclude='consensus-gates.sh' \
          crates/ ci/ .github/ Cargo.toml 2>/dev/null || true)
if [ -n "$stale" ]; then
    fail "stale ARENA references (SPEC.md, naming):"
    echo "$stale" >&2
else
    pass "no stale ARENA references in code"
fi

# ---------------------------------------------------------------------------------------
# 3. Consensus-relevant dependencies are pinned exactly
# ---------------------------------------------------------------------------------------
for crate in borsh ed25519-dalek primitive-types blake3 sha2 tlock tlock_age drand_core; do
    line=$(grep -E "^${crate} = " Cargo.toml || true)
    if [ -z "$line" ]; then
        fail "$crate is not declared in the workspace manifest"
    elif ! echo "$line" | grep -q '"='; then
        fail "$crate is not pinned with '=' (SPEC.md §13, CONSENSUS-LOCK.md): $line"
    else
        pass "$crate is pinned exactly"
    fi
done

# ---------------------------------------------------------------------------------------
# 4. The committed vector is present: §10 makes it a gate, not a convenience
# ---------------------------------------------------------------------------------------
if [ -f vectors/end_to_end.json ]; then
    pass "end-to-end vector is committed"
else
    fail "vectors/end_to_end.json is missing (SPEC.md §10)"
fi

exit $status
