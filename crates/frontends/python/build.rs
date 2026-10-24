use std::env;

fn main() {
    emit_runtime_search_path();
}

// sherpa-onnx-sys's own rpath applies only to its own targets, so the extension module that links
// it finds the bundled libraries beside itself through this one.
fn emit_runtime_search_path() {
    if env::var_os("CARGO_FEATURE_STT").is_none() {
        return;
    }
    match env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("linux") => println!("cargo:rustc-cdylib-link-arg=-Wl,-rpath,$ORIGIN"),
        Ok("macos") => println!("cargo:rustc-cdylib-link-arg=-Wl,-rpath,@loader_path"),
        _ => {}
    }
}
