// Sherpa entry points that can throw C++ exceptions; the linker routes calls to them through
// the `__wrap_` functions in `src/shim_wrap.cpp`, which must list the same symbols. Remove the
// wrapping once sherpa-onnx reports errors instead of throwing (#410, #411).
const WRAPPED: [&str; 5] = [
    "SherpaOnnxCreateOfflineRecognizer",
    "SherpaOnnxCreateOfflineStream",
    "SherpaOnnxAcceptWaveformOffline",
    "SherpaOnnxDecodeOfflineStream",
    "SherpaOnnxGetOfflineStreamResultAsJson",
];

fn main() {
    let android = std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "android");
    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++17")
        .flag("-fexceptions")
        .file("src/shim_core.cpp");
    if android {
        build
            .file("src/shim_wrap.cpp")
            .cpp_link_stdlib("c++_shared");
        for symbol in WRAPPED {
            println!("cargo:rustc-link-arg-cdylib=-Wl,--wrap={symbol}");
        }
    }
    build.compile("dengjen_stt_shim");
    println!("cargo:rerun-if-changed=src/shim_core.cpp");
    println!("cargo:rerun-if-changed=src/shim_wrap.cpp");
}
