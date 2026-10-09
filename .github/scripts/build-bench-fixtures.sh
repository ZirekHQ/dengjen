#!/usr/bin/env bash
set -euo pipefail

out="${1:?usage: build-bench-fixtures.sh OUT_DIR}"
piper_rev="${PIPER_REV:?set PIPER_REV to a rhasspy/piper-voices commit}"
kokoro_rev="${KOKORO_REV:?set KOKORO_REV to an onnx-community/Kokoro-82M-ONNX commit}"
voice="${KOKORO_VOICE:-af_bella}"

# crates/dengjen/models/kokoro/src/voice_style.rs: MAX_TOKEN_LEN x STYLE_DIM x f32.
style_rows=510
style_dim=256
voice_bytes=$((style_rows * style_dim * 4))

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$out" "$work/piper" "$work/kokoro/voices"

piper_base="https://huggingface.co/rhasspy/piper-voices/resolve/${piper_rev}/en/en_US/lessac/medium"
curl -fsSL "$piper_base/en_US-lessac-medium.onnx" -o "$work/piper/model.onnx"
curl -fsSL "$piper_base/en_US-lessac-medium.onnx.json" -o "$work/piper/model.onnx.json"

kokoro_base="https://huggingface.co/onnx-community/Kokoro-82M-ONNX/resolve/${kokoro_rev}"
curl -fsSL "$kokoro_base/onnx/model.onnx" -o "$work/kokoro/model.onnx"
curl -fsSL "$kokoro_base/tokenizer.json" -o "$work/kokoro/tokenizer.json"
curl -fsSL "$kokoro_base/voices/${voice}.bin" -o "$work/voice-full.bin"

# Upstream ships 512 style rows; the loader requires exactly the first 510.
head -c "$voice_bytes" "$work/voice-full.bin" > "$work/kokoro/voices/${voice}.bin"
actual="$(stat -c %s "$work/kokoro/voices/${voice}.bin")"
if [ "$actual" -ne "$voice_bytes" ]; then
  echo "voice ${voice}.bin is ${actual} bytes after truncation, expected ${voice_bytes}" >&2
  exit 1
fi

cat > "$work/kokoro/config.json" <<EOF
{
  "model_type": "kokoro",
  "model_path": "model.onnx",
  "voices_dir": "voices",
  "vocab_path": "tokenizer.json",
  "sample_rate": 24000,
  "voices": ["${voice}"]
}
EOF

tar -C "$work/piper" -czf "$out/piper-std.tar.gz" model.onnx model.onnx.json
tar -C "$work/kokoro" -czf "$out/kokoro.tar.gz" config.json model.onnx tokenizer.json voices
(cd "$out" && sha256sum piper-std.tar.gz kokoro.tar.gz)
