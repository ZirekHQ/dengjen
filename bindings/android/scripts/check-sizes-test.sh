#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

make_aar() {
  local module="$1" entry_bytes="$2"
  local stage="$work/stage-$module"
  mkdir -p "$stage/jni/x86_64"
  head -c "$entry_bytes" /dev/zero > "$stage/jni/x86_64/libfoo.so"
  (cd "$stage" && zip -q -r "$work/aars/$module-release.aar" jni)
}

setup() {
  rm -rf "$work/aars" "$work"/stage-*
  mkdir -p "$work/aars"
  make_aar core 1000
  make_aar tts 1000
  make_aar stt 1000
  cat > "$work/budgets.json" <<'JSON'
{
  "core": {"aar": 100000, "entries": {"jni/x86_64/libfoo.so": 2000}},
  "tts": {"aar": 100000, "entries": {"jni/x86_64/libfoo.so": 2000}},
  "stt": {"aar": 100000, "entries": {"jni/x86_64/libfoo.so": 2000}}
}
JSON
}

run() { BUDGETS="$work/budgets.json" "$here/check-sizes.sh" "$work/aars" 2>&1; }

expect_pass() {
  setup
  run >/dev/null || { echo "FAIL: $1 should pass"; exit 1; }
}

expect_fail() {
  local name="$1" needle="$2"
  local out
  if out="$(run)"; then echo "FAIL: $name should fail"; exit 1; fi
  grep -q "$needle" <<<"$out" || { echo "FAIL: $name did not mention '$needle': $out"; exit 1; }
}

expect_pass "within budget"

setup; make_aar tts 5000
expect_fail "entry over budget" "tts jni/x86_64/libfoo.so is"

setup; rm "$work/aars/core-release.aar"
expect_fail "missing aar" "core-release.aar is missing"

setup; sed -i 's#"jni/x86_64/libfoo.so": 2000}},#"jni/x86_64/libbar.so": 2000}},#' "$work/budgets.json"
expect_fail "budgeted entry missing" "has no jni/x86_64/libbar.so"

setup; jq 'del(.stt)' "$work/budgets.json" > "$work/b2.json" && mv "$work/b2.json" "$work/budgets.json"
expect_fail "module without a budget" "stt has no budget"

setup; jq '.tts.entries = {}' "$work/budgets.json" > "$work/b3.json" && mv "$work/b3.json" "$work/budgets.json"
expect_fail "empty entries" "tts has no budget"

setup; jq '.tts.entries["jni/x86_64/libfoo.so"] = "big"' "$work/budgets.json" > "$work/b4.json" && mv "$work/b4.json" "$work/budgets.json"
expect_fail "non-numeric entry limit" "tts has no budget"

setup; echo '{ not json' > "$work/budgets.json"
expect_fail "unparseable budgets" "is not valid JSON"

echo "ok"
