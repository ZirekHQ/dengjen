#!/usr/bin/env bash
# Usage: smoke-xcframework.sh <Libdengjen.xcframework>
# Imports the module through its modulemap, links the macOS slice and runs it.
set -euo pipefail
fw="$(cd "$1" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
slice="$(find "$fw" -maxdepth 1 -type d -name 'macos-*' | head -n1)"
[ -n "$slice" ] || { echo "no macOS slice in $fw" >&2; exit 1; }
cat > "$work/main.m" <<'SRC'
@import Libdengjen;
#include <stdio.h>
int main(void) { printf("abi %u\n", libdengjenAbiVersion()); return 0; }
SRC
clang -fmodules -fobjc-arc -I "$slice/Headers" "$work/main.m" "$slice"/*.a \
  -lc++ -framework Foundation -framework CoreML -framework Accelerate -o "$work/smoke"
"$work/smoke" | grep -E '^abi [0-9]+$'
