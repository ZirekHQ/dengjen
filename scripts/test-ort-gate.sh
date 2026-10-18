#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
gate=./ort-gate.sh
fail=0
expect() {
  local want="$1"; shift
  local got
  got="$("$gate" --decide "$@")"
  if [ "$got" != "$want" ]; then echo "FAIL: --decide $* -> $got, want $want" >&2; fail=1; fi
}
expect shared 1.27.0 27
expect shared 1.28.1 27
expect isolated 1.26.9 27
expect isolated 1.23.2 27
expect isolated 0.9.0 27
if "$gate" --decide nonsense 27 2>/dev/null; then echo "FAIL: accepted a malformed version" >&2; fail=1; fi
exit "$fail"
