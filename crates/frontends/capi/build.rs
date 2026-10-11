use cbindgen::{Config, ExportConfig};
use std::env;
use std::path::Path;

const HEADER_OUTPUT: &str = "libdengjen.h";

fn main() {
    println!("cargo:rerun-if-changed=./src/lib.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");

    let crate_root = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is not set");
    if !in_workspace_tree(Path::new(&crate_root)) {
        // Outside the workspace tree (a packaged archive) cbindgen's `cargo metadata` call cannot
        // resolve the sibling crates; skip and ship the committed header.
        return;
    }

    let config = Config {
        export: ExportConfig {
            include: vec!["SpeechSynthesisCallback".to_string()],
            ..Default::default()
        },
        ..Default::default()
    };

    let header = cbindgen::Builder::new()
        .with_crate(crate_root)
        .with_config(config)
        .with_include_version(true)
        .with_documentation(false)
        .with_parse_deps(true)
        .with_parse_include(&["ffi-support"])
        .with_cpp_compat(false)
        .with_language(cbindgen::Language::C)
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
