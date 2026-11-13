#!/usr/bin/env bash
# Fingerprints everything that determines the Java native libraries: the sources of the crates
# the selected natives kind links (workspace version lines stripped, so a version bump alone is not
# a native change) and the third-party crates the published build links.
#
# The root Cargo.toml counts only for the sections that change how code is built ([workspace.package]
# and [profile.*]); its member list and [workspace.dependencies] would make every new workspace
# crate republish ~50 MB of unchanged natives, and the dependencies that matter are already covered
# by `cargo tree`, which also shows the features crates/workspace-hack turns on.
#
#   natives-fingerprint.sh            print the fingerprint
#   natives-fingerprint.sh --update   rewrite the kind's record file
#   natives-fingerprint.sh --check    fail when the committed fingerprint is stale
#   natives-fingerprint.sh --check-bump <base-ref>
#                                     fail when the fingerprint changed but the kind's version did not
#
# NATIVES_KIND=tts (default) uses natives.fingerprint and nativesVersion; NATIVES_KIND=stt uses
# stt-natives.fingerprint and sttNativesVersion (dengjen-stt-natives).
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

gradle=bindings/java/build.gradle.kts
case "${NATIVES_KIND:-tts}" in
  tts)
    record=bindings/java/natives.fingerprint
    source_roots="crates/core crates/dengjen crates/text crates/audio crates/frontends/capi"
    tree_args=(-p libdengjen --features ort-dylib)
    extra_files=""
    version_key=nativesVersion ;;
  stt)
    record=bindings/java/stt-natives.fingerprint
    source_roots="crates/stt crates/frontends/capi-stt"
    extra_files="scripts/assemble-natives.sh scripts/ort-gate.sh scripts/ort_version.py"
    tree_args=(-p libdengjen-stt)
    version_key=sttNativesVersion ;;
  *) echo "unknown NATIVES_KIND '${NATIVES_KIND}'" >&2; exit 2 ;;
esac

# The [workspace.package] and [profile.*] tables of a manifest, version lines stripped.
build_sections() {
  awk '/^\[/ { keep = ($0 == "[workspace.package]" || $0 ~ /^\[profile\./) } keep' "$1" \
    | sed -E 's/version = "[^"]*"//'
}

sources() {
  printf '== Cargo.toml (build sections)\n'
  build_sections Cargo.toml
  # shellcheck disable=SC2086
  {
    git ls-files rust-toolchain.toml .cargo $source_roots \
      | grep -E '(\.(rs|toml|json|c|h)|/config)$' \
      | grep -vE '/(tests|benches|examples)/'
    for file in $extra_files; do echo "$file"; done
  } \
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
  cargo tree --color never --locked "${tree_args[@]}" --target all -e normal,build,features \
    --prefix none --no-dedupe \
    | grep -v '(/' | LC_ALL=C sort -u
}

fingerprint() {
  { sources; printf '== dependencies\n'; dependencies; } | sha256sum | cut -d' ' -f1
}

# Printed on a failed check so a CI mismatch can be compared section by section with a local run.
diagnose() {
  echo "sources:      $(sources | sha256sum | cut -d' ' -f1)" >&2
  echo "dependencies: $(dependencies | sha256sum | cut -d' ' -f1)" >&2
}

nativesVersion() { grep -m1 "^val ${version_key} = "; }

case "${1:-}" in
  "") fingerprint ;;
  --update) fingerprint > "$record"; echo "updated $record" ;;
  --check)
    if [ "$(fingerprint)" != "$(tr -d '[:space:]' < "$record")" ]; then
      echo "::error::the native inputs changed: run NATIVES_KIND=${NATIVES_KIND:-tts} scripts/natives-fingerprint.sh --update and bump ${version_key} in ${gradle}" >&2
      diagnose
      exit 1
    fi
    ;;
  --check-bump)
    base="${2:?usage: $0 --check-bump <base-ref>}"
    if [ "$(git show "${base}:${record}" 2>/dev/null | tr -d '[:space:]')" != "$(tr -d '[:space:]' < "$record")" ] \
      && [ "$(git show "${base}:${gradle}" | nativesVersion)" = "$(nativesVersion < "$gradle")" ]; then
      echo "::error::${record} changed but ${version_key} did not; bump ${version_key} in ${gradle}" >&2
      exit 1
    fi
    ;;
  *) echo "usage: $0 [--update|--check|--check-bump <base-ref>]" >&2; exit 2 ;;
esac
