# eSpeak NG Sys Bindings

Raw, unsafe FFI bindings to espeak-ng, generated with `bindgen` from `wrapper.h`. `build.rs` downloads the espeak-ng commit pinned in `espeak-ng.lock`, checks its sha256, builds it via CMake and statically links `espeak-ng`, `ucd`, and `speechPlayer`. Consumed by `dengjen-espeak-rs`, which wraps these bindings in a safe API.

The crate ships no espeak-ng source, because the language data alone exceeds crates.io's 10 MiB limit for a published crate. The pin is an upstream commit, not a release: the latest release (1.52.0) predates fixes this project needs, such as Kurmanji word stress.

## Building offline

Set `ESPEAK_NG_SOURCE` to an espeak-ng checkout or a `.tar.gz` of one (GitHub's archive layout, with a leading directory) and the build skips the download.

## Re-pinning

`scripts/update_espeak.py [COMMIT]` rewrites `espeak-ng.lock` for a commit (default: upstream master).

## docs.rs

docs.rs builds without network access, so with `DOCS_RS` set `build.rs` uses the bindings checked in under `docsrs/` and links nothing. Regenerate `docsrs/bindings.rs` from a normal build's `OUT_DIR/bindings.rs` when `wrapper.h` or the pin changes the API.

## Dependencies

- [espeak-ng](https://github.com/espeak-ng/espeak-ng)
