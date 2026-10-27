// Only the tested functions below are called here; the rest run when Cargo compiles build.rs itself.
#[allow(dead_code)]
#[path = "../build.rs"]
mod build_script;

use build_script::{
    BUILD_CONFIG_ENV_VARS, EspeakNgSource, copy_folder, ensure_libs_installed, entry_is_safe,
    espeak_ng_data_dir_const_source, extract_lib_assets, extract_tar_gz, is_below_a_symlink,
    materialize_espeak_ng_source, parse_clang_libraries_dir, parse_source_lock,
    resolve_espeak_ng_source, resolved_pcaudio_lib, resolved_sonic_lib, source_url, stripped_path,
    system_link_libs, verify_tar_sha256,
};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

#[test]
fn links_advapi32_on_windows_for_the_registry_lookups_espeak_makes() {
    assert_eq!(system_link_libs("windows"), ["advapi32"]);
}

#[test]
fn links_no_extra_system_libs_off_windows() {
    ["linux", "macos", "android"]
        .iter()
        .for_each(|os| assert!(system_link_libs(os).is_empty()));
}

#[test]
fn resolves_sonic_lib_path_when_cmake_found_system_libsonic() {
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

#[test]
fn extracts_the_libraries_search_dir_and_appends_lib_darwin() {
    let stdout = "programs: =/usr/bin\n\
                  libraries: =/usr/lib/gcc/x86_64-apple-darwin\n\
                  runtime: =/some/runtime\n";
    assert_eq!(
        parse_clang_libraries_dir(stdout),
        Some("/usr/lib/gcc/x86_64-apple-darwin/lib/darwin".to_string())
    );
}

#[test]
fn returns_none_when_no_libraries_line_is_present() {
    let stdout = "programs: =/usr/bin\nruntime: =/some/runtime\n";
    assert_eq!(parse_clang_libraries_dir(stdout), None);
}

#[test]
fn watches_every_env_var_the_build_script_reads_as_a_configuration_knob() {
    for var in [
        "ESPEAK_BUILD_SHARED_LIBS",
        "ESPEAK_LIB_PROFILE",
        "ESPEAK_STATIC_CRT",
        "ESPEAK_NG_SOURCE",
        "ANDROID_NDK_HOME",
        "ANDROID_NDK_ROOT",
        "NDK_HOME",
        "ANDROID_PLATFORM",
    ] {
        assert!(
            BUILD_CONFIG_ENV_VARS.contains(&var),
            "{var} should be registered for cargo:rerun-if-env-changed"
        );
    }
}

fn scratch_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "espeak-rs-sys-test-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id(),
    ))
}

#[test]
fn copy_folder_copies_files_into_a_fresh_destination() {
    let src = scratch_path("copy-src");
    let dst = scratch_path("copy-dst");
    std::fs::create_dir_all(src.join("nested")).unwrap();
    std::fs::write(src.join("nested").join("file.txt"), b"hello").unwrap();

    copy_folder(&src, &dst);

    assert_eq!(
        std::fs::read(dst.join("nested").join("file.txt")).unwrap(),
        b"hello"
    );

    std::fs::remove_dir_all(&src).unwrap();
    std::fs::remove_dir_all(&dst).unwrap();
}

#[test]
fn copy_folder_never_leaves_a_destination_behind_when_the_source_is_missing() {
    let src = scratch_path("missing-src");
    let dst = scratch_path("poisoned-dst");
    assert!(!src.exists());
    assert!(!dst.exists());

    let panicked = catch_unwind(AssertUnwindSafe(|| copy_folder(&src, &dst))).is_err();

    assert!(
        panicked,
        "copy_folder should panic when the source is missing"
    );
    assert!(
        !dst.exists(),
        "a failed copy must not leave the destination behind"
    );
}

