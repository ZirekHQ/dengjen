use cmake::Config;
use glob::glob;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

macro_rules! debug_log {
    ($($arg:tt)*) => {
        if std::env::var("BUILD_DEBUG").is_ok() {
            println!("cargo:warning=[DEBUG] {}", format!($($arg)*));
        }
    };
}

fn get_cargo_target_dir() -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
    let profile = std::env::var("PROFILE")?;
    let mut target_dir = None;
    let mut sub_path = out_dir.as_path();
    while let Some(parent) = sub_path.parent() {
        if parent.ends_with(&profile) {
            target_dir = Some(parent);
            break;
        }
        sub_path = parent;
    }
    let target_dir = target_dir.ok_or("not found")?;
    Ok(target_dir.to_path_buf())
}

pub(crate) fn copy_succeeded(is_windows: bool, exit_code: Option<i32>) -> bool {
    if is_windows {
        exit_code.is_some_and(|code| code < 8)
    } else {
        exit_code == Some(0)
    }
}

pub(crate) fn copy_folder(src: &Path, dst: &Path) {
    assert!(
        src.exists(),
        "copy source not found at {} (for the espeak-ng submodule, did you run \
         `git submodule update --init`?)",
        src.display()
    );

    let parent = dst
        .parent()
        .expect("copy destination must have a parent directory");
    std::fs::create_dir_all(parent).expect("Failed to create parent directory");

    let tmp_dst = parent.join(format!(
        "{}.tmp",
        dst.file_name()
            .and_then(|name| name.to_str())
            .expect("copy destination must have a UTF-8 file name")
    ));
    let _ = std::fs::remove_dir_all(&tmp_dst);
    assert!(
        !tmp_dst.exists(),
        "failed to remove stale temp copy at {} before copying into it",
        tmp_dst.display()
    );

    let status = if cfg!(windows) {
        std::process::Command::new("robocopy.exe")
            .arg("/e")
            .arg(src)
            .arg(&tmp_dst)
            .status()
            .expect("Failed to execute robocopy command")
    } else {
        std::process::Command::new("cp")
            .arg("-rf")
            .arg(src)
            .arg(&tmp_dst)
            .status()
            .expect("Failed to execute cp command")
    };

    assert!(
        copy_succeeded(cfg!(windows), status.code()),
        "copying {} to {} failed with {status}",
        src.display(),
        tmp_dst.display(),
    );

    std::fs::rename(&tmp_dst, dst).expect("Failed to move completed copy into place");
}

pub(crate) enum EspeakNgSource<'a> {
    Directory(&'a Path),
    Bundle(&'a Path),
}

pub(crate) fn resolve_espeak_ng_source<'a>(
    espeak_src: &'a Path,
    bundle_path: &'a Path,
) -> EspeakNgSource<'a> {
    if espeak_src.exists() {
        EspeakNgSource::Directory(espeak_src)
    } else if bundle_path.exists() {
        EspeakNgSource::Bundle(bundle_path)
    } else {
        panic!(
            "neither the espeak-ng submodule ({}) nor the pre-built bundle ({}) was found -- \
             did you run `git submodule update --init`?",
            espeak_src.display(),
            bundle_path.display()
        );
    }
}

pub(crate) fn extract_xz_tar_bundle(bundle: &Path, dst: &Path) {
    assert!(
        bundle.exists(),
        "espeak-ng bundle not found at {}",
        bundle.display()
    );

    let parent = dst
        .parent()
        .expect("extraction destination must have a parent directory");
    std::fs::create_dir_all(parent).expect("Failed to create parent directory");

    let tmp_dst = parent.join(format!(
        "{}.tmp",
        dst.file_name()
            .and_then(|name| name.to_str())
            .expect("extraction destination must have a UTF-8 file name")
    ));
    let _ = std::fs::remove_dir_all(&tmp_dst);
    assert!(
        !tmp_dst.exists(),
        "failed to remove stale temp extraction at {} before extracting into it",
        tmp_dst.display()
    );

    let compressed = std::fs::read(bundle)
        .unwrap_or_else(|e| panic!("failed to read bundle {}: {e}", bundle.display()));
    let mut decompressed = Vec::new();
    lzma_rs::xz_decompress(&mut &compressed[..], &mut decompressed)
        .unwrap_or_else(|e| panic!("failed to xz-decompress bundle {}: {e}", bundle.display()));

    std::fs::create_dir_all(&tmp_dst).expect("Failed to create extraction temp directory");
    tar::Archive::new(&decompressed[..])
        .unpack(&tmp_dst)
        .unwrap_or_else(|e| panic!("failed to unpack bundle {}: {e}", bundle.display()));

    std::fs::rename(&tmp_dst, dst).expect("Failed to move extracted bundle into place");
}

