#!/usr/bin/env bash
# Fingerprints everything that determines the Java native libraries: the sources of the crates
# libdengjen links (workspace version lines stripped, so a version bump alone is not a native
# change) and the third-party crates the published build links.
#
#   natives-fingerprint.sh            print the fingerprint
#   natives-fingerprint.sh --update   rewrite bindings/java/natives.fingerprint
#   natives-fingerprint.sh --check    fail when the committed fingerprint is stale
#   natives-fingerprint.sh --check-bump <base-ref>
#                                     fail when the fingerprint changed but nativesVersion did not
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

record=bindings/java/natives.fingerprint
gradle=bindings/java/build.gradle.kts

sources() {
  git ls-files Cargo.toml rust-toolchain.toml .cargo crates/dengjen crates/text crates/audio \
      crates/frontends/capi crates/workspace-hack \
    | grep -E '(\.(rs|toml|json|c|h)|/config)$' \
    | grep -vE '/(tests|benches|examples)/' \
    | LC_ALL=C sort \
    | while read -r file; do
        printf '== %s\n' "$file"
        case "$file" in
          *.toml) sed -E 's/version = "[^"]*"//' "$file" ;;
          *) cat "$file" ;;
        esac
      done
}

dependencies() {
  cargo tree --locked -p libdengjen --features ort-dylib --target all -e normal,build,features \
    --prefix none --no-dedupe \
    | grep -v '(/' | LC_ALL=C sort -u
}

fingerprint() {
  { sources; printf '== dependencies\n'; dependencies; } | sha256sum | cut -d' ' -f1
}

# Printed on a failed check so a CI mismatch can be compared with a local run: the dependency
# lines of the call that failed, and whether a second call returns the same lines.
diagnose() {
  local first="$1" second
  second="$(dependencies)"
  echo "sources:      $(sources | sha256sum | cut -d' ' -f1)" >&2
  echo "dependencies: first $(printf '%s\n' "$first" | sha256sum | cut -c1-16) second $(printf '%s\n' "$second" | sha256sum | cut -c1-16) ($(printf '%s\n' "$first" | wc -l) lines)" >&2
  printf '%s\n' "$first" | sed 's/^/d1| /' >&2
  diff <(printf '%s\n' "$first") <(printf '%s\n' "$second") | sed 's/^/d1-vs-d2| /' >&2 || true
}

nativesVersion() { grep -m1 '^val nativesVersion = '; }

case "${1:-}" in
  "") fingerprint ;;
  --update) fingerprint > "$record"; echo "updated $record" ;;
  --check)
    first="$(dependencies)"
    current="$({ sources; printf '== dependencies\n'; printf '%s\n' "$first"; } | sha256sum | cut -d' ' -f1)"
    if [ "$current" != "$(tr -d '[:space:]' < "$record")" ]; then
      echo "::error::the native inputs changed: run scripts/natives-fingerprint.sh --update and bump nativesVersion in ${gradle}" >&2
      diagnose "$first"
      exit 1
    fi
    ;;
  --check-bump)
    base="${2:?usage: $0 --check-bump <base-ref>}"
    if [ "$(git show "${base}:${record}" 2>/dev/null | tr -d '[:space:]')" != "$(tr -d '[:space:]' < "$record")" ] \
      && [ "$(git show "${base}:${gradle}" | nativesVersion)" = "$(nativesVersion < "$gradle")" ]; then
      echo "::error::natives.fingerprint changed but nativesVersion did not; bump nativesVersion in ${gradle}" >&2
      exit 1
    fi
    ;;
  *) echo "usage: $0 [--update|--check|--check-bump <base-ref>]" >&2; exit 2 ;;
esac