#[test]
fn copy_folder_refuses_to_proceed_when_a_stale_tmp_copy_cannot_be_removed() {
    let src = scratch_path("stale-tmp-src");
    let dst = scratch_path("stale-tmp-dst");
    let tmp_dst = dst.parent().unwrap().join(format!(
        "{}.tmp",
        dst.file_name().unwrap().to_str().unwrap()
    ));
    std::fs::create_dir_all(src.join("nested")).unwrap();
    std::fs::write(src.join("nested").join("file.txt"), b"hello").unwrap();
    std::fs::create_dir_all(tmp_dst.parent().unwrap()).unwrap();
    std::fs::write(&tmp_dst, b"not a directory").unwrap();

    let panicked = catch_unwind(AssertUnwindSafe(|| copy_folder(&src, &dst))).is_err();

    assert!(
        panicked,
        "copy_folder should panic rather than copy into an unremovable stale tmp path"
    );
    assert!(
        !dst.exists(),
        "a refused copy must not leave the destination behind"
    );

    std::fs::remove_dir_all(&src).unwrap();
    std::fs::remove_file(&tmp_dst).unwrap();
}

#[test]
fn materialize_espeak_ng_source_replaces_a_stale_destination_with_updated_source_content() {
    let src = scratch_path("materialize-src");
    let dst = scratch_path("materialize-dst");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("version.txt"), b"new").unwrap();
    std::fs::create_dir_all(&dst).unwrap();
    std::fs::write(dst.join("version.txt"), b"stale").unwrap();

    materialize_espeak_ng_source(Some(&src), "unused", &dst);

    assert_eq!(std::fs::read(dst.join("version.txt")).unwrap(), b"new");

    std::fs::remove_dir_all(&src).unwrap();
    std::fs::remove_dir_all(&dst).unwrap();
}

#[test]
fn generates_a_some_constant_when_a_data_dir_is_given() {
    let source = espeak_ng_data_dir_const_source(Some(Path::new("/out/share")));
    assert_eq!(
        source,
        "pub const ESPEAK_NG_DATA_DIR: Option<&str> = Some(\"/out/share\");"
    );
}

#[test]
fn generates_a_none_constant_when_no_data_dir_is_given() {
    let source = espeak_ng_data_dir_const_source(None);
    assert_eq!(source, "pub const ESPEAK_NG_DATA_DIR: Option<&str> = None;");
}

#[test]
fn builds_the_pinned_source_when_no_override_is_given() {
    assert!(matches!(
        resolve_espeak_ng_source(None),
        EspeakNgSource::Pinned
    ));
}

