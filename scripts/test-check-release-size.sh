#!/usr/bin/env bash
# Tests scripts/check-release-size.sh against fake staging directories.
set -euo pipefail
script="$(cd "$(dirname "$0")" && pwd)/check-release-size.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
fail() { echo "FAIL: $*" >&2; exit 1; }

mkdir -p "$work/io/x/small/1.0" "$work/io/x/big/2.0"
head -c 300000 /dev/zero > "$work/io/x/small/1.0/small-1.0.jar"
head -c 1200000 /dev/zero > "$work/io/x/big/2.0/big-2.0-linux.jar"
head -c 600000 /dev/zero > "$work/io/x/big/2.0/big-2.0-windows.jar"

out="$(bash "$script" "$work" 3)" || fail "2.1 MB must pass a 3 MB budget"
grep -q '1.80 MB  io/x/big/2.0' <<<"$out" || fail "per-artifact size missing: $out"
grep -q 'total: 2.10 MB' <<<"$out" || fail "total missing: $out"

if bash "$script" "$work" 2 >/dev/null 2>&1; then fail "2.1 MB must fail a 2 MB budget"; fi
if bash "$script" "$work/missing" 2 >/dev/null 2>&1; then fail "a missing directory must fail"; fi
for bad in -1 81 abc 2.5 ""; do
  if bash "$script" "$work" "$bad" >/dev/null 2>&1; then fail "budget '$bad' must be refused"; fi
done
if bash "$script" "$work" 0 >/dev/null 2>&1; then fail "a zero budget must fail any release"; fi
echo "check-release-size: ok"
