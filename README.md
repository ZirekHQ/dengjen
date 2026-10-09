# Dengjen

[Project board](https://github.com/orgs/ZirekHQ/projects/1) — live roadmap and status for this repo's issues.

A cross-platform Rust engine for neural TTS models.

## Features

* **Models**: [Piper](https://github.com/rhasspy/piper), [Kokoro](https://github.com/hexgrad/kokoro), and [MeloTTS](https://github.com/myshell-ai/MeloTTS) ONNX voices
* **Phonemization**: eSpeak-ng (100+ languages, IPA output) and Arabic diacritization via `libtashkeel`
* **Multi-speaker voices**: select by `speaker_id`
* **Streaming synthesis**: chunked output (`chunk_size`/`chunk_padding`) and a realtime gRPC stream
* **Prosody control**: rate, pitch, and volume via `dengjen-sonic-sys` (libsonic)
* **Synthesis modes**: lazy, parallel, and batched, selectable per request; realtime for the voices that support it
* **Bindings**: native Rust, C-API (`libdengjen`), Python (`pydengjen`), Go and Java bindings over the C API (see `bindings/go` and `bindings/java`), gRPC (any language over the wire), and a CLI

RHVoice-style formant/statistical synthesis is a different synthesis paradigm from
this engine's neural-ONNX pipeline and isn't planned.

## What each model supports

|  | Piper | Kokoro | MeloTTS |
| --- | --- | --- | --- |
| Lazy, parallel, batched modes | Yes | Yes | Yes |
| Realtime streaming | Voices whose manifest sets `"streaming": true` | Yes, by chunking an already synthesized sentence | No |
| Speakers | Manifest speaker map | Presets from the manifest's `voices` list | Manifest speaker map |
| Rate, pitch, volume, silence | Yes | Yes | Yes |
| Tunable inference knobs | `noise_scale`, `length_scale`, `noise_w` | None | `noise_scale`, `length_scale`, `noise_scale_w` |
| Phonemizers | eSpeak-ng, raw text, Hebrew, pinyin; Arabic diacritization via `libtashkeel` | eSpeak-ng, fixed to `en-US` | eSpeak-ng (`en`, `es`, `fr`, `ja`, `ko`) or pinyin (`zh`) |

The Hebrew and pinyin phonemizers are opt-in Cargo features (`hebrew`, `pinyin`) that no frontend in this repository enables, so the prebuilt `dengjen-tts-grpc` server and the `pydengjen` wheels do not include them; embedding the Rust crates lets you enable them. Arabic diacritization (the `tashkeel` feature) is enabled by default in every frontend this repository publishes.

## Documentation

The documentation lives at <https://zirekhq.github.io/dengjen-tts/next/index.html>:

- [Overview and crate list](https://zirekhq.github.io/dengjen-tts/next/index.html)
- [Get started with the gRPC server](https://zirekhq.github.io/dengjen-tts/next/start-grpc.html)
- [Get started with Python](https://zirekhq.github.io/dengjen-tts/next/start-python.html)
- [Build from source](https://zirekhq.github.io/dengjen-tts/next/installation.html): the workspace build and the eSpeak-ng data directory
- [Usage](https://zirekhq.github.io/dengjen-tts/next/usage.html): synthesize from the command line
- [Choosing and tuning a model backend](https://zirekhq.github.io/dengjen-tts/next/voices.html)
- [Streaming synthesis and the gRPC frontend](https://zirekhq.github.io/dengjen-tts/next/streaming.html)
- [Phonemizer language coverage](https://zirekhq.github.io/dengjen-tts/next/phonemizers.html)
- [Architecture](https://zirekhq.github.io/dengjen-tts/next/architecture.html)
- [Testing](https://zirekhq.github.io/dengjen-tts/next/testing.html)

To contribute, see [CONTRIBUTING.md](.github/CONTRIBUTING.md).

## License

Licensed under the GNU General Public License v3.0 or later (GPL-3.0-or-later) — see
[LICENSE](LICENSE). dengjen originated as a fork of [Sonata](https://github.com/mush42/sonata) by
Musharraf Omer, originally MIT-licensed; see [NOTICE](NOTICE) for project history and third-party
attributions.

---

## 💝 Support This Project

If this repository saves you time and effort, please consider supporting it!

- ⭐ [Star on GitHub](https://github.com/ZirekHQ/dengjen-tts)
- 🐦 [Share on Twitter](https://twitter.com/intent/tweet?text=dengjen%20-%20a%20cross-platform%20Rust%20engine%20for%20neural%20TTS%20models&url=https%3A%2F%2Fgithub.com%2FZirekHQ%2Fdengjen-tts)
- 💼 [Share on LinkedIn](https://www.linkedin.com/sharing/share-offsite/?url=https%3A%2F%2Fgithub.com%2FZirekHQ%2Fdengjen-tts)
- 💖 [Support on Open Collective](https://opencollective.com/zirek)

