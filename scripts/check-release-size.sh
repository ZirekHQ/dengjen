#!/usr/bin/env bash
# Fails when a staged Maven release is bigger than the budget, before anything is uploaded.
#
#   check-release-size.sh <staging-dir> <budget-mb>
#
# Maven Central caps how much an organization may publish per calendar month (80 MB on the free
# plan); a release over budget would use up the month on its own. Prints each artifact's size so
# a failure shows what grew.
set -euo pipefail
dir="${1:?usage: check-release-size.sh <staging-dir> <budget-mb>}"
budget_mb="${2:?usage: check-release-size.sh <staging-dir> <budget-mb>}"
if ! [[ "$budget_mb" =~ ^[0-9]+$ ]] || [ "$budget_mb" -gt 80 ]; then
  echo "::error::budget '${budget_mb}' must be a whole number of MB from 0 to 80 (Maven Central's monthly limit)" >&2
  exit 1
fi
[ -d "$dir" ] || { echo "::error::no staged release at $dir" >&2; exit 1; }

# One line per artifact version directory (group/artifact/version): bytes, then the path.
sizes="$(find "$dir" -type f -printf '%s %h\n' \
  | awk '{ bytes[$2] += $1 } END { for (d in bytes) print bytes[d], d }' \
  | LC_ALL=C sort -rn)"
total="$(awk '{ t += $1 } END { print t + 0 }' <<<"$sizes")"

mb() { awk -v b="$1" 'BEGIN { printf "%.2f MB", b / 1000000 }'; }
while read -r bytes path; do
  [ -n "$bytes" ] && printf '%12s  %s\n' "$(mb "$bytes")" "${path#"$dir"/}"
done <<<"$sizes"
echo "total: $(mb "$total") (budget ${budget_mb} MB)"

if [ "$total" -gt $((budget_mb * 1000000)) ]; then
  echo "::error::the staged release is $(mb "$total"), over the ${budget_mb} MB budget; Maven Central allows 80 MB per month" >&2
  exit 1
fi
