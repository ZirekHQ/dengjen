# dengjen (Android bindings)

Java bindings for the dengjen TTS engine, packaged as three AARs:

- `core`: ONNX Runtime and the native loader.
- `tts`: the JNI library and the `Voice` API. Depends on `core`.
- `stt`: the JNI library, the sherpa-onnx C API and the `Recognizer` API. Depends on `core`; has no dependency on `tts`.

Supported ABIs: `arm64-v8a`, `x86_64`. minSdk 27.

## Building and testing

Prerequisites: JDK 17, Android SDK (platform 35), NDK r27c (the version CI pins) with `ANDROID_NDK_HOME` set, `cargo-ndk`, and the Rust targets `aarch64-linux-android` and `x86_64-linux-android`.

```bash
scripts/fetch-ort.sh        # ONNX Runtime from the pinned sherpa-onnx release
scripts/build-jni.sh debug  # JNI library with the self-test hooks (use `release` for the AAR)
./gradlew :core:assembleRelease :tts:assembleRelease :stt:assembleRelease
```

Instrumented tests need an emulator or device and a Piper voice plus eSpeak data pushed to `/data/local/tmp/dengjen-voice` (`model.onnx`, `model.onnx.json`, `espeak-ng-data/`; the CI job shows where each comes from):

```bash
adb push voice/. /data/local/tmp/dengjen-voice/
./gradlew :tts:connectedDebugAndroidTest
```

STT instrumented tests need the recognition model (`model.int8.onnx`, `tokens.txt` from `.github/stt-smoke-model.json`) in `/data/local/tmp/dengjen-stt`; the model-backed cases fail without it.

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

### Speech recognition

```java
try (Recognizer recognizer = Recognizer.load("/path/to/model-dir", modelSha256, 2)) {
  String text = recognizer.recognize(monoSamples, 16_000); // blocks, call from a worker thread
}
```

A C++ exception thrown inside sherpa-onnx surfaces as a `DengjenException` instead of aborting the process.

## Releases

Each GitHub Release carries `dengjen-android-{core,tts,stt}-<version>.aar` and `SHA256SUMS-android.txt`, all with build-provenance attestations. `size-budgets.json` caps each AAR and its largest native libraries, in bytes: `aar` is the file size, each `entries` value is the uncompressed size of that file inside the AAR. CI enforces it on pull requests (the `sizes` job in `android.yaml`) and again before a release is published (`scripts/check-sizes.sh <dir>`, where `<dir>` holds Gradle's unrenamed `core-release.aar`, `tts-release.aar` and `stt-release.aar`). To raise a budget, edit the file and give the measured size in the pull request description.
