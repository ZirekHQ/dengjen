#!/usr/bin/env bash
# Usage: build-xcframework.sh [debug|release]
# Prebuilt ort covers only arm64 for iOS, the iOS simulator and macOS, so there are no x86_64 slices.
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
root="$(cd "$here/../.." && pwd)"
profile="${1:-release}"
flag="--release"; [ "$profile" = debug ] && { flag=""; profile=debug; }
targets=(aarch64-apple-ios aarch64-apple-ios-sim aarch64-apple-darwin)
out="${APPLE_OUT_DIR:-$root/target/apple}"
name="Libdengjen"
cd "$root"
for target in "${targets[@]}"; do
  cargo build --locked $flag -p libdengjen --lib --no-default-features --features espeak,coreml --target "$target"
done
rm -rf "$out/headers" "$out/$name.xcframework"
mkdir -p "$out/headers"
cp crates/frontends/capi/libdengjen.h "$out/headers/"
cp "$here/module.modulemap" "$out/headers/"
args=()
for target in "${targets[@]}"; do
  args+=(-library "target/$target/$profile/liblibdengjen.a" -headers "$out/headers")
done
xcodebuild -create-xcframework "${args[@]}" -output "$out/$name.xcframework"
(cd "$out" && ditto -c -k --sequesterRsrc --keepParent "$name.xcframework" "$name.xcframework.zip")
(cd "$out" && shasum -a 256 "$name.xcframework.zip" > "$name.xcframework.zip.sha256")
echo "$out/$name.xcframework.zip"
