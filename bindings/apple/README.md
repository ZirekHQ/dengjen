# Apple XCFramework

`Libdengjen.xcframework` wraps the `libdengjen` C API as a static library for iOS (device and simulator) and macOS, all arm64.

Prebuilt ONNX Runtime has no x86_64 Apple targets, so there are no Intel simulator or Mac slices.

## Build

```sh
bindings/apple/scripts/build-xcframework.sh [debug|release]
```

Needs macOS with Xcode, `llvm`, `protobuf` and `espeak-ng` from Homebrew. The result is `target/apple/Libdengjen.xcframework.zip` plus its `.sha256`. `IPHONEOS_DEPLOYMENT_TARGET` (default 16.0) and `MACOSX_DEPLOYMENT_TARGET` (default 13.0) set the minimum OS.

`APPLE_FEATURES` picks the Cargo features (default `espeak,coreml,voices`) and `APPLE_TARGETS` the slices (default all three), so a Mac app that needs no eSpeak-ng can build `APPLE_FEATURES=coreml,voices APPLE_TARGETS=aarch64-apple-darwin`. Kurmanji Piper voices use the built-in Kurmanji phonemizer either way.

## Use

Add the XCFramework to an Xcode target, then import the Clang module from Swift or Objective-C:

```swift
import Libdengjen

let version = libdengjenAbiVersion()
```

The archive links CoreML and Accelerate. Add `-lc++` to the app's linker flags if Xcode does not infer it.

## Voice downloads

The default build also exports the `libdengjen-voices` C API (`crates/frontends/capi-voices`): the voice catalogue, verified resumable downloads, and removal. It is a third Clang module:

```swift
import LibdengjenVoices
```

Like the dictation header, it defines its own `ExternError`, so a Swift file that imports both modules names the type with its module, for example `LibdengjenVoices.ExternError`.

## Dictation (opt-in)

`APPLE_FEATURES=espeak,coreml,stt bindings/apple/scripts/build-xcframework.sh` also exports the `libdengjen-stt` C API, importable as a second Clang module:

```swift
import LibdengjenStt
```

The two headers define the same `ExternError` type, so they are separate modules rather than one. Both APIs live in one static library, because two Rust static libraries in one app collide on duplicate runtime symbols. The recognizer needs the sherpa-onnx libraries embedded in the app; the XCFramework does not contain them.

## eSpeak data

The archive does not ship `espeak-ng-data`. Bundle it in the app and point `DENGJEN_ESPEAKNG_DATA_DIRECTORY` at its parent directory before loading a voice. Each release attaches `dengjen-espeak-ng-data-<version>.tar.gz`, built from the same pinned espeak-ng source; the distribution's copy lacks upstream fixes such as the Kurmanji word stress.

## Releases

Each GitHub Release carries, all with build-provenance attestations:

- `dengjen-apple-<version>.xcframework.zip`: speech synthesis and voice downloads, with `SHA256SUMS-apple.txt`.
- `dengjen-apple-stt-<version>.xcframework.zip`: synthesis, voice downloads and dictation, with `SHA256SUMS-apple-stt.txt`.
- `dengjen-apple-sherpa-onnx-ios-<version>.xcframework.zip`: `SherpaOnnxC.xcframework`. Embed it in iOS apps that use dictation.
- `dengjen-apple-sherpa-onnx-macos-<version>.zip`: the sherpa-onnx and ONNX Runtime dylibs for macOS apps that use dictation.

`bindings/apple/scripts/check-sizes.sh` enforces `size-budgets.json` on every Apple build.
