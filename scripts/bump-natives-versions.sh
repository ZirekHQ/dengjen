#!/usr/bin/env bash
# Refreshes both Java natives fingerprints and bumps the patch version of each kind whose
# fingerprint differs from <base-ref> and whose version is still the base's.
set -euo pipefail
base="${1:?usage: bump-natives-versions.sh <base-ref>}"
cd "$(git rev-parse --show-toplevel)"
gradle=bindings/java/build.gradle.kts

read_version() { grep -m1 "^val $1 = " | sed -E 's/.*"([^"]+)".*/\1/'; }

refresh() {
  local kind="$1" record="$2" key="$3" current base_version next
  NATIVES_KIND="$kind" scripts/natives-fingerprint.sh --update >/dev/null
  if [ "$(git show "$base:$record" | tr -d '[:space:]')" = "$(tr -d '[:space:]' < "$record")" ]; then
    return 0
  fi
  current="$(read_version "$key" < "$gradle")"
  base_version="$(git show "$base:$gradle" | read_version "$key")"
  [ "$current" = "$base_version" ] || return 0
  next="$(awk -F. '{ printf "%d.%d.%d", $1, $2, $3 + 1 }' <<<"$current")"
  sed -i -E "s/^(val $key = \")[^\"]+(\")/\1$next\2/" "$gradle"
  echo "$key $current -> $next"
}

refresh tts bindings/java/natives.fingerprint nativesVersion
refresh stt bindings/java/stt-natives.fingerprint sttNativesVersion
