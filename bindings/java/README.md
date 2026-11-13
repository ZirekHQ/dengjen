# dengjen (Java bindings)

Java bindings for [`libdengjen`](../../crates/frontends/capi), the C API for
[dengjen-tts](../../README.md), built on the Java FFM API
(`java.lang.foreign`).

## Prerequisites

- JDK 25 (the Gradle toolchain pins 25, the LTS release where the FFM API is
  stable and non-preview; FFM has been stable since Java 22)
- A Rust toolchain (to build `libdengjen` from source — see the parent
  repository's own build prerequisites)

## Building and testing

```bash
make test    # builds libdengjen in release mode, then runs unit + integration tests
```

`make native` alone builds just the native library, without running tests.

## Usage

```java
try (Voice voice = Voice.load("/path/to/voice/config.json")) {
    AudioInfo info = voice.getAudioInfo();

    SynthesisParams params = new SynthesisParams(SynthesisMode.LAZY, 10, 100, 50, 0);
    voice.speakToFile("Hello, world.", params, "out.wav");
}
```

For streaming synthesis (audio delivered incrementally via a callback):

```java
try (Voice voice = Voice.load("/path/to/voice/config.json")) {
    SynthesisParams params = new SynthesisParams(SynthesisMode.LAZY, 10, 100, 50, 0);
    voice.speak("Hello, world.", params, event -> {
        switch (event.type()) {
            case SPEECH -> { /* event.data() is a chunk of raw PCM audio */ }
            case FINISHED -> { /* stream complete */ }
            case ERROR -> { /* event.error() describes what went wrong */ }
        }
        return true; // keep receiving events; return false to stop early
    });
}
```

## Speech recognition

`Recognizer` wraps `libdengjen-stt`. Add the natives for your platform next to the bindings:

```kotlin
runtimeOnly("io.github.zirekhq:dengjen-stt-natives:<version>:linux-x86_64")
```

```java
try (Recognizer recognizer = Recognizer.load(modelDir, modelSha256, 2)) {
  String text = recognizer.recognize(samples, 16_000);
}
```

`samples` are mono floats in [-1, 1]. Failures throw `SttException` with an `SttErrorCode`. Set
`-Ddengjen.stt.native.dir=<dir>` to load a directory you built yourself instead of the classpath
jar. On Windows, `onnxruntime.dll` must be the only ONNX Runtime loaded in the JVM, so combine
this with the text-to-speech bindings only when both use the same ONNX Runtime version.

## Choosing an ONNX Runtime

Text to speech and speech recognition each read their own settings, because sherpa-onnx needs the
ONNX Runtime build it was compiled against. Use the `dengjen.onnxruntime` prefix for text to
speech and `dengjen.stt.onnxruntime` for speech recognition:

| System property | Meaning |
|---|---|
| `<prefix>.path` | A library file already on disk, used as is. |
| `<prefix>.url` and `<prefix>.sha256` | Download this file once and use it only if its SHA-256 matches. A URL without a digest is refused. |
| `<prefix>.file` | Optional with `.url`: the file name to keep it under, when the URL does not end in one. |
| `dengjen.cache.dir` | Where downloads are kept (default `~/.cache/dengjen` on Linux, `~/Library/Caches/dengjen` on macOS, `%LOCALAPPDATA%\dengjen\cache` on Windows). |

Downloads land in `<cache>/onnxruntime/<sha256>/<file name>`. A partial or mismatched download is
never used, and a later run reuses the verified file without going online. Pointing at your own
build (a GPU one, or a newer patch release) is at your own risk: it has to be a version the native
library supports. For speech recognition the file replaces the copy in the natives jar under that
copy's name, and `-Ddengjen.stt.native.dir` ignores these settings because it names a complete
directory.

## Callback safety

The handler passed to `speak` must not throw and must return promptly — an
exception is caught and treated as "stop early" rather than propagated,
since letting one unwind across the native call frames that invoke it would
be undefined behavior at this FFI boundary.

## Known limitations

- **Not yet published** — the coordinates below apply once the first `java-v*`
  release lands; Central Portal namespace verification and release secrets
  are still pending prerequisites. The native `libdengjen` library ships in the separate
  `dengjen-tts-natives` artifact as per-platform classifier jars (`linux-x86_64`, `linux-aarch64`,
  `windows-x64`, `macos-aarch64`). It is published only when the native code changes, so its
  version differs from the binding's (`NATIVES_VERSION` below is the `nativesVersion` in
  `build.gradle.kts` at the binding's tag). Consumers need both the main dependency and a
  `runtimeOnly` dependency on the classifier matching their platform:

  ```kotlin
  implementation("io.github.zirekhq:dengjen-tts:<version>")
  runtimeOnly("io.github.zirekhq:dengjen-tts-natives:<NATIVES_VERSION>:linux-x86_64")
  ```

  `DengjenLib` (via `NativeLibraryLoader`) detects the running platform and
  loads the matching classifier's native library automatically; set
  `-Ddengjen.native.library.path=<file>` to override with a library you
  built or placed yourself.
- **onnxruntime is not bundled** in the classifier jars. Install `onnxruntime` 1.28 or newer
  (the shared library of the official release, or the copy inside the `onnxruntime` pip package)
  and point to it with `-Ddengjen.onnxruntime.path=<file>` or the `ORT_DYLIB_PATH` environment
  variable; otherwise the platform loader search path is used. Set it before the first
  `Voice.load`. See [Choosing an ONNX Runtime](#choosing-an-onnx-runtime) to have it downloaded
  instead.
- Every JVM invocation of a consumer built against this module needs
  `--enable-native-access=ALL-UNNAMED` (or the module-qualified equivalent)
  to avoid a native-access warning — `build.gradle.kts` already adds this
  for the module's own tests; a downstream consumer must add it themselves.
- This is a thin wrapper: `speak`'s callback shape mirrors the C API
  closely rather than offering a fully idiomatic Java redesign
  (`CompletableFuture`, reactive streams).
