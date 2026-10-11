use std::env;
use std::path::{Path, PathBuf};

fn workspace_root(crate_dir: &Path) -> PathBuf {
    crate_dir
        .ancestors()
        .nth(3)
        .expect("crate directory is not nested 3 levels below the workspace root")
        .to_path_buf()
}

fn main() {
    let crate_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is not set"));

    let deps_dev_dir = workspace_root(&crate_dir).join("deps").join("dev");
    println!("cargo:rerun-if-changed={}", deps_dev_dir.display());

    let espeak_data_src = workspace_root(&crate_dir)
        .join("deps")
        .join("dev")
        .join("espeak-ng-data");
    let package_dir = crate_dir.join("python").join("pydengjen");
    let espeak_data_dst = package_dir.join("espeak-ng-data");

    if !espeak_data_src.exists() {
        println!(
            "cargo:warning=espeak-ng-data source not found at `{}`; skipping staging into pydengjen",
            espeak_data_src.display()
        );
        return;
    }

    if espeak_data_dst.exists() {
        std::fs::remove_dir_all(&espeak_data_dst)
            .expect("failed to remove the stale staged espeak-ng-data");
    }

    let options = fs_extra::dir::CopyOptions::new();
    fs_extra::dir::copy(&espeak_data_src, &package_dir, &options)
        .expect("failed to stage espeak-ng-data into the pydengjen package");
}
