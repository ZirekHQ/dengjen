# dengjen (Go bindings)

Go bindings for [`libdengjen`](../../crates/frontends/capi), the C API for
[dengjen-tts](../../README.md).

## Prerequisites

- Go 1.22+
- A Rust toolchain (to build `libdengjen` from source — see the parent
  repository's own build prerequisites)
- A C compiler (for cgo)
- eSpeak-ng data for synthesis tests: either install `espeak-ng`, or point
  `DENGJEN_ESPEAKNG_DATA_DIRECTORY` at a directory that holds
  `espeak-ng-data`

## Building and testing

```bash
make test    # builds libdengjen in release mode, then runs `go test ./...`
```

`make native` alone builds just the native library, without running tests.

## Usage

```go
import dengjen "github.com/ZirekHQ/dengjen-tts/bindings/go"

voice, err := dengjen.LoadVoice("/path/to/voice/config.json")
if err != nil {
    log.Fatal(err)
}
defer voice.Close()

params := dengjen.SynthesisParams{Mode: dengjen.SynthModeLazy, Rate: 10, Volume: 100, Pitch: 50}
_, err = voice.SpeakToFile("Hello, world.", params, "out.wav")
```

For streaming synthesis (audio delivered incrementally via a callback):

```go
err = voice.Speak("Hello, world.", params, func(e dengjen.SynthesisEvent) bool {
    switch e.Type {
    case dengjen.EventSpeech:
        // e.Data is a chunk of raw PCM audio
    case dengjen.EventFinished:
        // stream complete
    case dengjen.EventError:
        // e.Err describes what went wrong
    }
    return true // keep receiving events; return false to stop early
})
```

## Callback safety

`onEvent` (passed to `Speak`) must not panic and must not call
`runtime.Goexit` (which `testing.T.Fatal` does internally) — either would
unwind across the C/Rust call frames that invoke it, which is undefined
behavior at this FFI boundary. Recover from panics inside your own callback
if there's any chance it might panic.

## Known limitations

- This directory is the **development copy** of the Go bindings, used by
  this monorepo's own CI (`rust-lint.yaml`, `sonar.yaml`) and for local
  iteration. Its cgo build flags (`dengjen.go`) resolve `libdengjen.h`
  and the built native library via paths relative to this directory
  (`../../crates/frontends/capi`, `../../target/release`), which only
  exist inside a checkout of this repository — it is not `go get`-able
  as a standalone dependency from here.
- For a standalone, `go get`-able module with prebuilt binaries for
  linux/amd64, linux/arm64, windows/amd64, and darwin/arm64, use
  [github.com/ZirekHQ/dengjen-tts-go](https://github.com/ZirekHQ/dengjen-tts-go)
  instead — it's generated from this directory on every tagged release
  (see `.github/workflows/publish-go.yaml`). Changes to the bindings
  themselves are made here, not in that repo.
- A consumer binary built against *this* development copy needs
  `LD_LIBRARY_PATH` (or an equivalent runtime library search path)
  pointing at the directory holding the built `libdengjen` shared
  library at *run time*, not just at build/test time — `make test`'s
  `LD_LIBRARY_PATH=$(TARGET_DIR)` wiring only covers the test binary.
  (`dengjen-tts-go` doesn't have this limitation on Linux and macOS — it
  bakes an rpath in. Windows has no rpath equivalent; its consumers still
  need `libdengjen.dll` next to their built executable, or its directory
  on `PATH`. `dengjen-tts-go`'s Windows DLL also needs the
  [Visual C++ Redistributable](https://learn.microsoft.com/cpp/windows/latest-supported-vc-redist)
  on the target machine, since it's built with MSVC's default dynamic C
  runtime — usually already present, but not guaranteed on a minimal
  install.)
- This is a thin wrapper: `Speak`'s callback shape mirrors the C API
  closely rather than offering a fully idiomatic Go redesign (channels,
  `context.Context`-driven cancellation).

## Speech recognition

The `stt` package wraps `libdengjen-stt`. It is a separate package so text-to-speech users never
link sherpa-onnx; the published module ships its libraries for linux/amd64, windows/amd64 and
darwin/arm64 (not linux/arm64).

```go
r, err := stt.Load(modelDir, modelSHA256, 2)
if err != nil {
	return err
}
defer r.Close()
text, err := r.Recognize(samples, 16000) // mono float32 in [-1, 1]
if errors.Is(err, stt.ErrInvalidAudio) {
	// empty buffer or a zero sample rate
}
```

On Windows, put `stt/lib/windows_amd64` on `PATH` next to your executable's DLLs.

