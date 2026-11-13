# Dengjen for Linux

A GTK 4 app that reads text aloud with Dengjen's Kurmanji voices, for anyone, not only screen
reader users. Paste or type text, open a `.txt`, Markdown or HTML file, or read whatever is on
the clipboard. The sentence being read is highlighted, and the whole text can be saved as a WAV
file.

It lives in its own cargo workspace so GTK and ALSA stay out of the engine's builds.

## Voices

The app uses the Piper voices `dengjen voices install` puts in `~/.local/share/dengjen/voices`
(or `DENGJEN_VOICES_DIR`):

```sh
dengjen voices install ku_TR-berfin_renas-medium
```

## Build and run

Needs GTK 4.12, libadwaita 1.4 and ALSA headers:

```sh
sudo apt install libgtk-4-dev libadwaita-1-dev libasound2-dev
cargo run --release
```

That downloads ONNX Runtime at build time. To use a system ONNX Runtime instead, build with
`--features ort-dylib` and point `ORT_DYLIB_PATH` at `libonnxruntime.so`.

## Keys

| Key | Action |
|---|---|
| F5 | Play or pause |
| Escape | Stop |
| Alt+Left, Alt+Right | Previous or next sentence |
| Ctrl+O | Open a file |
| Ctrl+Shift+V | Read the clipboard |
| Ctrl+S | Save as audio |

Reading starts at the sentence under the caret. A new voice, speed or pitch is used from the
next time reading starts or moves to another sentence with the buttons.

## Tests

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```
