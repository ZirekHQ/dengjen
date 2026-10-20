# dengjen-voices

Voice catalogue, resumable verified download, install and removal for dengjen. Blocking and
synchronous; HTTP sits behind the `Transport` trait, with a `ureq` + rustls default.

```rust
use dengjen_voices::{Config, UreqTransport, VoiceManager};

let config = Config {
    voices_dir: "/home/me/.local/share/dengjen/voices".into(),
    model_type: "piper".into(),
    catalogue_url: "https://huggingface.co/rhasspy/piper-voices/raw/main/voices.json".into(),
    files_base_url: "https://huggingface.co/rhasspy/piper-voices/resolve/main".into(),
};
let manager = VoiceManager::new(config, UreqTransport::new())?;
manager.fetch_catalogue()?;
let kurmanji = manager.list(Some("ku"))?;
manager.install("ku_TR-berfin_renas-medium", &mut |p| eprintln!("{}/{}", p.done, p.total), &|| false)?;
```

Layout under `voices_dir`: `<model_type>/<key>/` holds installed voices with a `voice.json` sidecar.
Working state lives in `.staging/`, `.locks/`, `.old/` and `.trash/`, each namespaced by model type
(`.staging/<model_type>/<key>`), so model types can share one `voices_dir`; the catalogue cache in
`.catalogue/` is shared and keyed by URL.

- A voice is either fully installed or absent: files are verified in staging and the directory is
  renamed into place. An existing install is moved to `.old/` first, so no rename targets an
  existing directory (required on Windows); a failed replace restores it.
- A backup stranded in `.old/` by a crash is restored by the next install or removal of that voice.
  `installed()` does not recover it, so until then the voice reads as not installed.
- `remove` parks the voice in `.trash/` before deleting it, so an interrupted removal never leaves a
  half-deleted voice that looks installed.
- A file larger than its catalogue size, or one that fails its digest, is discarded and the install
  fails. A download that ends early is kept and resumed by the next install.
- Partial downloads are named by the catalogue digest, so a partial file from an older catalogue
  revision is discarded, not resumed. Files already verified in staging are kept across resumed
  installs. On Windows a voice with open files cannot be replaced or removed; the call returns an
  I/O error and the installed voice stays as it was.
- `install` and `remove` take an OS advisory lock per key; a concurrent call returns
  `VoiceError::Busy`.
- Catalogue file paths and keys are validated; a catalogue with an unsafe path or a file without a
  digest is rejected as a whole.
- `UreqTransport` honours `ALL_PROXY`, `HTTPS_PROXY`, `HTTP_PROXY` and `NO_PROXY`, follows up to
  five `https` redirects (also to other hosts; digests still gate every file) and fails a body read
  that stalls for 30 seconds (`UreqTransport::with_stall_timeout` changes that); total download time
  is unlimited.
- The C API is `libdengjen-voices` (`crates/frontends/capi-voices`).
