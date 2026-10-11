#!/usr/bin/env bash
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
base="github.com/ZirekHQ/dengjen-tts-go"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

assert_path() {
  local version="$1" expected="$2" actual
  actual="$("$here/go-module-path.sh" "$version")"
  [[ "$actual" == "$expected" ]] || { echo "${version}: expected ${expected}, got ${actual}" >&2; exit 1; }
}

assert_resolves() {
  local version="$1" module_path repo scratch
  module_path="$("$here/go-module-path.sh" "$version")"
  repo="$work/mirror-${version}"
  scratch="$work/consumer-${version}"

  git init -q "$repo"
  printf 'module %s\n\ngo 1.22\n' "$module_path" > "$repo/go.mod"
  printf 'package dengjen\n' > "$repo/doc.go"
  git -C "$repo" add -A
  git -C "$repo" -c user.name=t -c user.email=t@t -c commit.gpgSign=false commit -q -m release
  git -C "$repo" -c tag.gpgSign=false tag "$version"

  mkdir "$scratch"
  (
    cd "$scratch"
    go mod init verify >/dev/null 2>&1
    GOPROXY=direct GONOSUMDB="$base" GOFLAGS=-mod=mod GIT_CONFIG_COUNT=1 \
      GIT_CONFIG_KEY_0="url.file://${repo}.insteadOf" GIT_CONFIG_VALUE_0="https://${base}" \
      go get "${module_path}@${version}" >/dev/null
  )
}

assert_path v0.1.0 "$base"
assert_path v1.2.3 "$base"
assert_path v2.0.1 "${base}/v2"
assert_path v10.0.0 "${base}/v10"
assert_path v18446744073709551616.0.0 "${base}/v18446744073709551616"

for invalid in 2.0.1 v2.0.0-rc.1 v2.0; do
  if "$here/go-module-path.sh" "$invalid" 2>/dev/null; then
    echo "${invalid} must be rejected" >&2
    exit 1
  fi
done

for version in "$@"; do
  assert_resolves "$version"
done
echo "go module path checks passed"
