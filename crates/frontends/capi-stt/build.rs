use cbindgen::Config;
use std::env;
use std::path::Path;

const HEADER_OUTPUT: &str = "libdengjen_stt.h";

fn main() {
    emit_runtime_search_path();
    println!("cargo:rerun-if-changed=./src/lib.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");

    let crate_root = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is not set");
    if !in_workspace_tree(Path::new(&crate_root)) {
        // Outside the workspace tree (a packaged archive) cbindgen's `cargo metadata` call cannot
        // resolve the sibling crates; skip and ship the committed header.
        return;
    }

    println!("cargo:rerun-if-changed=cbindgen.toml");
    let config = Config::from_file(Path::new(&crate_root).join("cbindgen.toml"))
        .expect("cbindgen.toml must parse");

    let header = cbindgen::Builder::new()
        .with_crate(crate_root)
        .with_config(config)
        .generate()
        .expect("cbindgen failed to generate C bindings");

    header.write_to_file(HEADER_OUTPUT);
}

fn in_workspace_tree(crate_dir: &Path) -> bool {
    crate_dir
        .ancestors()
        .nth(2)
        .and_then(Path::file_name)
        .is_some_and(|name| name == "crates")
}

// sherpa-onnx-sys's own rpath applies only to its own targets, not to this cdylib.
fn emit_runtime_search_path() {
    match env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("linux") => println!("cargo:rustc-cdylib-link-arg=-Wl,-rpath,$ORIGIN"),
        Ok("macos") => println!("cargo:rustc-cdylib-link-arg=-Wl,-rpath,@loader_path"),
        _ => {}
    }
}
