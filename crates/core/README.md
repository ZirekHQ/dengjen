# dengjen-core

Shared runtime primitives for the dengjen speech engines.

- `CancellationToken`: a cloneable flag that stops a running synthesis from another thread.
- `resolve_within`: joins a relative path onto a base directory and refuses absolute paths, drive prefixes and `..` components, so a config file cannot name a path outside its directory. Symlinks are not resolved.
- `lock_ignoring_poison`, `read_ignoring_poison`, `write_ignoring_poison`: lock guards that survive a panicking holder; callers must tolerate whatever state it left behind.
- `panic_message`: renders a caught panic payload as text.

With the `ort-providers` feature, the crate also owns the process-wide ONNX Runtime environment: `environment`, `session_builder`, `install_environment` and `execution_providers`. The `cuda`, `directml` and `coreml` features register those accelerators ahead of the CPU provider; with none enabled, ONNX Runtime stays on its CPU provider.
