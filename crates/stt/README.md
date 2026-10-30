# dengjen-stt

Offline Kurmanji speech recognition on [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx), using omnilingual CTC models.

`catalogue` lists the downloadable models with their archive URL and the SHA-256 of `model.int8.onnx` and `tokens.txt`. `DEFAULT_MODEL` is the 300M int8 model; the 1B int8 model is the higher-accuracy option. Verify a downloaded model before loading it, because a corrupt model file aborts the process:

```rust
use dengjen_stt::{catalogue, verify_model, verify_tokens, Recognizer, RecognizerConfig};
use std::path::Path;

let entry = catalogue::DEFAULT_MODEL;
let dir = Path::new("/path/to/unpacked/archive");
verify_model(dir, entry.model_sha256)?;
verify_tokens(dir, entry.tokens_sha256)?;
let recognizer = Recognizer::new(dir, RecognizerConfig::default())?;
```

## Support

This crate is part of [Zirek](https://github.com/ZirekHQ). 💖 [Support on Open Collective](https://opencollective.com/zirek)
