# libdengjen-stt

C API for the dengjen-stt speech recognizer. It is a separate library from libdengjen, so the
sherpa-onnx runtime never shares a binary with the `ort` copy that libdengjen links.

```c
#include "libdengjen_stt.h"

ExternError err = {0};
DengjenRecognizer *r = libdengjenSttLoad(model_dir, NULL, expected_sha256, &err);
if (!r) { /* inspect err.code, then libdengjenSttFreeString(err.message) */ }
char *text = libdengjenSttRecognize(r, samples, n, 16000, &err);
libdengjenSttFreeString(text);
libdengjenSttUnload(r);
```

- `libdengjenSttLoad` requires the hex SHA-256 of `model.int8.onnx` and refuses a file that does not
  match: sherpa-onnx aborts the process on a corrupt model. `config` may be NULL.
- `libdengjenSttRecognize` blocks until decoding ends and cannot be cancelled; call it from a worker
  thread. See the doc comments in `src/lib.rs` for the audio limits.
- Failures set `out_error` as in libdengjen; free its message with `libdengjenSttFreeString`.
  STT codes are 40–45, and the shared ones (`INVALID_UTF8_SEQUENCE` 20, `NULL_POINTER` 22,
  `INVALID_CONFIGURATION` 24) keep libdengjen's values.
- `libdengjenSttAbiVersion()` returns `STT_ABI_VERSION`.
- `libdengjen_stt.h` and `libdengjen.h` both define `ExternError`; include only one per translation
  unit.
- Linking is shared: ship the shared sherpa-onnx and ONNX Runtime libraries beside this one.
