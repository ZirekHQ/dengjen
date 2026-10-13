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

## Callback safety

The handler passed to `speak` must not throw and must return promptly — an
exception is caught and treated as "stop early" rather than propagated,
since letting one unwind across the native call frames that invoke it would
be undefined behavior at this FFI boundary.

## Known limitations

- **Not yet published** — the coordinates below apply once the first `java-v*`
  release lands; Central Portal namespace verification and release secrets
  are still pending prerequisites. Published artifacts ship the native `libdengjen` library as per-platform
  classifier jars (`linux-x86_64`, `linux-aarch64`, `windows-x64`,
  `macos-aarch64`). Consumers need both the main dependency and a
  `runtimeOnly` dependency on the classifier matching their platform:

  ```kotlin
  implementation("io.github.zirekhq:dengjen-tts:<version>")
  runtimeOnly("io.github.zirekhq:dengjen-tts:<version>:linux-x86_64")
  ```

  `DengjenLib` (via `NativeLibraryLoader`) detects the running platform and
  loads the matching classifier's native library automatically; set
  `-Ddengjen.native.library.path=<file>` to override with a library you
  built or placed yourself.
- **onnxruntime is not bundled** in the classifier jars. Install `onnxruntime` 1.28 or newer
  (the shared library of the official release, or the copy inside the `onnxruntime` pip package)
  and point to it with `-Ddengjen.onnxruntime.path=<file>` or the `ORT_DYLIB_PATH` environment
  variable; otherwise the platform loader search path is used. Set it before the first
  `Voice.load`.
- Every JVM invocation of a consumer built against this module needs
  `--enable-native-access=ALL-UNNAMED` (or the module-qualified equivalent)
  to avoid a native-access warning — `build.gradle.kts` already adds this
  for the module's own tests; a downstream consumer must add it themselves.
- This is a thin wrapper: `speak`'s callback shape mirrors the C API
  closely rather than offering a fully idiomatic Java redesign
  (`CompletableFuture`, reactive streams).
