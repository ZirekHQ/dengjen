# Rust Code Standards & Architecture
- **Error Handling**:
  - Return explicit `Result<T, E>` / `DengjenResult<T>`. No panics or `.unwrap()`/`.expect()` on reachable paths outside `#[cfg(test)]`.
  - Use shared domain error enums; convert explicitly at FFI/C-API boundaries.
  - Do NOT perform bulk unwrap/indexing sweeps (`unwrap_used`, `indexing_slicing` are deferred).
- **Idiomatic Rust**:
  - `impl Trait` > `Box<dyn Trait>` where possible.
  - Avoid unneeded `.clone()` calls or superfluous `#[derive]` attributes.
  - Prefer iterator combinators (`map`, `filter_map`, `fold`) over explicit loops.
- **Unsafe & FFI Policy**:
  - Non-FFI crates must maintain `#![forbid(unsafe_code)]`.
  - Inside `unsafe fn`, scope `unsafe { }` blocks strictly to the minimal raw pointer/FFI operation (no blanket function blocks).
  - Every `unsafe fn` MUST have a `# Safety` doc comment defining caller obligations.
  - Every `unsafe { }` block MUST have a `// SAFETY:` comment immediately above it explaining soundness.
  - Do not use Miri if linked C libraries/FFI are present; rely on AddressSanitizer (`asan`).

# Verification Gates (Run Before Finalizing)
- Clippy: `cargo clippy --workspace --lib --bins -- -D warnings`
- Format: `cargo fmt --all -- --check`
- Tests: `cargo test --workspace --no-fail-fast` (plain `cargo test` stops at the first failing crate and never reaches the rest of the workspace). Needs `espeak-ng` installed or `DENGJEN_ESPEAKNG_DATA_DIRECTORY` pointed at its data dir — see `.github/workflows/rust-lint.yaml`'s `asan` job for the same recipe. `dengjen-tts`'s `test_lazy_stream`/`test_parallel_stream`/`test_realtime_stream` self-skip (print and pass) when real Piper voice fixtures aren't at `crates/dengjen/synth/models/{std,rt}/`, which is the case by default; see `.github/CONTRIBUTING.md#benchmarks`.
- Dependencies & Licenses: `cargo deny check licenses bans sources`
- Review: Run `/code-review` before merging non-trivial changes