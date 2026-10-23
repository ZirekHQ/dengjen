#!/usr/bin/env bash
# Usage: check-sizes.sh <Libdengjen.xcframework.zip>
set -euo pipefail
zip="${1:?usage: check-sizes.sh <xcframework-zip>}"
budgets="${BUDGETS:-$(cd "$(dirname "$0")/.." && pwd)/size-budgets.json}"
status=0
fail() { echo "::error::$*" >&2; status=1; }
jq -e '(.zip | type == "number") and (.entries | type == "object" and length > 0 and all(.[]; type == "number"))' "$budgets" >/dev/null 2>&1 \
  || { fail "$budgets is not a valid size budget"; exit 1; }
[ -f "$zip" ] || { fail "$zip is missing"; exit 1; }
size=$(wc -c < "$zip")
limit=$(jq -r .zip "$budgets")
[ "$size" -le "$limit" ] || fail "XCFramework zip is $size bytes, budget $limit"
listing=$(unzip -l "$zip")
while IFS=$'\t' read -r entry max; do
  actual=$(awk -v e="$entry" '$4 == e { print $1 }' <<<"$listing")
  if [ -z "$actual" ]; then fail "zip has no $entry"; continue; fi
  [ "$actual" -le "$max" ] || fail "$entry is $actual bytes, budget $max"
done < <(jq -r '.entries | to_entries[] | [.key, .value] | @tsv' "$budgets")
exit "$status"
