#!/usr/bin/env bash
set -euo pipefail
dir="${1:?usage: check-sizes.sh <aar-dir>}"
budgets="${BUDGETS:-$(cd "$(dirname "$0")/.." && pwd)/size-budgets.json}"
status=0
fail() { echo "::error::$*" >&2; status=1; }
jq -e . "$budgets" >/dev/null 2>&1 || { fail "$budgets is not valid JSON"; exit 1; }
for module in core tts stt; do
  if ! jq -e --arg m "$module" '(.[$m].aar | type == "number") and (.[$m].entries | type == "object" and length > 0 and all(.[]; type == "number"))' "$budgets" >/dev/null; then
    fail "$module has no budget in $budgets"
    continue
  fi
  aar="$dir/$module-release.aar"
  [ -f "$aar" ] || { fail "$aar is missing"; continue; }
  size=$(stat -c %s "$aar")
  limit=$(jq -r --arg m "$module" '.[$m].aar' "$budgets")
  [ "$size" -le "$limit" ] || fail "$module AAR is $size bytes, budget $limit"
  listing=$(unzip -l "$aar")
  while IFS=$'\t' read -r entry max; do
    actual=$(awk -v e="$entry" '$4 == e { print $1 }' <<<"$listing")
    if [ -z "$actual" ]; then fail "$module AAR has no $entry"; continue; fi
    [ "$actual" -le "$max" ] || fail "$module $entry is $actual bytes, budget $max"
  done < <(jq -r --arg m "$module" '.[$m].entries | to_entries[] | [.key, .value] | @tsv' "$budgets")
done
exit "$status"
