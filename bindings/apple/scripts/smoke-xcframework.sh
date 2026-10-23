#!/usr/bin/env bash
# Usage: smoke-xcframework.sh <Libdengjen.xcframework> [stt]
# Imports the modules through the modulemap, links the macOS slice and runs it. With `stt`, also
# imports LibdengjenStt and links the sherpa-onnx libraries found under SHERPA_LIB_DIR.
set -euo pipefail
fw="$(cd "$1" && pwd)"
variant="${2:-}"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
slice="$(find "$fw" -maxdepth 1 -type d -name 'macos-*' | head -n1)"
[ -n "$slice" ] || { echo "no macOS slice in $fw" >&2; exit 1; }
cat > "$work/main.m" <<'SRC'
@import Libdengjen;
#ifdef WITH_STT
@import LibdengjenStt;
#endif
#include <stdio.h>
int main(void) {
  printf("abi %u\n", libdengjenAbiVersion());
#ifdef WITH_STT
  printf("stt abi %u\n", libdengjenSttAbiVersion());
#endif
  return 0;
}
SRC
stt_flags=()
if [ "$variant" = stt ]; then
  : "${SHERPA_LIB_DIR:?SHERPA_LIB_DIR must point at the sherpa-onnx libraries}"
  stt_flags=(-DWITH_STT -L "$SHERPA_LIB_DIR" -lsherpa-onnx-c-api -Wl,-rpath,"$SHERPA_LIB_DIR")
fi
clang -fmodules -fobjc-arc -I "$slice/Headers" "${stt_flags[@]}" "$work/main.m" "$slice"/*.a \
  -lc++ -framework Foundation -framework CoreML -framework Accelerate -o "$work/smoke"
"$work/smoke" | grep -E '^abi [0-9]+$'
[ "$variant" != stt ] || "$work/smoke" | grep -E '^stt abi [0-9]+$'
