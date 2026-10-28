# dengjen-core

Shared runtime primitives for the dengjen speech engines.

- `CancellationToken`: a cloneable flag for cooperative cancellation. One thread calls `cancel`; synthesis loops poll `is_cancelled` between chunks and stop early. Nothing is interrupted mid-chunk.
- `resolve_within`: joins a relative path onto a base directory and refuses absolute paths, drive prefixes and `..` components, so a config file cannot name a path outside its directory by lexical traversal. Symlinks are not resolved, so a symlink inside the directory can still lead out of it.
- `lock_ignoring_poison`, `read_ignoring_poison`, `write_ignoring_poison`: lock guards that survive a panicking holder; callers must tolerate whatever state it left behind.
- `panic_message`: renders a caught panic payload as text.

With the `ort-providers` feature, the crate also owns the process-wide ONNX Runtime environment: `environment`, `session_builder`, `install_environment` and `execution_providers`. The `cuda`, `directml` and `coreml` features register those accelerators ahead of the CPU provider; with none enabled, ONNX Runtime stays on its CPU provider.
