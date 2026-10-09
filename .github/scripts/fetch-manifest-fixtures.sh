#!/usr/bin/env bash
set -euo pipefail

manifest="${1:?usage: fetch-manifest-fixtures.sh MANIFEST ROOT}"
root="${2:?root}"

tag="$(jq -r .tag "$manifest")"
jq -r '.fixtures[] | [.asset, .sha256, .dest] | @tsv' "$manifest" |
  while IFS=$'\t' read -r asset sha dest; do
    bash "$(dirname "$0")/fetch-bench-fixture.sh" "$tag" "$asset" "$sha" "$root/$dest"
  done
