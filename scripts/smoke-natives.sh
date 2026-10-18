#!/usr/bin/env bash
# Loads the packaged libraries from a clean directory and starts the packaged server.
#   smoke-natives.sh <native-dir> [--recognize]
set -euo pipefail
python="$(command -v python3 || command -v python)"
here="$(cd "$(dirname "$0")" && pwd)"
dir="$(cd "${1:?native-dir}" && pwd)"; mode="${2:-}"
case "$(uname -s)" in
  Linux) lib="$(ls "$dir"/lib*dengjen_stt.so)" ;;
  Darwin) lib="$(ls "$dir"/lib*dengjen_stt.dylib)" ;;
  *) lib="$(ls "$dir"/*dengjen_stt.dll)" ;;
esac

env -i PATH="$PATH" "$python" -I - "$dir" "$lib" <<'PY'
import ctypes, os, sys
directory, lib = sys.argv[1], sys.argv[2]
if hasattr(os, "add_dll_directory"):
    os.add_dll_directory(directory)
stt = ctypes.CDLL(lib)
stt.libdengjenSttAbiVersion.restype = ctypes.c_uint32
assert stt.libdengjenSttAbiVersion() == 1, "ABI version mismatch"
print("libdengjen-stt loaded from", directory)
PY

server="$dir/dengjen-tts-grpc"
if [ -e "$server.exe" ]; then server="$server.exe"; fi
log="$(mktemp)"
env -i PATH="$PATH" DENGJEN_GRPC_SERVER_PORT=50999 "$server" > "$log" 2>&1 &
pid=$!
trap 'kill "$pid" 2>/dev/null || true; rm -f "$log"' EXIT
for _ in $(seq 1 30); do
  if grep -q DENGJEN_GRPC_LISTENING "$log"; then break; fi
  kill -0 "$pid" 2>/dev/null || { cat "$log" >&2; echo "::error::packaged server exited during startup" >&2; exit 1; }
  sleep 1
done
grep -q DENGJEN_GRPC_LISTENING "$log" || { cat "$log" >&2; echo "::error::packaged server never announced its port" >&2; exit 1; }
echo "server started"

if [ "$mode" = "--recognize" ]; then
  : "${DENGJEN_STT_MODEL_DIR:?}" "${DENGJEN_STT_MODEL_SHA256:?}"
  "$python" -I "$here/smoke_recognize.py" "$dir" "$lib"
fi