#[test]
fn uses_an_override_directory_as_is() {
    let dir = scratch_path("override-dir");
    std::fs::create_dir_all(&dir).unwrap();

    match resolve_espeak_ng_source(Some(&dir)) {
        EspeakNgSource::Directory(src) => assert_eq!(src, dir),
        _ => panic!("expected a directory override"),
    }

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn extracts_an_override_file_as_an_archive() {
    let file = scratch_path("override-archive.tar.gz");
    std::fs::write(&file, b"archive").unwrap();

    match resolve_espeak_ng_source(Some(&file)) {
        EspeakNgSource::Archive(path) => assert_eq!(path, file),
        _ => panic!("expected an archive override"),
    }

    std::fs::remove_file(&file).unwrap();
}

#[test]
fn panics_when_the_override_path_does_not_exist() {
    let missing = scratch_path("override-missing");
    assert!(!missing.exists());

    let panicked = catch_unwind(AssertUnwindSafe(|| {
        resolve_espeak_ng_source(Some(&missing))
    }))
    .is_err();

    assert!(panicked);
}

#[test]
fn parses_the_commit_and_checksum_from_the_lock() {
    let lock = parse_source_lock("# comment\ncommit = abc123\nsha256 =  def456 \n").unwrap();
    assert_eq!(lock.commit, "abc123");
    assert_eq!(lock.sha256, "def456");
}

#[test]
fn rejects_a_lock_missing_a_key() {
    let error = parse_source_lock("commit = abc123\n").err().unwrap();
    assert!(error.contains("sha256"), "{error}");
}

#[test]
fn rejects_a_lock_with_an_empty_value() {
    assert!(parse_source_lock("commit = \nsha256 = def456\n").is_err());
}

#[test]
fn points_the_download_at_the_pinned_commit() {
    assert_eq!(
        source_url("abc123"),
        "https://github.com/espeak-ng/espeak-ng/archive/abc123.tar.gz"
    );
}

fn tar_sha256_hex(tar_bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(tar_bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[test]
fn accepts_a_matching_tar_checksum_in_either_case() {
    let tar_bytes = build_tar_fixture(&[("espeak-ng-abc/a.c", b"int a;")]);
    let known = tar_sha256_hex(&tar_bytes);
    let gz = gzip(&tar_bytes, flate2::Compression::fast());
    assert!(verify_tar_sha256(&gz, &known).is_ok());
    assert!(verify_tar_sha256(&gz, &known.to_uppercase()).is_ok());
}

#[test]
fn verifies_the_tar_stream_whatever_the_gzip_layout() {
    let tar_bytes = build_tar_fixture(&[("espeak-ng-abc/a.c", b"int a;")]);
    let known = tar_sha256_hex(&tar_bytes);
    let fast = gzip(&tar_bytes, flate2::Compression::fast());
    let best = gzip(&tar_bytes, flate2::Compression::best());
    assert_ne!(fast, best, "the fixtures need different gzip bytes");
    assert!(verify_tar_sha256(&fast, &known).is_ok());
    assert!(verify_tar_sha256(&best, &known).is_ok());
}

#[test]
fn rejects_a_mismatched_tar_checksum() {
    let gz = gzip(
        &build_tar_fixture(&[("espeak-ng-abc/a.c", b"tampered")]),
        flate2::Compression::fast(),
    );
    let error = verify_tar_sha256(&gz, "00").unwrap_err();
    assert!(error.contains("expected 00"), "{error}");
}

#[test]
fn rejects_an_archive_that_is_not_gzip() {
    let error = verify_tar_sha256(b"not a gzip stream", "00").unwrap_err();
    assert!(error.contains("decompress"), "{error}");
}

#[test]
fn strips_the_leading_archive_component() {
    assert_eq!(
        stripped_path(Path::new("espeak-ng-abc/src/a.c")),
        Some(PathBuf::from("src/a.c"))
    );
}

#[test]
fn skips_the_archive_root_and_unsafe_paths() {
    assert_eq!(stripped_path(Path::new("espeak-ng-abc")), None);
    assert_eq!(stripped_path(Path::new("espeak-ng-abc/")), None);
    assert_eq!(stripped_path(Path::new("pax_global_header")), None);
    assert_eq!(stripped_path(Path::new("espeak-ng-abc/../escape")), None);
}

fn build_tar_fixture(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut tar_bytes = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_bytes);
        for (path, content) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            header.set_mtime(0);
            header.set_cksum();
            builder.append_data(&mut header, path, *content).unwrap();
        }
        builder.finish().unwrap();
    }
    tar_bytes
}

fn gzip(bytes: &[u8], level: flate2::Compression) -> Vec<u8> {
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), level);
    std::io::Write::write_all(&mut gz, bytes).unwrap();
    gz.finish().unwrap()
}

fn build_tar_gz_fixture(entries: &[(&str, &[u8])]) -> Vec<u8> {
    gzip(&build_tar_fixture(entries), flate2::Compression::fast())
}

fn symlink_tar_gz_fixture(path: &str, target: &str) -> Vec<u8> {
    let mut tar_bytes = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_bytes);
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        header.set_mtime(0);
        builder.append_link(&mut header, path, target).unwrap();
        builder.finish().unwrap();
    }
    gzip(&tar_bytes, flate2::Compression::fast())
}

#[test]
fn allows_files_directories_and_symlinks_that_stay_inside() {
    let relative = Path::new("src/include/espeak/speak_lib.h");
    let inside = Path::new("../espeak-ng/speak_lib.h");
    assert!(entry_is_safe(tar::EntryType::Regular, relative, None).is_ok());
    assert!(entry_is_safe(tar::EntryType::Directory, relative, None).is_ok());
    assert!(entry_is_safe(tar::EntryType::Symlink, relative, Some(inside)).is_ok());
}

#[test]
fn refuses_symlinks_that_leave_the_destination() {
    let relative = Path::new("src/include/espeak/speak_lib.h");
    for escaping in ["../../../../etc/passwd", "/etc/passwd", "../../../../.."] {
        let result = entry_is_safe(tar::EntryType::Symlink, relative, Some(Path::new(escaping)));
        assert!(result.is_err(), "{escaping} should be refused");
    }
}

