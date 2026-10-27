use cmake::Config;
use glob::glob;
use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};

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

fn copy_dir_recursive(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).expect("Failed to create destination directory");
    for entry in std::fs::read_dir(src).expect("Failed to read source directory") {
        let entry = entry.expect("Failed to read directory entry");
        let file_type = entry.file_type().expect("Failed to read entry file type");
        let dst_path = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&entry.path(), &dst_path);
        } else {
            std::fs::copy(entry.path(), &dst_path).expect("Failed to copy file");
        }
    }
}

pub(crate) fn copy_folder(src: &Path, dst: &Path) {
    assert!(src.exists(), "copy source not found at {}", src.display());

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

    copy_dir_recursive(src, &tmp_dst);

    std::fs::rename(&tmp_dst, dst).expect("Failed to move completed copy into place");
}

pub(crate) struct SourceLock {
    pub(crate) commit: String,
    pub(crate) sha256: String,
}

/// Reads the `key = value` lines of espeak-ng.lock.
pub(crate) fn parse_source_lock(text: &str) -> Result<SourceLock, String> {
    let value = |key: &str| {
        text.lines()
            .filter_map(|line| line.split_once('='))
            .find(|(k, _)| k.trim() == key)
            .map(|(_, v)| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .ok_or_else(|| format!("espeak-ng.lock has no `{key}`"))
    };
    Ok(SourceLock {
        commit: value("commit")?,
        sha256: value("sha256")?,
    })
}

pub(crate) fn source_url(commit: &str) -> String {
    format!("https://github.com/espeak-ng/espeak-ng/archive/{commit}.tar.gz")
}

/// Checks the tar stream inside a gzip archive. GitHub does not promise stable gzip bytes for
/// `/archive/<commit>.tar.gz`, but the tar stream of a fixed commit does not change.
pub(crate) fn verify_tar_sha256(gz: &[u8], expected: &str) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut tar = Vec::new();
    flate2::read::GzDecoder::new(gz)
        .read_to_end(&mut tar)
        .map_err(|e| format!("could not decompress the archive: {e}"))?;
    let actual: String = Sha256::digest(&tar)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(format!("tar sha256 is {actual}, expected {expected}"))
    }
}

const DOWNLOAD_ATTEMPTS: u64 = 3;
const DOWNLOAD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);
const MAX_DOWNLOAD_BYTES: u64 = 256 * 1024 * 1024;

fn download_once(url: &str) -> Result<Vec<u8>, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(DOWNLOAD_TIMEOUT))
        .build()
        .into();
    let mut response = agent.get(url).call().map_err(|e| e.to_string())?;
    response
        .body_mut()
        .with_config()
        .limit(MAX_DOWNLOAD_BYTES)
        .read_to_vec()
        .map_err(|e| e.to_string())
}

fn download(url: &str) -> Result<Vec<u8>, String> {
    let mut last_error = String::new();
    for attempt in 1..=DOWNLOAD_ATTEMPTS {
        match download_once(url) {
            Ok(bytes) => return Ok(bytes),
            Err(e) => last_error = e,
        }
        std::thread::sleep(std::time::Duration::from_secs(attempt));
    }
    Err(format!(
        "downloading {url} failed after {DOWNLOAD_ATTEMPTS} attempts: {last_error}"
    ))
}

fn fetch_pinned_archive(lock: &SourceLock) -> Vec<u8> {
    let bytes = download(&source_url(&lock.commit)).unwrap_or_else(|e| {
        panic!("{e}; set ESPEAK_NG_SOURCE to an espeak-ng checkout or .tar.gz to build offline")
    });
    verify_tar_sha256(&bytes, &lock.sha256)
        .unwrap_or_else(|e| panic!("espeak-ng {} tarball: {e}", lock.commit));
    bytes
}

pub(crate) enum EspeakNgSource {
    Directory(PathBuf),
    Archive(PathBuf),
    Pinned,
}

pub(crate) fn resolve_espeak_ng_source(override_path: Option<&Path>) -> EspeakNgSource {
    match override_path {
        None => EspeakNgSource::Pinned,
        Some(path) if path.is_dir() => EspeakNgSource::Directory(path.to_path_buf()),
        Some(path) if path.is_file() => EspeakNgSource::Archive(path.to_path_buf()),
        Some(path) => panic!(
            "ESPEAK_NG_SOURCE {} is neither a directory nor a .tar.gz file",
            path.display()
        ),
    }
}

