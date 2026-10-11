#!/usr/bin/env bash
set -euo pipefail

manifest="${1:?usage: fetch-manifest-fixtures.sh MANIFEST ROOT [NAME]}"
root="${2:?root}"
name="${3:-}"

tag="$(jq -r .tag "$manifest")"
rows="$(jq -r --arg name "$name" '.fixtures[] | select($name == "" or .name == $name) | [.asset, .sha256, .dest] | @tsv' "$manifest")"
if [ -z "$rows" ]; then
  echo "no fixture matching '${name}' in ${manifest}" >&2
  exit 1
fi
while IFS=$'\t' read -r asset sha dest; do
  bash "$(dirname "$0")/fetch-bench-fixture.sh" "$tag" "$asset" "$sha" "$root/$dest"
done <<< "$rows"
