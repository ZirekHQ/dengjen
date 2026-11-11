#!/usr/bin/env bash
# Builds libonnxruntime.so from source for core, in place of fetch-ort.sh's prebuilt.
# F-Droid builds native code from source, so its recipe uses this script.
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
lock="$here/ort-source.lock"
: "${ANDROID_NDK_HOME:?set ANDROID_NDK_HOME to the NDK}"
: "${ANDROID_HOME:?set ANDROID_HOME to the Android SDK}"

pin() { sed -n "s/^$1=//p" "$lock"; }
version="$(pin version)"
commit="$(pin commit)"
[ -n "$version" ] && [ -n "$commit" ] || { echo "$lock needs version= and commit=" >&2; exit 1; }
# Matches the prebuilt (and the AARs' minSdk), so the JNI libraries link the same libc symbols.
api="${ORT_ANDROID_API:-27}"
jobs="${ORT_JOBS:-$(nproc)}"

work="${ORT_WORK_DIR:-}"
if [ -z "$work" ]; then
  work="$(mktemp -d)"
  trap 'rm -rf "$work"' EXIT
fi
src="$work/onnxruntime"
if [ ! -d "$src/.git" ]; then
  git init -q "$src"
  git -C "$src" remote add origin https://github.com/microsoft/onnxruntime.git
fi
git -C "$src" fetch -q --depth 1 origin "$commit"
git -C "$src" -c advice.detachedHead=false checkout -q FETCH_HEAD
[ "$(git -C "$src" rev-parse HEAD)" = "$commit" ] || { echo "checkout is not $commit" >&2; exit 1; }
[ "$(cat "$src/VERSION_NUMBER")" = "$version" ] || { echo "source is $(cat "$src/VERSION_NUMBER"), lock says $version" >&2; exit 1; }

# Cross-compiling needs a host protoc. Build it from the protobuf source ONNX Runtime pins,
# so CMake does not download a prebuilt protoc binary.
protoc="$work/protoc/bin/protoc"
if [ ! -x "$protoc" ]; then
  IFS=';' read -r _ pb_url pb_sha1 < <(grep '^protobuf;' "$src/cmake/deps.txt")
  [ -n "$pb_url" ] && [ -n "$pb_sha1" ] || { echo "no protobuf pin in cmake/deps.txt" >&2; exit 1; }
  curl -fsSL -o "$work/protobuf.zip" "$pb_url"
  echo "$pb_sha1  $work/protobuf.zip" | sha1sum -c --quiet -
  rm -rf "$work/protobuf"
  mkdir -p "$work/protobuf"
  unzip -q "$work/protobuf.zip" -d "$work/protobuf"
  pb_src="$(find "$work/protobuf" -mindepth 1 -maxdepth 1 -type d -print -quit)"
  cmake -S "$pb_src" -B "$work/protobuf-build" -DCMAKE_BUILD_TYPE=Release \
    -Dprotobuf_BUILD_TESTS=OFF -Dprotobuf_BUILD_SHARED_LIBS=OFF -Dprotobuf_WITH_ZLIB=OFF \
    -DCMAKE_INSTALL_PREFIX="$work/protoc" >/dev/null
  cmake --build "$work/protobuf-build" --target protoc -j "$jobs"
  install -D "$work/protobuf-build/protoc" "$protoc"
fi

generator="Unix Makefiles"
command -v ninja >/dev/null && generator=Ninja
tools="$(echo "$ANDROID_NDK_HOME"/toolchains/llvm/prebuilt/*/bin)"
for abi in arm64-v8a x86_64; do
  python3 "$src/tools/ci_build/build.py" \
    --build_dir "$work/build/$abi" --config Release --update --build \
    --android --android_abi "$abi" --android_api "$api" \
    --android_sdk_path "$ANDROID_HOME" --android_ndk_path "$ANDROID_NDK_HOME" \
    --build_shared_lib --skip_tests --skip_submodule_sync --compile_no_warning_as_error \
    --path_to_protoc_exe "$protoc" --cmake_generator "$generator" --parallel "$jobs"
  dest="$here/core/src/main/jniLibs/$abi/libonnxruntime.so"
  install -D "$work/build/$abi/Release/libonnxruntime.so" "$dest"
  "$tools/llvm-strip" --strip-unneeded "$dest"
  # Same shape as the prebuilt: stt's sherpa C API needs this SONAME, and libc++ stays static.
  dynamic="$("$tools/llvm-readelf" -d "$dest")"
  grep -q 'SONAME.*\[libonnxruntime\.so\]' <<<"$dynamic" || { echo "$abi: unexpected SONAME" >&2; exit 1; }
  if grep -q 'NEEDED.*libc++_shared' <<<"$dynamic"; then echo "$abi: links libc++_shared.so" >&2; exit 1; fi
done