const ESPEAK_NG_DATA_DIR_NAME: &str = "espeak-ng-data";

pub(crate) fn copy_espeak_ng_data_next_to_binary(out_dir: &Path, target_dir: &Path) {
    let src = out_dir.join("share").join(ESPEAK_NG_DATA_DIR_NAME);
    if !src.exists() {
        return;
    }
    let dst = target_dir.join(ESPEAK_NG_DATA_DIR_NAME);
    if dst.exists() {
        std::fs::remove_dir_all(&dst)
            .expect("Failed to remove stale espeak-ng-data copy before refreshing it");
    }
    copy_folder(&src, &dst);
}

fn extract_lib_names(out_dir: &Path, build_shared_libs: bool, target_os: &str) -> Vec<String> {
    let lib_pattern = if target_os == "windows" {
        "*.lib"
    } else if target_os == "macos" {
        if build_shared_libs { "*.dylib" } else { "*.a" }
    } else if build_shared_libs {
        "*.so"
    } else {
        "*.a"
    };
    let libs_dir = out_dir.join("lib");
    let pattern = libs_dir.join(lib_pattern);
    debug_log!("Extract libs {}", pattern.display());

    let mut lib_names: Vec<String> = Vec::new();

    for entry in glob(pattern.to_str().unwrap()).unwrap() {
        match entry {
            Ok(path) => {
                let stem = path.file_stem().unwrap();
                let stem_str = stem.to_str().unwrap();

                let lib_name = if stem_str.starts_with("lib") {
                    stem_str.strip_prefix("lib").unwrap_or(stem_str)
                } else {
                    stem_str
                };
                lib_names.push(lib_name.to_string());
            }
            Err(e) => println!("cargo:warning=error={}", e),
        }
    }
    lib_names
}

fn extract_lib_assets(out_dir: &Path, target_os: &str) -> Vec<PathBuf> {
    let shared_lib_pattern = if target_os == "windows" {
        "*.dll"
    } else if target_os == "macos" {
        "*.dylib"
    } else {
        "*.so"
    };

    let libs_dir = out_dir.join("lib");
    let pattern = libs_dir.join(shared_lib_pattern);
    debug_log!("Extract lib assets {}", pattern.display());
    let mut files = Vec::new();

    for entry in glob(pattern.to_str().unwrap()).unwrap() {
        match entry {
            Ok(path) => {
                files.push(path);
            }
            Err(e) => eprintln!("cargo:warning=error={}", e),
        }
    }

    files
}

fn android_abi(target_arch: &str) -> &'static str {
    match target_arch {
        "aarch64" => "arm64-v8a",
        "arm" => "armeabi-v7a",
        "x86" => "x86",
        "x86_64" => "x86_64",
        other => panic!("unsupported Android target arch: {other}"),
    }
}

fn android_ndk_home() -> String {
    ["ANDROID_NDK_HOME", "ANDROID_NDK_ROOT", "NDK_HOME"]
        .into_iter()
        .find_map(|var| env::var(var).ok())
        .expect(
            "targeting Android requires ANDROID_NDK_HOME (or ANDROID_NDK_ROOT/NDK_HOME) \
             to be set to an Android NDK installation",
        )
}