#[test]
fn refuses_dot_dot_after_a_normal_component() {
    // `a/s -> ..` is fine, but `b/t -> ../a/s/..` steps out of a symlinked directory.
    let relative = Path::new("b/t");
    for chained in ["../a/s/..", "x/..", "../x/../.."] {
        let result = entry_is_safe(tar::EntryType::Symlink, relative, Some(Path::new(chained)));
        assert!(result.is_err(), "{chained} should be refused");
    }
    let first = entry_is_safe(
        tar::EntryType::Symlink,
        Path::new("a/s"),
        Some(Path::new("..")),
    );
    assert!(first.is_ok());
}

#[test]
fn spots_entries_below_an_extracted_symlink() {
    let symlinks = std::collections::HashSet::from([PathBuf::from("a/s")]);
    assert!(is_below_a_symlink(Path::new("a/s/f"), &symlinks));
    assert!(is_below_a_symlink(Path::new("a/s/deep/f"), &symlinks));
    assert!(!is_below_a_symlink(Path::new("a/s"), &symlinks));
    assert!(!is_below_a_symlink(Path::new("a/other/f"), &symlinks));
}

#[test]
fn extract_tar_gz_refuses_to_write_through_a_symlinked_directory() {
    let dst = scratch_path("extract-write-through-dst");
    let mut tar_bytes = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_bytes);
        let mut link = tar::Header::new_gnu();
        link.set_entry_type(tar::EntryType::Symlink);
        link.set_size(0);
        link.set_mtime(0);
        builder
            .append_link(&mut link, "espeak-ng-abc/a/s", ".")
            .unwrap();
        let mut file = tar::Header::new_gnu();
        file.set_size(1);
        file.set_mode(0o644);
        file.set_mtime(0);
        file.set_cksum();
        builder
            .append_data(&mut file, "espeak-ng-abc/a/s/f", &b"x"[..])
            .unwrap();
        builder.finish().unwrap();
    }
    let archive = gzip(&tar_bytes, flate2::Compression::fast());

    let panicked = catch_unwind(AssertUnwindSafe(|| extract_tar_gz(&archive, &dst))).is_err();

    assert!(panicked, "writing below an extracted symlink should abort");
    assert!(
        !dst.exists(),
        "a refused archive must not leave the destination behind"
    );
}

#[test]
fn passes_installed_libraries_through() {
    let libs = vec!["espeak-ng".to_string(), "ucd".to_string()];
    let kept = ensure_libs_installed(libs.clone(), Path::new("/out/lib")).unwrap();
    assert_eq!(kept, libs);
}

#[test]
fn names_the_directory_when_no_libraries_were_installed() {
    let error = ensure_libs_installed(Vec::new(), Path::new("/out/lib")).unwrap_err();
    assert!(error.contains("/out/lib"), "{error}");
}

#[test]
fn refuses_hard_links_and_special_files() {
    let relative = Path::new("src/a.c");
    let link = Some(Path::new("src/b.c"));
    assert!(entry_is_safe(tar::EntryType::Link, relative, link).is_err());
    assert!(entry_is_safe(tar::EntryType::Fifo, relative, None).is_err());
    assert!(entry_is_safe(tar::EntryType::Char, relative, None).is_err());
}

#[test]
fn extracts_a_symlink_that_stays_inside() {
    let dst = scratch_path("extract-symlink-inside-dst");
    let archive = symlink_tar_gz_fixture("espeak-ng-abc/src/include/h.h", "../espeak-ng/h.h");

    extract_tar_gz(&archive, &dst);

    let link = dst.join("src").join("include").join("h.h");
    assert_eq!(
        std::fs::read_link(&link).unwrap(),
        Path::new("../espeak-ng/h.h")
    );

    std::fs::remove_dir_all(&dst).unwrap();
}

#[test]
fn extract_tar_gz_refuses_a_symlink_that_leaves_the_destination() {
    let dst = scratch_path("extract-symlink-escape-dst");
    let archive = symlink_tar_gz_fixture("espeak-ng-abc/escape", "../../../../etc/passwd");

    let panicked = catch_unwind(AssertUnwindSafe(|| extract_tar_gz(&archive, &dst))).is_err();

    assert!(panicked, "an escaping symlink should abort the build");
    assert!(
        !dst.exists(),
        "a refused archive must not leave the destination behind"
    );
}

