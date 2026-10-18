#!/usr/bin/env bash
# Verifies a native directory resolves its own dependencies without the build tree.
#   check-imported-libs.sh <linux|macos|windows> <native-dir>
set -euo pipefail
os="${1:?os}"; dir="${2:?native-dir}"
fail=0

needed_linux()   { readelf -d "$1" | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p'; }
runpath_linux()  { readelf -d "$1" | sed -n 's/.*(\(RUNPATH\|RPATH\)).*\[\(.*\)\]/\2/p'; }
needed_macos()   { otool -L "$1" | tail -n +2 | awk '{print $1}' | sed 's#.*/##'; }
runpath_macos()  { otool -l "$1" | awk '/LC_RPATH/{f=1} f&&/path /{print $2; f=0}'; }
needed_windows() { llvm-objdump -p "$1" | sed -n 's/^[[:space:]]*DLL Name:[[:space:]]*//p'; }

audit() {
  local file="$1" needed
  needed="$("needed_$os" "$file")"
  echo "== $file"; echo "$needed"
  if [ "$os" != windows ]; then
    case "$("runpath_$os" "$file")" in
      *'$ORIGIN'*|*'@loader_path'*) ;;
      *) echo "::error::$file has no \$ORIGIN or @loader_path rpath" >&2; fail=1 ;;
    esac
  fi
  while IFS= read -r dep; do
    case "$dep" in
      *sherpa-onnx-c-api*|*onnxruntime*)
        [ -e "$dir/$dep" ] || { echo "::error::$file needs $dep, missing from $dir" >&2; fail=1; } ;;
    esac
  done <<< "$needed"
}

for f in "$dir"/*dengjen_stt.* "$dir"/dengjen-tts-grpc*; do
  if [ -f "$f" ]; then audit "$f"; fi
done
exit "$fail"
