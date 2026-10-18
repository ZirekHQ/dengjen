#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
fail=0
check() { if ! eval "$1"; then echo "FAIL: $2" >&2; fail=1; fi; }

mkdir -p "$tmp/bin"
cat > "$tmp/bin/patchelf" <<'SH'
#!/usr/bin/env bash
echo "patchelf $*" >> "$PATCH_LOG"
SH
cat > "$tmp/bin/readelf" <<'SH'
#!/usr/bin/env bash
echo " 0x0000000000000001 (NEEDED)             Shared library: [libonnxruntime.so.1]"
SH
chmod +x "$tmp/bin/patchelf" "$tmp/bin/readelf"
export PATH="$tmp/bin:$PATH" PATCH_LOG="$tmp/patch.log"
: > "$PATCH_LOG"

src="$tmp/src"; mkdir -p "$src"
for f in liblibdengjen_stt.so libsherpa-onnx-c-api.so libonnxruntime.so.1.24.1 dengjen-tts-grpc; do : > "$src/$f"; done
ln -s libonnxruntime.so.1.24.1 "$src/libonnxruntime.so.1"
ln -s libonnxruntime.so.1.24.1 "$src/libonnxruntime.so"

./assemble-natives.sh linux shared "$src" "$tmp/shared"
check '[ -f "$tmp/shared/libonnxruntime.so.1" ] && [ ! -L "$tmp/shared/libonnxruntime.so.1" ]' "shared ships the runtime as one regular file under its needed name"
check '[ "$(ls "$tmp/shared" | grep -c onnxruntime)" -eq 1 ]' "shared ships exactly one runtime file"
check '[ ! -s "$tmp/patch.log" ]' "shared does not patch"

./assemble-natives.sh linux isolated "$src" "$tmp/iso"
check '[ -f "$tmp/iso/libonnxruntime-sherpa.so" ] && [ ! -L "$tmp/iso/libonnxruntime-sherpa.so" ]' "isolated renames the runtime to a regular file"
check '[ "$(ls "$tmp/iso" | grep -c onnxruntime)" -eq 1 ]' "isolated leaves no other runtime file"
check 'grep -q -- "--set-soname libonnxruntime-sherpa.so" "$tmp/patch.log"' "isolated sets the soname"
check 'grep -q -- "--replace-needed libonnxruntime.so.1 libonnxruntime-sherpa.so .*libsherpa-onnx-c-api.so" "$tmp/patch.log"' "isolated rewrites sherpa's needed entry"
check 'grep -q -- "--replace-needed libonnxruntime.so.1 libonnxruntime-sherpa.so .*dengjen_stt.so" "$tmp/patch.log"' "isolated rewrites libdengjen-stt's needed entry"

wsrc="$tmp/wsrc"; mkdir -p "$wsrc"
for f in libdengjen_stt.dll sherpa-onnx-c-api.dll onnxruntime.dll dengjen-tts-grpc.exe; do : > "$wsrc/$f"; done
./assemble-natives.sh windows shared "$wsrc" "$tmp/win"
check '[ -f "$tmp/win/onnxruntime.dll" ]' "windows shared copies the runtime"
if ./assemble-natives.sh windows isolated "$wsrc" "$tmp/winiso" 2>/dev/null; then
  echo "FAIL: windows isolated must fail" >&2; fail=1
fi
nosrc="$tmp/nosrv"; mkdir -p "$nosrc"
for f in liblibdengjen_stt.so libsherpa-onnx-c-api.so libonnxruntime.so.1.24.1; do : > "$nosrc/$f"; done
check './assemble-natives.sh linux shared "$nosrc" "$tmp/nosrv-out" 2>/dev/null' "assembles without the gRPC server binary"
exit "$fail"