const PINNED_STAMP: &str = ".pinned-sha256";

fn remove_stale(dst: &Path) {
    if dst.exists() {
        std::fs::remove_dir_all(dst).expect("Failed to remove stale materialized espeak-ng source");
    }
}

fn materialize_pinned(lock_text: &str, dst: &Path) {
    let lock = parse_source_lock(lock_text).unwrap_or_else(|e| panic!("{e}"));
    let stamp = dst.join(PINNED_STAMP);
    if std::fs::read_to_string(&stamp).is_ok_and(|s| s == lock.sha256) {
        return;
    }
    remove_stale(dst);
    extract_tar_gz(&fetch_pinned_archive(&lock), dst);
    std::fs::write(&stamp, &lock.sha256).expect("Failed to record the pinned espeak-ng checksum");
}

pub(crate) fn materialize_espeak_ng_source(
    override_path: Option<&Path>,
    lock_text: &str,
    dst: &Path,
) {
    match resolve_espeak_ng_source(override_path) {
        EspeakNgSource::Pinned => materialize_pinned(lock_text, dst),
        EspeakNgSource::Directory(src) => {
            remove_stale(dst);
            copy_folder(&src, dst);
        }
        EspeakNgSource::Archive(path) => {
            remove_stale(dst);
            let bytes = std::fs::read(&path)
                .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
            extract_tar_gz(&bytes, dst);
        }
    }
}

/// Drops the leading `espeak-ng-<commit>/` component that GitHub archives add; anything
/// that would leave the destination is skipped.
pub(crate) fn stripped_path(path: &Path) -> Option<PathBuf> {
    let mut components = path.components();
    components.next()?;
    let rest: PathBuf = components.collect();
    let safe = rest
        .components()
        .all(|c| matches!(c, std::path::Component::Normal(_)));
    (safe && !rest.as_os_str().is_empty()).then_some(rest)
}

/// Whether a symlink at `relative` (inside the destination) points somewhere inside it too.
/// `..` is accepted only as a leading prefix: after a normal component it could step out of a
/// directory that is itself an extracted symlink, which a lexical count cannot see.
fn symlink_stays_inside(relative: &Path, link: &Path) -> bool {
    use std::path::Component::{CurDir, Normal, ParentDir};
    // Directories between the destination root and the link's own directory.
    let mut depth = relative.components().count().saturating_sub(1);
    let mut descended = false;
    for component in link.components() {
        match component {
            Normal(_) => descended = true,
            CurDir => {}
            ParentDir if depth > 0 && !descended => depth -= 1,
            _ => return false,
        }
    }
    true
}

/// `Entry::unpack` does not validate link targets the way `unpack_in` does, and the stripped
/// destination rules out `unpack_in`, so only files, directories and contained symlinks pass.
pub(crate) fn entry_is_safe(
    kind: tar::EntryType,
    relative: &Path,
    link: Option<&Path>,
) -> Result<(), String> {
    if kind.is_file() || kind.is_dir() {
        return Ok(());
    }
    match link {
        Some(link) if kind.is_symlink() && symlink_stays_inside(relative, link) => Ok(()),
        Some(link) if kind.is_symlink() => Err(format!(
            "symlink {} points outside the archive: {}",
            relative.display(),
            link.display()
        )),
        _ => Err(format!(
            "unsupported {kind:?} entry in the archive: {}",
            relative.display()
        )),
    }
}

/// Refuses an entry written beneath a symlink extracted earlier, which would move it, and what
/// the symlink checks assume about `..`, somewhere else.
pub(crate) fn is_below_a_symlink(relative: &Path, symlinks: &HashSet<PathBuf>) -> bool {
    relative
        .ancestors()
        .skip(1)
        .any(|ancestor| symlinks.contains(ancestor))
}

