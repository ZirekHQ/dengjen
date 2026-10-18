#!/usr/bin/env bash
# Copies the shared libraries pydengjen's `stt` feature loads into the package, so maturin ships
# them in the wheel beside the extension module (its rpath and DLL directory point there).
#   stage-python-stt-libs.sh <native-dir>
set -euo pipefail
native="${1:?native-dir}"
package="$(cd "$(dirname "$0")/.." && pwd)/crates/frontends/python/python/pydengjen"

stage_family() {
  local name="$1" pattern="$2" count=0 f
  for f in "$native"/$pattern; do
    if [ -e "$f" ]; then cp -P "$f" "$package/"; count=$((count + 1)); fi
  done
  if [ "$count" -eq 0 ]; then
    echo "stage-python-stt-libs: no $name library in $native" >&2
    exit 1
  fi
  echo "staged $count $name librar(ies) into $package"
}

stage_family sherpa-onnx-c-api '*sherpa-onnx-c-api*'
stage_family onnxruntime '*onnxruntime*'