#[test]
fn extracted_files_get_fresh_modification_times() {
    let dst = scratch_path("extract-mtime-dst");
    let archive = build_tar_gz_fixture(&[("espeak-ng-abc/a.c", b"int a;")]);

    extract_tar_gz(&archive, &dst);

    let age = std::time::SystemTime::now()
        .duration_since(
            std::fs::metadata(dst.join("a.c"))
                .unwrap()
                .modified()
                .unwrap(),
        )
        .unwrap();
    assert!(
        age < std::time::Duration::from_secs(300),
        "a file stamped with the archive's 1970 mtime would look older than previously built objects, got age {age:?}"
    );

    std::fs::remove_dir_all(&dst).unwrap();
}

#[test]
fn extracts_a_tar_gz_into_the_destination_without_the_top_level_directory() {
    let dst = scratch_path("extract-archive-dst");
    let archive = build_tar_gz_fixture(&[
        ("espeak-ng-abc/CMakeLists.txt", b"cmake stuff"),
        (
            "espeak-ng-abc/dictsource/en_list",
            b"english dictionary source",
        ),
    ]);

    extract_tar_gz(&archive, &dst);

    assert_eq!(
        std::fs::read(dst.join("CMakeLists.txt")).unwrap(),
        b"cmake stuff"
    );
    assert_eq!(
        std::fs::read(dst.join("dictsource").join("en_list")).unwrap(),
        b"english dictionary source"
    );

    std::fs::remove_dir_all(&dst).unwrap();
}

#[test]
fn extract_tar_gz_never_leaves_a_destination_behind_when_the_archive_is_corrupt() {
    let dst = scratch_path("extract-corrupt-archive-dst");

    let panicked = catch_unwind(AssertUnwindSafe(|| {
        extract_tar_gz(b"not a valid gzip stream", &dst)
    }))
    .is_err();

    assert!(
        panicked,
        "extract_tar_gz should panic when the archive isn't a valid gzip stream"
    );
    assert!(
        !dst.exists(),
        "a failed extraction must not leave the destination behind"
    );
}

#[test]
fn materializes_an_override_archive_over_a_stale_destination() {
    let archive_path = scratch_path("materialize-archive.tar.gz");
    let dst = scratch_path("materialize-archive-dst");
    std::fs::write(
        &archive_path,
        build_tar_gz_fixture(&[("espeak-ng-abc/version.txt", b"new")]),
    )
    .unwrap();
    std::fs::create_dir_all(&dst).unwrap();
    std::fs::write(dst.join("version.txt"), b"stale").unwrap();

    materialize_espeak_ng_source(Some(&archive_path), "unused", &dst);

    assert_eq!(std::fs::read(dst.join("version.txt")).unwrap(), b"new");

    std::fs::remove_file(&archive_path).unwrap();
    std::fs::remove_dir_all(&dst).unwrap();
}

fn out_dir_with(name: &str, files: &[&str]) -> PathBuf {
    let out = scratch_path(name);
    let _ = std::fs::remove_dir_all(&out);
    for file in files {
        let path = out.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"").unwrap();
    }
    out
}

#[test]
fn windows_runtime_dll_is_found_in_bin() {
    let out = out_dir_with(
        "assets-win-bin",
        &["bin/espeak-ng.dll", "lib/espeak-ng.lib"],
    );

    assert_eq!(
        extract_lib_assets(&out, "windows"),
        vec![out.join("bin/espeak-ng.dll")]
    );

    std::fs::remove_dir_all(&out).unwrap();
}

#[test]
fn windows_dll_in_lib_is_still_found() {
    let out = out_dir_with("assets-win-lib", &["lib/espeak-ng.dll"]);

    assert_eq!(
        extract_lib_assets(&out, "windows"),
        vec![out.join("lib/espeak-ng.dll")]
    );

    std::fs::remove_dir_all(&out).unwrap();
}

#[test]
fn linux_shared_libs_are_not_searched_in_bin() {
    let out = out_dir_with("assets-linux", &["lib/libespeak-ng.so", "bin/stray.so"]);

    assert_eq!(
        extract_lib_assets(&out, "linux"),
        vec![out.join("lib/libespeak-ng.so")]
    );

    std::fs::remove_dir_all(&out).unwrap();
}
