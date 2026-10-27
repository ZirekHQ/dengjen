#!/usr/bin/env bash
set -euo pipefail
version="${SHERPA_VERSION:-2.0.1}"
here="$(cd "$(dirname "$0")/.." && pwd)"
asset="sherpa-onnx-v${version}-android.tar.bz2"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
curl -fsSL -o "$work/$asset" "https://github.com/k2-fsa/sherpa-onnx/releases/download/v${version}/${asset}"
(cd "$work" && echo "$(cat "$here/ort.sha256")  $asset" | sha256sum -c -)
tar -xjf "$work/$asset" -C "$work"
for abi in arm64-v8a x86_64; do
  src="$(find "$work" -path "*/${abi}/libonnxruntime.so" -print -quit)"
  [ -n "$src" ] || { echo "missing ${abi}/libonnxruntime.so" >&2; exit 1; }
  install -D "$src" "$here/core/src/main/jniLibs/${abi}/libonnxruntime.so"
done