fn android_toolchain_file(ndk_home: &str) -> PathBuf {
    let toolchain_file = Path::new(ndk_home)
        .join("build")
        .join("cmake")
        .join("android.toolchain.cmake");
    assert!(
        toolchain_file.exists(),
        "Android NDK toolchain file not found at {} (checked NDK home {})",
        toolchain_file.display(),
        ndk_home
    );
    toolchain_file
}

fn ndk_host_tag() -> &'static str {
    if cfg!(target_os = "macos") {
        "darwin-x86_64"
    } else if cfg!(target_os = "windows") {
        "windows-x86_64"
    } else {
        "linux-x86_64"
    }
}

fn android_sysroot(ndk_home: &str) -> PathBuf {
    let sysroot = Path::new(ndk_home)
        .join("toolchains")
        .join("llvm")
        .join("prebuilt")
        .join(ndk_host_tag())
        .join("sysroot");
    assert!(
        sysroot.exists(),
        "Android NDK sysroot not found at {} (checked NDK home {})",
        sysroot.display(),
        ndk_home
    );
    sysroot
}

fn android_api_level() -> u32 {
    env::var("ANDROID_PLATFORM")
        .ok()
        .and_then(|v| v.trim_start_matches("android-").parse().ok())
        .unwrap_or(21)
}

fn macos_link_search_path() -> Option<String> {
    let output = Command::new("clang")
        .arg("--print-search-dirs")
        .output()
        .ok()?;
    if !output.status.success() {
        println!(
            "failed to run 'clang --print-search-dirs', continuing without a link search path"
        );
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if line.contains("libraries: =") {
            let path = line.split('=').nth(1)?;
            return Some(format!("{}/lib/darwin", path));
        }
    }

    println!("failed to determine link search path, continuing without it");
    None
}

fn cmake_bool_is_true(value: &str) -> bool {
    let upper = value.to_ascii_uppercase();
    match upper.as_str() {
        "ON" | "YES" | "TRUE" | "Y" => true,
        "OFF" | "NO" | "FALSE" | "N" | "IGNORE" | "NOTFOUND" | "" => false,
        _ if upper.ends_with("-NOTFOUND") => false,
        _ => upper.parse::<i64>().is_ok_and(|n| n != 0),
    }
}

pub(crate) fn resolved_pcaudio_lib(cache_contents: &str) -> Option<PathBuf> {
    resolved_system_lib(
        cache_contents,
        "USE_LIBPCAUDIO:BOOL=",
        "PCAUDIO_LIB:FILEPATH=",
        "PCAUDIO_INC:PATH=",
    )
}

pub(crate) fn resolved_sonic_lib(cache_contents: &str) -> Option<PathBuf> {
    resolved_system_lib(
        cache_contents,
        "USE_LIBSONIC:BOOL=",
        "SONIC_LIB:FILEPATH=",
        "SONIC_INC:PATH=",
    )
}

fn resolved_system_lib(
    cache_contents: &str,
    use_key: &str,
    lib_key: &str,
    inc_key: &str,
) -> Option<PathBuf> {
    let mut enabled = false;
    let mut lib = None;
    let mut inc = None;

    for line in cache_contents.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix(use_key) {
            enabled = cmake_bool_is_true(value);
        } else if let Some(value) = line.strip_prefix(lib_key) {
            lib = Some(value);
        } else if let Some(value) = line.strip_prefix(inc_key) {
            inc = Some(value);
        }
    }

    let is_resolved = |value: Option<&str>| matches!(value, Some(v) if !v.is_empty() && !v.ends_with("-NOTFOUND"));

    if enabled && is_resolved(lib) && is_resolved(inc) {
        lib.map(PathBuf::from)
    } else {
        None
    }
}

fn emit_system_lib_link_directives(lib_path: &Path, default_name: &str) {
    if let Some(dir) = lib_path.parent() {
        println!("cargo:rustc-link-search=native={}", dir.display());
    }
    let kind = if lib_path.extension().and_then(|ext| ext.to_str()) == Some("a") {
        "static"
    } else {
        "dylib"
    };
    let name = lib_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .and_then(|stem| stem.strip_prefix("lib"))
        .unwrap_or(default_name);
    println!("cargo:rustc-link-lib={kind}={name}");
}

