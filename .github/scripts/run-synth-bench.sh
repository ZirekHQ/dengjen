#!/usr/bin/env bash
# DIVAN_SAMPLE_SIZE overrides every bench's sample_size, Kokoro's sample_size = 1 included,
# so Kokoro runs in its own invocation. The :-10 fallback matches the bench_group in benchmarks.rs.
set -euo pipefail

out="${1:?output file}"

DIVAN_SAMPLE_SIZE="${PIPER_SAMPLE_SIZE:-10}" \
  cargo bench -p dengjen-tts --bench benchmarks -- --skip bench_realtime --skip kokoro | tee "$out"
cargo bench -p dengjen-tts --bench benchmarks -- kokoro | tee -a "$out"
