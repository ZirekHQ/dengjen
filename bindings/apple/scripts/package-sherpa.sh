#!/usr/bin/env bash
# Usage: package-sherpa.sh <cargo-target-dir> <out-dir> <version>
# Zips the prebuilt sherpa-onnx libraries that sherpa-onnx-sys downloaded for the stt build.
set -euo pipefail
target_dir="${1:?usage: package-sherpa.sh <cargo-target-dir> <out-dir> <version>}"
out="${2:?usage: package-sherpa.sh <cargo-target-dir> <out-dir> <version>}"
version="${3:?usage: package-sherpa.sh <cargo-target-dir> <out-dir> <version>}"
prebuilt="$target_dir/sherpa-onnx-prebuilt"
ios="$(find "$prebuilt" -maxdepth 2 -type d -name SherpaOnnxC.xcframework | head -n1)"
macos="$(find "$prebuilt" -maxdepth 2 -type d -path '*osx-arm64-shared-lib/lib' | head -n1)"
[ -n "$ios" ] || { echo "::error::SherpaOnnxC.xcframework not found under $prebuilt" >&2; exit 1; }
[ -n "$macos" ] || { echo "::error::osx-arm64-shared-lib/lib not found under $prebuilt" >&2; exit 1; }
mkdir -p "$out"
ditto -c -k --sequesterRsrc --keepParent "$ios" "$out/dengjen-apple-sherpa-onnx-ios-$version.xcframework.zip"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
mkdir "$stage/sherpa-onnx-macos-arm64"
cp -a "$macos"/*.dylib "$stage/sherpa-onnx-macos-arm64/"
ditto -c -k --sequesterRsrc --keepParent "$stage/sherpa-onnx-macos-arm64" "$out/dengjen-apple-sherpa-onnx-macos-$version.zip"
ls -l "$out"
