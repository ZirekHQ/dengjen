#!/usr/bin/env bash
# Picks the native layout from the ONNX Runtime bundled with sherpa-onnx.
#   ort-gate.sh --decide <version> <required-api>   print shared|isolated
#   ort-gate.sh <runtime-library> <required-api>    read the version, then decide
set -euo pipefail
python="$(command -v python3 || command -v python)"

decide() {
  local version="$1" required="$2"
  if ! [[ "$version" =~ ^([0-9]+)\.([0-9]+)\.([0-9]+) ]]; then
    echo "ort-gate: malformed ONNX Runtime version '$version'" >&2
    return 2
  fi
  local major="${BASH_REMATCH[1]}" minor="${BASH_REMATCH[2]}"
  if [ "$major" -eq 1 ] && [ "$minor" -ge "$required" ]; then echo shared; else echo isolated; fi
}

case "${1:-}" in
  --decide) decide "${2:?version}" "${3:?required api}" ;;
  "") echo "usage: $0 [--decide <version> | <runtime-library>] <required-api>" >&2; exit 2 ;;
  *) decide "$("$python" -I "$(dirname "$0")/ort_version.py" "$1")" "${2:?required api}" ;;
esac
