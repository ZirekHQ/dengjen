#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
root="$(cd "$here/../.." && pwd)"
profile="${1:-release}"
features="ort-dylib"; out="$here/tts/src/main/jniLibs"; flag="--release"
if [ "$profile" = debug ]; then features="ort-dylib,self-test"; out="$here/tts/src/debug/jniLibs"; flag=""; fi
cd "$root"
cargo ndk -t arm64-v8a -t x86_64 -o "$out" build $flag -p dengjen-capi-jni-tts --no-default-features --features "$features"
# cargo-ndk also copies the libdengjen cdylib; the JNI library already embeds the engine.
rm -f "$out"/*/liblibdengjen.so
# The JNI libraries link libc++_shared.so; core owns it so sibling modules do not collide on the file.
sysroot="$(echo "$ANDROID_NDK_HOME"/toolchains/llvm/prebuilt/*/sysroot/usr/lib)"
strip="$(echo "$ANDROID_NDK_HOME"/toolchains/llvm/prebuilt/*/bin/llvm-strip)"
for pair in arm64-v8a:aarch64-linux-android x86_64:x86_64-linux-android; do
  dest="$here/core/src/main/jniLibs/${pair%%:*}/libc++_shared.so"
  install -D "$sysroot/${pair#*:}/libc++_shared.so" "$dest"
  "$strip" --strip-unneeded "$dest"
done

# The prebuilt libonnxruntime.so needs libc symbols from API 24 (__write_chk), which cargo-ndk's default platform lacks.
stt_out="$here/stt/src/main/jniLibs"
cargo ndk -t arm64-v8a -t x86_64 -P 27 -o "$stt_out" build --release -p dengjen-capi-jni-stt
rm -f "$stt_out"/*/liblibdengjen_stt.so
# libonnxruntime.so and libc++_shared.so belong to core; only the sherpa C API ships with stt.
prebuilt="${CARGO_TARGET_DIR:-$root/target}/sherpa-onnx-prebuilt/jniLibs"
for abi in arm64-v8a x86_64; do
  install -D "$prebuilt/$abi/libsherpa-onnx-c-api.so" "$stt_out/$abi/libsherpa-onnx-c-api.so"
done