fn unpack_stripped(archive: &[u8], dst: &Path) -> std::io::Result<()> {
    let invalid = |e: String| std::io::Error::new(std::io::ErrorKind::InvalidData, e);
    let mut symlinks = HashSet::new();
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive));
    for entry in tar.entries()? {
        let mut entry = entry?;
        let Some(relative) = stripped_path(&entry.path()?) else {
            continue;
        };
        let kind = entry.header().entry_type();
        let link = entry.link_name()?.map(|l| l.into_owned());
        entry_is_safe(kind, &relative, link.as_deref()).map_err(invalid)?;
        if is_below_a_symlink(&relative, &symlinks) {
            return Err(invalid(format!(
                "{} is below a symlink in the archive",
                relative.display()
            )));
        }
        if kind.is_symlink() {
            symlinks.insert(relative.clone());
        }
        let target = dst.join(relative);
        if let Some(dir) = target.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // GitHub stamps every file with the commit time. Keeping it would let CMake treat sources
        // from a re-pinned commit as older than the objects already built in OUT_DIR.
        entry.set_preserve_mtime(false);
        entry.unpack(&target)?;
    }
    Ok(())
}

pub(crate) fn extract_tar_gz(archive: &[u8], dst: &Path) {
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

    std::fs::create_dir_all(&tmp_dst).expect("Failed to create extraction temp directory");
    if let Err(e) = unpack_stripped(archive, &tmp_dst) {
        let _ = std::fs::remove_dir_all(&tmp_dst);
        panic!("failed to unpack the espeak-ng archive: {e}");
    }

    std::fs::rename(&tmp_dst, dst).expect("Failed to move extracted archive into place");
}

const ESPEAK_NG_DATA_DIR_NAME: &str = "espeak-ng-data";

pub(crate) fn espeak_ng_data_dir_const_source(data_dir: Option<&Path>) -> String {
    match data_dir {
        Some(dir) => format!(
            "pub const ESPEAK_NG_DATA_DIR: Option<&str> = Some({:?});",
            dir.display().to_string()
        ),
        None => "pub const ESPEAK_NG_DATA_DIR: Option<&str> = None;".to_string(),
    }
}