fn main() {
    println!("cargo:rustc-link-lib=speechPlayer");
    println!("cargo:rustc-link-lib=espeak-ng");
    println!("cargo:rustc-link-lib=ucd");
    let target = env::var("TARGET").unwrap();
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    let target_dir = get_cargo_target_dir().unwrap();
    let espeak_dst = out_dir.join("espeak-ng");
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("Failed to get CARGO_MANIFEST_DIR");
    let espeak_src = Path::new(&manifest_dir).join("espeak-ng");
    let bundle_path = Path::new(&manifest_dir)
        .join("bundled")
        .join("espeak-ng.tar.xz");
    let build_shared_libs = false;

    let build_shared_libs = std::env::var("ESPEAK_BUILD_SHARED_LIBS")
        .map(|v| v == "1")
        .unwrap_or(build_shared_libs);
    let profile = env::var("ESPEAK_LIB_PROFILE").unwrap_or("Release".to_string());
    let static_crt = env::var("ESPEAK_STATIC_CRT")
        .map(|v| v == "1")
        .unwrap_or(false);

    debug_log!("TARGET: {}", target);
    debug_log!("CARGO_MANIFEST_DIR: {}", manifest_dir);
    debug_log!("TARGET_DIR: {}", target_dir.display());
    debug_log!("OUT_DIR: {}", out_dir.display());
    debug_log!("BUILD_SHARED: {}", build_shared_libs);

    if !espeak_dst.exists() {
        match resolve_espeak_ng_source(&espeak_src, &bundle_path) {
            EspeakNgSource::Directory(src) => {
                debug_log!("Copy {} to {}", src.display(), espeak_dst.display());
                copy_folder(src, &espeak_dst);
            }
            EspeakNgSource::Bundle(bundle) => {
                debug_log!("Extract {} to {}", bundle.display(), espeak_dst.display());
                extract_xz_tar_bundle(bundle, &espeak_dst);
            }
        }
    }
    unsafe {
        env::set_var(
            "CMAKE_BUILD_PARALLEL_LEVEL",
            std::thread::available_parallelism()
                .unwrap()
                .get()
                .to_string(),
        );
    }

    let mut bindgen_builder = bindgen::Builder::default()
        .header("wrapper.h")
        .clang_arg(format!("-I{}", out_dir.display()))
        .clang_arg(format!("-I{}", espeak_dst.display()))
        .clang_arg(format!(
            "-I{}",
            espeak_dst.join("src").join("include").display()
        ));

    if target_os == "android" {
        let ndk_home = android_ndk_home();
        let sysroot = android_sysroot(&ndk_home);
        bindgen_builder = bindgen_builder
            .clang_arg(format!("--target={target}{}", android_api_level()))
            .clang_arg(format!("--sysroot={}", sysroot.display()));
    }

    let bindings = bindgen_builder
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("Failed to generate bindings");

    let bindings_path = out_dir.join("bindings.rs");
    bindings
        .write_to_file(bindings_path)
        .expect("Failed to write bindings");
    println!("cargo:rerun-if-changed=wrapper.h");
    println!("cargo:rerun-if-changed=./espeak-ng");
    println!("cargo:rerun-if-changed={}", bundle_path.display());

    debug_log!("Bindings Created");

    let mut config = Config::new(&espeak_dst);

    config.define(
        "BUILD_SHARED_LIBS",
        if build_shared_libs { "ON" } else { "OFF" },
    );

    if target_os == "windows" {
        config.static_crt(static_crt);
    }

    if target_os == "macos" {
        config.define("USE_LIBPCAUDIO", "OFF");
    }

    if target_os == "android" {
        let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();
        let android_platform = format!("android-{}", android_api_level());
        config
            .define(
                "CMAKE_TOOLCHAIN_FILE",
                android_toolchain_file(&android_ndk_home()),
            )
            .define("ANDROID_ABI", android_abi(&target_arch))
            .define("ANDROID_PLATFORM", android_platform)
            .define("ANDROID_STL", "c++_shared");
    }

    config
        .profile(&profile)
        .define("ENABLE_TESTS", "OFF")
        .define(
            "COMPILE_INTONATIONS",
            if cfg!(feature = "compile-espeak-intonations") {
                "ON"
            } else {
                "OFF"
            },
        )
        .very_verbose(std::env::var("CMAKE_VERBOSE").is_ok())
        .always_configure(false);

    let bindings_dir = config.build();

    copy_espeak_ng_data_next_to_binary(&out_dir, &target_dir);

    println!("cargo:rustc-link-search={}", out_dir.join("lib").display());
    println!(
        "cargo:rustc-link-search={}",
        out_dir.join("build/src/speechPlayer").display()
    );
    println!(
        "cargo:rustc-link-search={}",
        out_dir.join("build/src/ucd-tools").display()
    );
    println!("cargo:rustc-link-search={}", bindings_dir.display());

    if target_os == "windows" {
        println!(
            "cargo:rustc-link-search={}",
            out_dir.join("build/src/speechPlayer/Release").display()
        );
        println!(
            "cargo:rustc-link-search={}",
            out_dir.join("build/src/ucd-tools/Release").display()
        );
    }

    if target_os == "macos" {
        println!("cargo:rustc-link-lib=framework=Foundation");
        println!("cargo:rustc-link-lib=c++");
    }

    if target_os == "android" {
        println!("cargo:rustc-link-lib=c++_shared");
    }

    let espeak_libs_kind = if build_shared_libs { "dylib" } else { "static" };
    let espeak_libs = extract_lib_names(&out_dir, build_shared_libs, &target_os);

    for lib in espeak_libs {
        debug_log!(
            "LINK {}",
            format!("cargo:rustc-link-lib={}={}", espeak_libs_kind, lib)
        );
        println!("cargo:rustc-link-lib={}={}", espeak_libs_kind, lib);
    }

    if target_os == "windows" && cfg!(debug_assertions) {
        println!("cargo:rustc-link-lib=dylib=msvcrtd");
    }

    if target_os == "linux" {
        println!("cargo:rustc-link-lib=dylib=stdc++");
        let cmake_cache = out_dir.join("build").join("CMakeCache.txt");
        let cache_contents = std::fs::read_to_string(&cmake_cache).ok();

        if let Some(pcaudio_lib) = cache_contents.as_deref().and_then(resolved_pcaudio_lib) {
            emit_system_lib_link_directives(&pcaudio_lib, "pcaudio");
        }
        if let Some(sonic_lib) = cache_contents.as_deref().and_then(resolved_sonic_lib) {
            emit_system_lib_link_directives(&sonic_lib, "sonic");
        }
    }

    if target.contains("apple")
        && let Some(path) = macos_link_search_path()
    {
        println!("cargo:rustc-link-lib=clang_rt.osx");
        println!("cargo:rustc-link-search={}", path);
    }

    if build_shared_libs {
        let libs_assets = extract_lib_assets(&out_dir, &target_os);
        for asset in libs_assets {
            let asset_clone = asset.clone();
            let filename = asset_clone.file_name().unwrap();
            let filename = filename.to_str().unwrap();
            let dst = target_dir.join(filename);
            debug_log!("HARD LINK {} TO {}", asset.display(), dst.display());
            if !dst.exists() {
                std::fs::hard_link(asset.clone(), dst).unwrap();
            }

            if target_dir.join("examples").exists() {
                let dst = target_dir.join("examples").join(filename);
                debug_log!("HARD LINK {} TO {}", asset.display(), dst.display());
                if !dst.exists() {
                    std::fs::hard_link(asset.clone(), dst).unwrap();
                }
            }

            let dst = target_dir.join("deps").join(filename);
            debug_log!("HARD LINK {} TO {}", asset.display(), dst.display());
            if !dst.exists() {
                std::fs::hard_link(asset.clone(), dst).unwrap();
            }
        }
    }
}
