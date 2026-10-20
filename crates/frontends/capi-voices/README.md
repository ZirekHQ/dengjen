# libdengjen-voices

C API for the dengjen-voices voice manager: fetch a voice catalogue, download voices with resume
and digest verification, install and remove them. Calls block; run them on a worker thread.

```c
#include <stdbool.h>
#include <stdio.h>
#include "libdengjen_voices.h"

VoicesConfig config = {
    .struct_size = sizeof(VoicesConfig),
    .voices_dir = "/home/me/.local/share/dengjen/voices",
    .model_type = "piper",
    .catalogue_url = "https://huggingface.co/rhasspy/piper-voices/raw/main/voices.json",
    .files_base_url = "https://huggingface.co/rhasspy/piper-voices/resolve/main",
};
/* Reports a failed call, releases its message and re-arms err for the next call. */
static bool failed(ExternError *err) {
    if (err->code == 0) return false;
    fprintf(stderr, "error %d: %s\n", err->code, err->message);
    libdengjenVoicesFreeString(err->message);
    *err = (ExternError){0};
    return true;
}

ExternError err = {0};
DengjenVoices *v = libdengjenVoicesOpen(&config, &err);
if (!v) {
    failed(&err);
    return 1;
}
libdengjenVoicesFetchCatalogue(v, &err);
if (!failed(&err)) {
    char *json = libdengjenVoicesListJson(v, "ku", &err);
    if (!failed(&err)) {
        puts(json);
        libdengjenVoicesFreeString(json);
    }
}
libdengjenVoicesInstall(v, "ku_TR-berfin_renas-medium", on_progress, NULL, &err);
failed(&err);
/* from another thread, or from on_progress: libdengjenVoicesCancel(v); */
libdengjenVoicesClose(v);
```

- `catalogue_url` and `files_base_url` are plain settings: point them at a mirror to leave
  Hugging Face.
- Digests: `sha256_digest` is verified when the catalogue carries it, otherwise `md5_digest`.
  MD5 only detects corruption, so prefer a catalogue with SHA-256 for untrusted mirrors.
- `libdengjenVoicesCancel(handle)` stops the installs in flight on that handle at the next chunk
  (`VOICES_ERR_CANCELLED`); installs started afterwards are unaffected, and a cancel issued before
  an install begins is not remembered. A cancelled or interrupted install keeps its partial files;
  the next install of the same voice resumes them. A cancel that arrives after the last data chunk
  has been read does not stop the install.
- The progress callback runs on the thread that called `libdengjenVoicesInstall`. Never call
  `libdengjenVoicesClose` while another call on the same handle is in flight.
- `libdengjenVoicesLoadCachedCatalogue` reads the catalogue cached by the last fetch for offline
  use. A failed fetch never falls back to the cache by itself.
- Error codes are the `VOICES_ERR_*` constants in `libdengjen_voices.h`; `VOICES_ERR_BUSY` means
  another call (or process) is installing or removing the same voice.
