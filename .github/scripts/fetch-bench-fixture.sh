#!/usr/bin/env bash
set -euo pipefail

tag="${1:?usage: fetch-bench-fixture.sh TAG ASSET SHA256 DEST}"
asset="${2:?asset}"
sha="${3:?sha256}"
dest="${4:?dest}"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

gh release download "$tag" --pattern "$asset" --dir "$tmp" --repo "${GITHUB_REPOSITORY:?}"
echo "${sha}  ${tmp}/${asset}" | sha256sum -c -
mkdir -p "$dest"
tar -xzf "${tmp}/${asset}" -C "$dest"
