# libdengjen

C API bindings for the dengjen-tts speech synthesis engine.

Exposes `extern "C"` functions such as `libdengjenLoadVoiceFromConfigPath`, `libdengjenSpeak`, and `libdengjenSpeakToFile`, delivering audio and completion events to a caller-supplied `SpeechSynthesisCallback` via a `SynthesisEvent` struct; the C header is generated from this crate with `cbindgen`.

## ABI stability

- `libdengjenAbiVersion()` returns the ABI version, equal to `DENGJEN_ABI_VERSION` in the header. Bindings compare the two before any other call; an incompatible change to an exported function or struct bumps it.
- `libdengjenSpeak` and `libdengjenSpeakToFile` take `const SynthesisParams *`. Set `struct_size` to `sizeof(SynthesisParams)` as you compiled it. Later versions only append fields and read them when `struct_size` covers them, so a caller built against an older header keeps working.
- A `struct_size` smaller than the first layout returns `INVALID_CONFIGURATION`.
