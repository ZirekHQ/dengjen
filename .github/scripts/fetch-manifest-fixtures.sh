#!/usr/bin/env bash
set -euo pipefail

manifest="${1:?usage: fetch-manifest-fixtures.sh MANIFEST ROOT [NAME]}"
root="${2:?root}"
name="${3:-}"

tag="$(jq -r .tag "$manifest")"
jq -r --arg name "$name" '.fixtures[] | select($name == "" or .name == $name) | [.asset, .sha256, .dest] | @tsv' "$manifest" |
  while IFS=$'\t' read -r asset sha dest; do
    bash "$(dirname "$0")/fetch-bench-fixture.sh" "$tag" "$asset" "$sha" "$root/$dest"
  done
