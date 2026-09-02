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

fn copy_folder(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).expect("Failed to create dst directory");
    if cfg!(unix) {
        std::process::Command::new("cp")
            .arg("-rf")
            .arg(src)
            .arg(dst.parent().unwrap())
            .status()
            .expect("Failed to execute cp command");
    }

    if cfg!(windows) {
        std::process::Command::new("robocopy.exe")
            .arg("/e")
            .arg(src)
            .arg(dst)
            .status()
            .expect("Failed to execute robocopy command");
    }
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

    // Process the libraries based on the pattern
    for entry in glob(pattern.to_str().unwrap()).unwrap() {
        match entry {
            Ok(path) => {
                let stem = path.file_stem().unwrap();
                let stem_str = stem.to_str().unwrap();

                // Remove the "lib" prefix if present
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

/// Maps a Rust `CARGO_CFG_TARGET_ARCH` to the ABI name the Android NDK's
/// CMake toolchain file expects for `ANDROID_ABI`.
fn android_abi(target_arch: &str) -> &'static str {
    match target_arch {
        "aarch64" => "arm64-v8a",
        "arm" => "armeabi-v7a",
        "x86" => "x86",
        "x86_64" => "x86_64",
        other => panic!("unsupported Android target arch: {other}"),
    }
}

/// Resolves the Android NDK installation directory from the first of
/// `ANDROID_NDK_HOME`, `ANDROID_NDK_ROOT`, or `NDK_HOME` that is set.
fn android_ndk_home() -> String {
    ["ANDROID_NDK_HOME", "ANDROID_NDK_ROOT", "NDK_HOME"]
        .into_iter()
        .find_map(|var| env::var(var).ok())
        .expect(
            "targeting Android requires ANDROID_NDK_HOME (or ANDROID_NDK_ROOT/NDK_HOME) \
             to be set to an Android NDK installation",
        )
}

/// Locates the Android NDK's CMake toolchain file under `ndk_home`.
///
/// The `cmake` crate has no built-in Android support: without an explicit
/// `CMAKE_TOOLCHAIN_FILE`, CMake's configure step fails outright when
/// cross-compiling for Android, so this must be set up manually the same
/// way `ndk-build`/Gradle's CMake integration does.
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

/// The NDK's own host-tag naming for the prebuilt toolchain/sysroot
/// directory, keyed off the machine build.rs itself is *running* on (not
/// the Android target it's cross-compiling for).
fn ndk_host_tag() -> &'static str {
    if cfg!(target_os = "macos") {
        "darwin-x86_64"
    } else if cfg!(target_os = "windows") {
        "windows-x86_64"
    } else {
        "linux-x86_64"
    }
}

/// Locates the NDK's unified sysroot (bionic libc + Android-specific
/// headers) under `ndk_home`.
///
/// bindgen/clang otherwise silently fall back to the host's own system
/// headers (e.g. glibc under `/usr/include`) when parsing `wrapper.h`
/// for a foreign target, which fail in target-specific ways the host
/// toolchain was never meant to resolve (see issue #9). An explicit NDK
/// sysroot avoids the host headers entirely.
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

/// The Android API level to target, from `ANDROID_PLATFORM` (accepting
/// either `21` or `android-21`), defaulting to 21.
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

/// Mirrors espeak-ng's own `HAVE_LIBPCAUDIO AND USE_LIBPCAUDIO` gate
/// (src/libespeak-ng/CMakeLists.txt) by reading CMake's cache instead of
/// re-running its `find_library`/`find_path` detection. Returns the absolute
/// path CMake resolved for libpcaudio, so callers can point the Rust linker
/// at the exact location and library kind CMake linked against instead of
/// guessing a bare `-lpcaudio` will resolve in the default search path.
/// Requires PCAUDIO_LIB/PCAUDIO_INC to actually be resolved (not
/// `*-NOTFOUND`) in addition to `USE_LIBPCAUDIO:BOOL=ON`, since that flag
/// alone can be stale (CACHE'd from an earlier configure where the library
/// was present) while the resolved paths were invalidated since.
/// CMake's `if()` boolean grammar (case-insensitive): true is `ON`, `YES`,
/// `TRUE`, `Y`, or a non-zero number; everything else (including `OFF`,
/// `NO`, `FALSE`, `N`, `IGNORE`, `NOTFOUND`, a `*-NOTFOUND` suffix, or empty)
/// is false. `option()`-declared cache entries persist whatever spelling was
/// used to set them (verified: `-DVAR=TRUE` is not normalized to `ON` in
/// CMakeCache.txt), so a literal `== "ON"` check misses valid true values.
fn cmake_bool_is_true(value: &str) -> bool {
    let upper = value.to_ascii_uppercase();
    match upper.as_str() {
        "ON" | "YES" | "TRUE" | "Y" => true,
        "OFF" | "NO" | "FALSE" | "N" | "IGNORE" | "NOTFOUND" | "" => false,
        _ if upper.ends_with("-NOTFOUND") => false,
        _ => upper.parse::<i64>().is_ok_and(|n| n != 0),
    }
}

fn resolved_pcaudio_lib(cache_contents: &str) -> Option<PathBuf> {
    resolved_system_lib(
        cache_contents,
        "USE_LIBPCAUDIO:BOOL=",
        "PCAUDIO_LIB:FILEPATH=",
        "PCAUDIO_INC:PATH=",
    )
}

/// Mirrors espeak-ng's own `HAVE_LIBSONIC AND USE_LIBSONIC` gate
/// (src/libespeak-ng/CMakeLists.txt) the same way [`resolved_pcaudio_lib`]
/// mirrors the pcaudio one. Unlike pcaudio, when no system libsonic is found
/// `deps.cmake` falls back to fetching and compiling sonic in-tree as an
/// OBJECT library, whose object files get embedded directly into espeak-ng's
/// own static lib — no extra linker flag needed for that case. That fallback
/// only `set()`s `SONIC_LIB` to the bare in-tree target name (`sonic`)
/// locally within the CMake run; it never overwrites the cached
/// `SONIC_LIB:FILEPATH` entry `find_library` originally wrote, so the
/// `*-NOTFOUND` check below naturally distinguishes "linked as a real system
/// library" from "compiled in-tree and already embedded".
fn resolved_sonic_lib(cache_contents: &str) -> Option<PathBuf> {
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

/// Emits the `cargo:rustc-link-search`/`cargo:rustc-link-lib` directives for
/// a system library CMake resolved to `lib_path`, deriving the search
/// directory, kind (static vs. dylib), and bare library name from the
/// resolved file itself rather than assuming a search path or extension.
/// `default_name` is used only if the resolved file's name is unexpectedly
/// malformed (no `lib` prefix or no stem at all).
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

#[cfg(test)]
mod tests {
    use super::{resolved_pcaudio_lib, resolved_sonic_lib};
    use std::path::PathBuf;

    #[test]
    fn resolves_sonic_lib_path_when_cmake_found_system_libsonic() {
        // CMake found a system libsonic (SONIC_LIB resolves to a real path,
        // not the in-tree FetchContent fallback's bare `sonic` target name).
        let cache = "SONIC_LIB:FILEPATH=/usr/lib/x86_64-linux-gnu/libsonic.so\n\
                      SONIC_INC:PATH=/usr/include\n\
                      USE_LIBSONIC:BOOL=ON\n";
        assert_eq!(
            resolved_sonic_lib(cache),
            Some(PathBuf::from("/usr/lib/x86_64-linux-gnu/libsonic.so"))
        );
    }

    #[test]
    fn returns_none_for_sonic_when_use_libsonic_is_off() {
        let cache = "SONIC_LIB:FILEPATH=SONIC_LIB-NOTFOUND\n\
                      SONIC_INC:PATH=SONIC_INC-NOTFOUND\n\
                      USE_LIBSONIC:BOOL=OFF\n";
        assert_eq!(resolved_sonic_lib(cache), None);
    }

    #[test]
    fn returns_none_for_sonic_when_falling_back_to_in_tree_fetchcontent_build() {
        // deps.cmake's fallback path when no system libsonic is found: it
        // fetches and compiles sonic in-tree as an OBJECT library and links
        // it into espeak-ng's own static lib directly, so no extra `-lsonic`
        // is needed. SONIC_LIB's *cached* value stays SONIC_LIB-NOTFOUND
        // (the in-tree `set(SONIC_LIB sonic)` only shadows it locally within
        // that CMake run; it isn't written back to CMakeCache.txt), while
        // USE_LIBSONIC still defaults to ON via HAVE_LIBSONIC.
        let cache = "SONIC_LIB:FILEPATH=SONIC_LIB-NOTFOUND\n\
                      SONIC_INC:PATH=SONIC_INC-NOTFOUND\n\
                      USE_LIBSONIC:BOOL=ON\n";
        assert_eq!(resolved_sonic_lib(cache), None);
    }

    #[test]
    fn returns_none_for_sonic_when_cache_missing_keys() {
        let cache = "CMAKE_INSTALL_PREFIX:PATH=/out\n";
        assert_eq!(resolved_sonic_lib(cache), None);
    }

    #[test]
    fn treats_true_as_enabled_for_sonic_like_cmake_boolean_semantics() {
        let cache = "SONIC_LIB:FILEPATH=/usr/lib/x86_64-linux-gnu/libsonic.so\n\
                      SONIC_INC:PATH=/usr/include\n\
                      USE_LIBSONIC:BOOL=TRUE\n";
        assert_eq!(
            resolved_sonic_lib(cache),
            Some(PathBuf::from("/usr/lib/x86_64-linux-gnu/libsonic.so"))
        );
    }

    #[test]
    fn resolves_lib_path_when_cmake_enabled_and_found_pcaudio() {
        let cache = "PCAUDIO_LIB:FILEPATH=/usr/lib/x86_64-linux-gnu/libpcaudio.so\n\
                      PCAUDIO_INC:PATH=/usr/include\n\
                      USE_LIBPCAUDIO:BOOL=ON\n";
        assert_eq!(
            resolved_pcaudio_lib(cache),
            Some(PathBuf::from("/usr/lib/x86_64-linux-gnu/libpcaudio.so"))
        );
    }

    #[test]
    fn returns_none_when_use_libpcaudio_is_off() {
        let cache = "PCAUDIO_LIB:FILEPATH=PCAUDIO_LIB-NOTFOUND\n\
                      PCAUDIO_INC:PATH=PCAUDIO_INC-NOTFOUND\n\
                      USE_LIBPCAUDIO:BOOL=OFF\n";
        assert_eq!(resolved_pcaudio_lib(cache), None);
    }

    #[test]
    fn returns_none_when_lib_path_is_notfound_despite_use_libpcaudio_on() {
        // A stale USE_LIBPCAUDIO=ON cached from an earlier configure, while
        // PCAUDIO_LIB was never (re-)resolved this run.
        let cache = "PCAUDIO_LIB:FILEPATH=PCAUDIO_LIB-NOTFOUND\n\
                      PCAUDIO_INC:PATH=/usr/include\n\
                      USE_LIBPCAUDIO:BOOL=ON\n";
        assert_eq!(resolved_pcaudio_lib(cache), None);
    }

    #[test]
    fn returns_none_when_cache_missing_keys() {
        let cache = "CMAKE_INSTALL_PREFIX:PATH=/out\n";
        assert_eq!(resolved_pcaudio_lib(cache), None);
    }

    #[test]
    fn treats_true_as_enabled_like_cmake_boolean_semantics() {
        // A cache entry set via `-DUSE_LIBPCAUDIO=TRUE` persists that literal
        // string rather than CMake normalizing it to ON (verified against a
        // real `cmake` invocation, not assumed).
        let cache = "PCAUDIO_LIB:FILEPATH=/usr/lib/x86_64-linux-gnu/libpcaudio.so\n\
                      PCAUDIO_INC:PATH=/usr/include\n\
                      USE_LIBPCAUDIO:BOOL=TRUE\n";
        assert_eq!(
            resolved_pcaudio_lib(cache),
            Some(PathBuf::from("/usr/lib/x86_64-linux-gnu/libpcaudio.so"))
        );
    }

    #[test]
    fn treats_1_as_enabled_like_cmake_boolean_semantics() {
        let cache = "PCAUDIO_LIB:FILEPATH=/usr/lib/x86_64-linux-gnu/libpcaudio.so\n\
                      PCAUDIO_INC:PATH=/usr/include\n\
                      USE_LIBPCAUDIO:BOOL=1\n";
        assert_eq!(
            resolved_pcaudio_lib(cache),
            Some(PathBuf::from("/usr/lib/x86_64-linux-gnu/libpcaudio.so"))
        );
    }
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

    // Prepare espeak-ng source
    if !espeak_dst.exists() {
        debug_log!("Copy {} to {}", espeak_src.display(), espeak_dst.display());
        copy_folder(&espeak_src, &espeak_dst);
    }
    // Speed up build
    // SAFETY: build.rs runs single-threaded at this point, before any
    // concurrent env access (e.g. from the cmake crate's child process spawn).
    unsafe {
        env::set_var(
            "CMAKE_BUILD_PARALLEL_LEVEL",
            std::thread::available_parallelism()
                .unwrap()
                .get()
                .to_string(),
        );
    }

    // Bindings
    let mut bindgen_builder = bindgen::Builder::default()
        .header("wrapper.h")
        .clang_arg(format!("-I{}", espeak_dst.display()))
        .clang_arg(format!(
            "-I{}",
            espeak_dst.join("src").join("include").display()
        ));

    if target_os == "android" {
        // bindgen infers --target from Cargo's TARGET env var but has no
        // sysroot of its own, so clang falls back to the host's system
        // headers (glibc) while still targeting Android/bionic — a
        // mismatch that fails opaquely (see issue #9). Point it at the
        // NDK's own sysroot instead.
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

    // Write the generated bindings to an output file
    let bindings_path = out_dir.join("bindings.rs");
    bindings
        .write_to_file(bindings_path)
        .expect("Failed to write bindings");
    println!("cargo:rerun-if-changed=wrapper.h");
    println!("cargo:rerun-if-changed=./espeak-ng");

    debug_log!("Bindings Created");

    // Build with Cmake

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
        // c++_shared, not c++_static: the NDK recommends against a static
        // STL once an app links more than one native library, since each
        // static copy gets its own C++ runtime globals (locale, exception
        // state) and duplicate-symbol/ODR clashes across .so boundaries
        // follow. The app's packaging step must bundle the matching
        // libc++_shared.so from the NDK sysroot alongside the built .so.
        config
            .define(
                "CMAKE_TOOLCHAIN_FILE",
                android_toolchain_file(&android_ndk_home()),
            )
            .define("ANDROID_ABI", android_abi(&target_arch))
            .define("ANDROID_PLATFORM", android_platform)
            .define("ANDROID_STL", "c++_shared");
    }

    // General
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
        .very_verbose(std::env::var("CMAKE_VERBOSE").is_ok()) // Not verbose by default
        .always_configure(false);

    let bindings_dir = config.build();

    // Search paths
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

    // macOS
    if target_os == "macos" {
        println!("cargo:rustc-link-lib=framework=Foundation");
        println!("cargo:rustc-link-lib=c++");
    }

    // Android: speechPlayer is C++, so the NDK's C++ runtime must be linked
    // explicitly (rustc, not CMake, drives this final link step).
    if target_os == "android" {
        println!("cargo:rustc-link-lib=c++_shared");
    }

    // Link libraries
    let espeak_libs_kind = if build_shared_libs { "dylib" } else { "static" };
    let espeak_libs = extract_lib_names(&out_dir, build_shared_libs, &target_os);

    for lib in espeak_libs {
        debug_log!(
            "LINK {}",
            format!("cargo:rustc-link-lib={}={}", espeak_libs_kind, lib)
        );
        println!("cargo:rustc-link-lib={}={}", espeak_libs_kind, lib);
    }

    // Windows debug
    if target_os == "windows" && cfg!(debug_assertions) {
        println!("cargo:rustc-link-lib=dylib=msvcrtd");
    }

    // Linux
    if target_os == "linux" {
        println!("cargo:rustc-link-lib=dylib=stdc++");
        // CMake links espeak-ng against pcaudio/sonic only when it found the
        // system lib and headers (see USE_LIBPCAUDIO/USE_LIBSONIC); mirror
        // that decision here, pointing the linker at the exact path CMake
        // resolved, instead of guessing a bare `-lpcaudio`/`-lsonic` will
        // resolve in the default search path.
        let cmake_cache = out_dir.join("build").join("CMakeCache.txt");
        let cache_contents = std::fs::read_to_string(&cmake_cache).ok();

        if let Some(pcaudio_lib) = cache_contents.as_deref().and_then(resolved_pcaudio_lib) {
            emit_system_lib_link_directives(&pcaudio_lib, "pcaudio");
        }
        if let Some(sonic_lib) = cache_contents.as_deref().and_then(resolved_sonic_lib) {
            emit_system_lib_link_directives(&sonic_lib, "sonic");
        }
    }

    if target.contains("apple") {
        // On (older) OSX we need to link against the clang runtime,
        // which is hidden in some non-default path.
        //
        // More details at https://github.com/alexcrichton/curl-rust/issues/279.
        if let Some(path) = macos_link_search_path() {
            println!("cargo:rustc-link-lib=clang_rt.osx");
            println!("cargo:rustc-link-search={}", path);
        }
    }

    // copy DLLs to target
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

            // Copy DLLs to examples as well
            if target_dir.join("examples").exists() {
                let dst = target_dir.join("examples").join(filename);
                debug_log!("HARD LINK {} TO {}", asset.display(), dst.display());
                if !dst.exists() {
                    std::fs::hard_link(asset.clone(), dst).unwrap();
                }
            }

            // Copy DLLs to target/profile/deps as well for tests
            let dst = target_dir.join("deps").join(filename);
            debug_log!("HARD LINK {} TO {}", asset.display(), dst.display());
            if !dst.exists() {
                std::fs::hard_link(asset.clone(), dst).unwrap();
            }
        }
    }
}
