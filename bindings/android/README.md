# dengjen (Android bindings)

Java bindings for the dengjen TTS engine, packaged as two AARs:

- `core`: ONNX Runtime and the native loader.
- `tts`: the JNI library and the `Voice` API. Depends on `core`.

Supported ABIs: `arm64-v8a`, `x86_64`. minSdk 27.

## Building and testing

Prerequisites: JDK 17, Android SDK (platform 35), a recent NDK with `ANDROID_NDK_HOME` set, `cargo-ndk`, and the Rust targets `aarch64-linux-android` and `x86_64-linux-android`.

```bash
scripts/fetch-ort.sh        # ONNX Runtime from the pinned sherpa-onnx release
scripts/build-jni.sh debug  # JNI library with the self-test hooks (use `release` for the AAR)
./gradlew :core:assembleRelease :tts:assembleRelease
```

Instrumented tests need an emulator or device and a Piper voice plus eSpeak data pushed to `/data/local/tmp/dengjen-voice` (`model.onnx`, `model.onnx.json`, `espeak-ng-data/`; the CI job shows where each comes from):

```bash
adb push voice/. /data/local/tmp/dengjen-voice/
./gradlew :tts:connectedDebugAndroidTest
```

## Usage

```java
Voice.setEspeakDataDirectory(dirContainingEspeakNgData);
try (Voice voice = Voice.load("/path/to/model.onnx.json")) {
  SynthesisParams params = new SynthesisParams(SynthesisMode.LAZY, 10, 100, 50, 0);
  voice.speak("Hello.", params, event -> {
    if (event.type() == EventType.SPEECH) { /* event.data() is raw PCM */ }
    return true; // return false to stop early
  });
}
```

The handler runs on the synthesis thread, must return promptly, and stops synthesis if it throws. The engine uses the CPU execution provider on Android; NNAPI is opt-in through the `nnapi` feature of `libdengjen`.