/// An empty install otherwise surfaces only as a linker error, e.g. when a distro's CMake
/// installs to lib64 instead.
pub(crate) fn ensure_libs_installed(
    libs: Vec<String>,
    libs_dir: &Path,
) -> Result<Vec<String>, String> {
    if libs.is_empty() {
        return Err(format!(
            "no espeak-ng libraries were installed under {}; check where CMake put them",
            libs_dir.display()
        ));
    }
    Ok(libs)
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

pub(crate) fn extract_lib_assets(out_dir: &Path, target_os: &str) -> Vec<PathBuf> {
    let shared_lib_pattern = if target_os == "windows" {
        "*.dll"
    } else if target_os == "macos" {
        "*.dylib"
    } else {
        "*.so"
    };

    // CMake installs a Windows DLL as a runtime artifact into bin, not lib.
    let asset_dirs: &[&str] = if target_os == "windows" {
        &["lib", "bin"]
    } else {
        &["lib"]
    };
    let mut files = Vec::new();

    for dir in asset_dirs {
        let pattern = out_dir.join(dir).join(shared_lib_pattern);
        debug_log!("Extract lib assets {}", pattern.display());
        for entry in glob(pattern.to_str().unwrap()).unwrap() {
            match entry {
                Ok(path) => files.push(path),
                Err(e) => eprintln!("cargo:warning=error={}", e),
            }
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

pub(crate) fn parse_clang_libraries_dir(stdout: &str) -> Option<String> {
    for line in stdout.lines() {
        if line.contains("libraries: =") {
            let path = line.split('=').nth(1)?;
            return Some(format!("{}/lib/darwin", path));
        }
    }
    None
}

fn macos_link_search_path() -> Option<String> {
    let compiler = cc::Build::new().get_compiler();
    let output = compiler
        .to_command()
        .arg("--print-search-dirs")
        .output()
        .ok()?;
    if !output.status.success() {
        println!(
            "failed to run '{:?} --print-search-dirs', continuing without a link search path",
            compiler.path()
        );
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let result = parse_clang_libraries_dir(&stdout);
    if result.is_none() {
        println!("failed to determine link search path, continuing without it");
    }
    result
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

pub(crate) fn system_link_libs(target_os: &str) -> &'static [&'static str] {
    match target_os {
        "windows" => &["advapi32"],
        _ => &[],
    }
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

pub(crate) const BUILD_CONFIG_ENV_VARS: &[&str] = &[
    "ESPEAK_BUILD_SHARED_LIBS",
    "ESPEAK_LIB_PROFILE",
    "ESPEAK_STATIC_CRT",
    "ESPEAK_NG_SOURCE",
    "ANDROID_NDK_HOME",
    "ANDROID_NDK_ROOT",
    "NDK_HOME",
    "ANDROID_PLATFORM",
];

// docs.rs builds without network, so it documents these checked-in bindings and links nothing.
fn use_docsrs_bindings(manifest_dir: &Path, out_dir: &Path) {
    std::fs::copy(
        manifest_dir.join("docsrs").join("bindings.rs"),
        out_dir.join("bindings.rs"),
    )
    .expect("Failed to copy the docs.rs bindings");
    std::fs::write(
        out_dir.join("espeak_ng_data_dir.rs"),
        espeak_ng_data_dir_const_source(None),
    )
    .expect("Failed to write the docs.rs data dir constant");
}

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("Failed to get CARGO_MANIFEST_DIR");
    if env::var_os("DOCS_RS").is_some() {
        use_docsrs_bindings(Path::new(&manifest_dir), &out_dir);
        return;
    }
    // Without `static=`, a staticlib depending on this crate omits both and fails to link.
    println!("cargo:rustc-link-lib=static=speechPlayer");
    println!("cargo:rustc-link-lib=espeak-ng");
    println!("cargo:rustc-link-lib=static=ucd");
    let target = env::var("TARGET").unwrap();
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();

    let target_dir = get_cargo_target_dir().ok();
    let espeak_dst = out_dir.join("espeak-ng");
    let lock_path = Path::new(&manifest_dir).join("espeak-ng.lock");
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
    debug_log!("TARGET_DIR: {:?}", target_dir);
    debug_log!("OUT_DIR: {}", out_dir.display());
    debug_log!("BUILD_SHARED: {}", build_shared_libs);

    let lock_text = std::fs::read_to_string(&lock_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", lock_path.display()));
    let source_override = env::var_os("ESPEAK_NG_SOURCE").map(PathBuf::from);
    if let Some(path) = &source_override {
        // Cargo walks a directory for changes, so editing the override rebuilds from it.
        println!("cargo:rerun-if-changed={}", path.display());
    }
    debug_log!(
        "Materializing espeak-ng source (override {:?}) into {}",
        source_override,
        espeak_dst.display()
    );
    materialize_espeak_ng_source(source_override.as_deref(), &lock_text, &espeak_dst);
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
    println!("cargo:rerun-if-changed={}", lock_path.display());
    for var in BUILD_CONFIG_ENV_VARS {
        println!("cargo:rerun-if-env-changed={var}");
    }

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

    // CMake builds iOS executables as app bundles, and espeak-ng's install() has no BUNDLE DESTINATION.
    if target_os == "ios" {
        config.define("CMAKE_MACOSX_BUNDLE", "OFF");
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

    // espeak-ng includes GNUInstallDirs, which installs to lib64 on RHEL-family systems, but
    // extract_lib_names and the link search below read OUT_DIR/lib.
    config
        .profile(&profile)
        .define("CMAKE_INSTALL_LIBDIR", "lib")
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

    let espeak_ng_data_parent = out_dir.join("share");
    let data_dir = espeak_ng_data_parent
        .join(ESPEAK_NG_DATA_DIR_NAME)
        .exists()
        .then_some(espeak_ng_data_parent.as_path());
    std::fs::write(
        out_dir.join("espeak_ng_data_dir.rs"),
        espeak_ng_data_dir_const_source(data_dir),
    )
    .expect("Failed to write generated espeak_ng_data_dir.rs");

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
    let espeak_libs = ensure_libs_installed(
        extract_lib_names(&out_dir, build_shared_libs, &target_os),
        &out_dir.join("lib"),
    )
    .unwrap_or_else(|e| panic!("{e}"));

    for lib in espeak_libs {
        debug_log!(
            "LINK {}",
            format!("cargo:rustc-link-lib={}={}", espeak_libs_kind, lib)
        );
        println!("cargo:rustc-link-lib={}={}", espeak_libs_kind, lib);
    }

    for lib in system_link_libs(&target_os) {
        println!("cargo:rustc-link-lib=dylib={lib}");
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

    if target_os == "macos"
        && let Some(path) = macos_link_search_path()
    {
        println!("cargo:rustc-link-lib=clang_rt.osx");
        println!("cargo:rustc-link-search={}", path);
    }

    if build_shared_libs {
        let target_dir = target_dir.expect(
            "ESPEAK_BUILD_SHARED_LIBS=1 requires a discoverable cargo target directory, but none was found (unsupported build layout for shared-lib output placement)",
        );
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
