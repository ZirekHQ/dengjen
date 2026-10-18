#!/usr/bin/env bash
# Assembles the flat native directory every release consumer copies from.
#   assemble-natives.sh <linux|macos|windows> <shared|isolated> <src-dir> <out-dir>
# Symlink chains are flattened: the ONNX Runtime ships once, as a regular file under the name
# libsherpa-onnx-c-api asks the loader for.
set -euo pipefail
os="${1:?os}"; layout="${2:?layout}"; src="${3:?src-dir}"; out="${4:?out-dir}"

case "$os" in
  linux)   plain=("lib*dengjen_stt.so" "libsherpa-onnx-c-api.so" "dengjen-tts-grpc"); runtime_glob="libonnxruntime.so*" ;;
  macos)   plain=("lib*dengjen_stt.dylib" "libsherpa-onnx-c-api.dylib" "dengjen-tts-grpc"); runtime_glob="libonnxruntime*.dylib" ;;
  windows) plain=("*dengjen_stt.dll" "sherpa-onnx-c-api.dll" "dengjen-tts-grpc.exe"); runtime_glob="onnxruntime.dll" ;;
  *) echo "assemble-natives: unknown os '$os'" >&2; exit 2 ;;
esac

if [ "$os" = windows ] && [ "$layout" = isolated ]; then
  echo "assemble-natives: Windows cannot isolate two onnxruntime.dll copies; align the ort and sherpa-onnx ONNX Runtime versions" >&2
  exit 1
fi

shopt -s nullglob
mkdir -p "$out"

for glob in "${plain[@]}"; do
  matches=()
  for candidate in "$src"/$glob; do
    if [ -e "$candidate" ]; then matches+=("$candidate"); fi
  done
  if [ "${#matches[@]}" -eq 0 ]; then
    case "$glob" in
      dengjen-tts-grpc*) continue ;;
      *) echo "assemble-natives: nothing matched '$glob' in $src" >&2; exit 1 ;;
    esac
  fi
  cp -L "${matches[@]}" "$out/"
done

runtimes=("$src"/$runtime_glob)
if [ "${#runtimes[@]}" -eq 0 ]; then
  echo "assemble-natives: no ONNX Runtime matched '$runtime_glob' in $src" >&2
  exit 1
fi

needed_linux() { readelf -d "$1" | sed -n 's/.*(NEEDED).*\[\(libonnxruntime[^]]*\)\].*/\1/p' | head -n1; }
needed_macos() { otool -L "$1" | awk 'NR > 1 {print $1}' | grep -m1 onnxruntime || true; }

# The name sherpa-onnx's loader request resolves to; macOS keeps the whole install path for -change.
runtime_request=onnxruntime.dll
if [ "$os" != windows ]; then
  runtime_request="$("needed_$os" "$out/$(basename "${out}"/libsherpa-onnx-c-api.*)")"
  if [ -z "$runtime_request" ]; then
    echo "assemble-natives: libsherpa-onnx-c-api does not name an ONNX Runtime" >&2
    exit 1
  fi
fi
runtime_name="$(basename "$runtime_request")"
cp -L "${runtimes[0]}" "$out/$runtime_name"

isolate_linux() {
  local renamed=libonnxruntime-sherpa.so lib
  mv "$out/$runtime_name" "$out/$renamed"
  patchelf --set-soname "$renamed" "$out/$renamed"
  for lib in "$out"/libsherpa-onnx-c-api.so "$out"/lib*dengjen_stt.so; do
    patchelf --replace-needed "$runtime_name" "$renamed" "$lib"
  done
}

# install_name_tool invalidates the arm64 code signature, which dyld enforces.
isolate_macos() {
  local renamed=libonnxruntime-sherpa.dylib lib
  mv "$out/$runtime_name" "$out/$renamed"
  install_name_tool -id "@rpath/$renamed" "$out/$renamed"
  for lib in "$out"/libsherpa-onnx-c-api.dylib "$out"/lib*dengjen_stt.dylib; do
    install_name_tool -change "$runtime_request" "@rpath/$renamed" "$lib"
    codesign --force -s - "$lib"
  done
  codesign --force -s - "$out/$renamed"
}

if [ "$layout" = isolated ]; then
  "isolate_$os"
fi
