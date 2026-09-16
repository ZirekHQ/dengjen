# eSpeak NG Sys Bindings

Raw, unsafe FFI bindings to espeak-ng, generated with `bindgen` from `wrapper.h`. `build.rs` builds the vendored espeak-ng sources via CMake and statically links `espeak-ng`, `ucd`, and `speechPlayer`; the vendored copy is a git submodule pinned to espeak-ng 1.52 (commit `724808c`). Consumed by `dengjen-espeak-rs`, which wraps these bindings in a safe API.

## Dependencies

- [espeak-ng](https://github.com/espeak-ng/espeak-ng)
