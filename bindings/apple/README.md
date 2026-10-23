# Apple XCFramework

`Libdengjen.xcframework` wraps the `libdengjen` C API as a static library for iOS (device and simulator) and macOS, all arm64.

Prebuilt ONNX Runtime has no x86_64 Apple targets, so there are no Intel simulator or Mac slices.

## Build

```sh
bindings/apple/scripts/build-xcframework.sh [debug|release]
```

Needs macOS with Xcode, `llvm`, `protobuf` and `espeak-ng` from Homebrew. The result is `target/apple/Libdengjen.xcframework.zip` plus its `.sha256`. `IPHONEOS_DEPLOYMENT_TARGET` (default 16.0) and `MACOSX_DEPLOYMENT_TARGET` (default 13.0) set the minimum OS.

## Use

Add the XCFramework to an Xcode target, then import the Clang module from Swift or Objective-C:

```swift
import Libdengjen

let version = libdengjenAbiVersion()
```

The archive links CoreML and Accelerate. Add `-lc++` to the app's linker flags if Xcode does not infer it.

## eSpeak data

The archive does not ship `espeak-ng-data`. Bundle it in the app and point `DENGJEN_ESPEAKNG_DATA_DIRECTORY` at its parent directory before loading a voice.

## Releases

Each GitHub Release carries `dengjen-apple-<version>.xcframework.zip` and `SHA256SUMS-apple.txt`, both with build-provenance attestations. `bindings/apple/scripts/check-sizes.sh` enforces `size-budgets.json` on every Apple build.
