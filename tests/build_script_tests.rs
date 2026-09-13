// Only the handful of functions covered by #[test]s below are called from this
// binary; the rest run when Cargo compiles build.rs as the actual build script.
#[allow(dead_code)]
#[path = "../build.rs"]
mod build_script;

use build_script::{
    EspeakNgSource, copy_espeak_ng_data_next_to_binary, copy_folder, copy_succeeded,
    extract_xz_tar_bundle, resolve_espeak_ng_source, resolved_pcaudio_lib, resolved_sonic_lib,
};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;

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
fn unix_copy_succeeds_only_on_exit_code_zero() {
    assert!(copy_succeeded(false, Some(0)));
    assert!(!copy_succeeded(false, Some(1)));
    assert!(!copy_succeeded(false, None));
}

#[test]
fn robocopy_copy_succeeds_below_the_failure_bit() {
    assert!(copy_succeeded(true, Some(0)));
    assert!(copy_succeeded(true, Some(1)));
    assert!(copy_succeeded(true, Some(7)));
    assert!(!copy_succeeded(true, Some(8)));
    assert!(!copy_succeeded(true, None));
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
fn copies_espeak_ng_data_next_to_the_final_binary() {
    let out_dir = scratch_path("espeak-data-out");
    let target_dir = scratch_path("espeak-data-target");
    let data_src = out_dir.join("share").join("espeak-ng-data");
    std::fs::create_dir_all(&data_src).unwrap();
    std::fs::write(data_src.join("phontab"), b"phontab-contents").unwrap();

    copy_espeak_ng_data_next_to_binary(&out_dir, &target_dir);

    assert_eq!(
        std::fs::read(target_dir.join("espeak-ng-data").join("phontab")).unwrap(),
        b"phontab-contents"
    );

    std::fs::remove_dir_all(&out_dir).unwrap();
    std::fs::remove_dir_all(&target_dir).unwrap();
}

#[test]
fn leaves_an_existing_copy_alone_when_out_dir_has_no_fresh_source() {
    let out_dir = scratch_path("espeak-data-missing-out");
    let target_dir = scratch_path("espeak-data-existing-target");
    let existing_dst = target_dir.join("espeak-ng-data");
    std::fs::create_dir_all(&existing_dst).unwrap();
    std::fs::write(existing_dst.join("phontab"), b"already-here").unwrap();
    assert!(!out_dir.exists());

    copy_espeak_ng_data_next_to_binary(&out_dir, &target_dir);

    assert_eq!(
        std::fs::read(existing_dst.join("phontab")).unwrap(),
        b"already-here"
    );

    std::fs::remove_dir_all(&target_dir).unwrap();
}

#[test]
fn does_nothing_when_out_dir_has_no_espeak_ng_data_to_copy() {
    let out_dir = scratch_path("espeak-data-cross-compile-out");
    let target_dir = scratch_path("espeak-data-cross-compile-target");
    std::fs::create_dir_all(&out_dir).unwrap();
    assert!(!out_dir.join("share").exists());

    copy_espeak_ng_data_next_to_binary(&out_dir, &target_dir);

    assert!(!target_dir.join("espeak-ng-data").exists());

    std::fs::remove_dir_all(&out_dir).unwrap();
}

#[test]
fn refreshes_a_stale_espeak_ng_data_copy_when_a_fresh_source_is_available() {
    let out_dir = scratch_path("espeak-data-refresh-out");
    let target_dir = scratch_path("espeak-data-refresh-target");
    let data_src = out_dir.join("share").join("espeak-ng-data");
    std::fs::create_dir_all(&data_src).unwrap();
    std::fs::write(data_src.join("phontab"), b"fresh-contents").unwrap();

    let existing_dst = target_dir.join("espeak-ng-data");
    std::fs::create_dir_all(&existing_dst).unwrap();
    std::fs::write(existing_dst.join("phontab"), b"stale-contents").unwrap();
    std::fs::write(existing_dst.join("only-in-stale-copy"), b"leftover").unwrap();

    copy_espeak_ng_data_next_to_binary(&out_dir, &target_dir);

    assert_eq!(
        std::fs::read(existing_dst.join("phontab")).unwrap(),
        b"fresh-contents"
    );
    assert!(
        !existing_dst.join("only-in-stale-copy").exists(),
        "stale files from the old copy must not survive a refresh"
    );

    std::fs::remove_dir_all(&out_dir).unwrap();
    std::fs::remove_dir_all(&target_dir).unwrap();
}

#[test]
fn prefers_the_live_submodule_directory_when_it_exists() {
    let espeak_src = scratch_path("resolve-src-dir");
    let bundle_path = scratch_path("resolve-src-bundle");
    std::fs::create_dir_all(&espeak_src).unwrap();
    std::fs::write(&bundle_path, b"not actually used").unwrap();

    match resolve_espeak_ng_source(&espeak_src, &bundle_path) {
        EspeakNgSource::Directory(src) => assert_eq!(src, espeak_src),
        EspeakNgSource::Bundle(_) => panic!("expected the live directory to win"),
    }

    std::fs::remove_dir_all(&espeak_src).unwrap();
    std::fs::remove_file(&bundle_path).unwrap();
}

#[test]
fn falls_back_to_the_bundle_when_no_live_submodule_directory_exists() {
    let espeak_src = scratch_path("resolve-missing-src-dir");
    let bundle_path = scratch_path("resolve-fallback-bundle");
    std::fs::write(&bundle_path, b"bundle contents").unwrap();
    assert!(!espeak_src.exists());

    match resolve_espeak_ng_source(&espeak_src, &bundle_path) {
        EspeakNgSource::Bundle(bundle) => assert_eq!(bundle, bundle_path),
        EspeakNgSource::Directory(_) => panic!("expected the bundle fallback"),
    }

    std::fs::remove_file(&bundle_path).unwrap();
}

#[test]
fn panics_when_neither_submodule_directory_nor_bundle_exists() {
    let espeak_src = scratch_path("resolve-nothing-src-dir");
    let bundle_path = scratch_path("resolve-nothing-bundle");
    assert!(!espeak_src.exists());
    assert!(!bundle_path.exists());

    let panicked = catch_unwind(AssertUnwindSafe(|| {
        resolve_espeak_ng_source(&espeak_src, &bundle_path)
    }))
    .is_err();

    assert!(panicked);
}

fn build_xz_tar_fixture(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut tar_bytes = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_bytes);
        for (path, content) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, path, *content).unwrap();
        }
        builder.finish().unwrap();
    }
    let mut compressed = Vec::new();
    lzma_rs::xz_compress(&mut &tar_bytes[..], &mut compressed).unwrap();
    compressed
}

#[test]
fn extracts_an_xz_tar_bundle_into_the_destination() {
    let bundle_path = scratch_path("extract-bundle-fixture.tar.xz");
    let dst = scratch_path("extract-bundle-dst");
    std::fs::write(
        &bundle_path,
        build_xz_tar_fixture(&[
            ("CMakeLists.txt", b"cmake stuff"),
            ("dictsource/en_list", b"english dictionary source"),
        ]),
    )
    .unwrap();

    extract_xz_tar_bundle(&bundle_path, &dst);

    assert_eq!(
        std::fs::read(dst.join("CMakeLists.txt")).unwrap(),
        b"cmake stuff"
    );
    assert_eq!(
        std::fs::read(dst.join("dictsource").join("en_list")).unwrap(),
        b"english dictionary source"
    );

    std::fs::remove_file(&bundle_path).unwrap();
    std::fs::remove_dir_all(&dst).unwrap();
}

#[test]
fn extract_xz_tar_bundle_never_leaves_a_destination_behind_when_the_bundle_is_missing() {
    let bundle_path = scratch_path("extract-missing-bundle.tar.xz");
    let dst = scratch_path("extract-missing-bundle-dst");
    assert!(!bundle_path.exists());
    assert!(!dst.exists());

    let panicked = catch_unwind(AssertUnwindSafe(|| {
        extract_xz_tar_bundle(&bundle_path, &dst)
    }))
    .is_err();

    assert!(
        panicked,
        "extract_xz_tar_bundle should panic when the bundle is missing"
    );
    assert!(
        !dst.exists(),
        "a failed extraction must not leave the destination behind"
    );
}

#[test]
fn extract_xz_tar_bundle_never_leaves_a_destination_behind_when_the_bundle_is_corrupt() {
    let bundle_path = scratch_path("extract-corrupt-bundle.tar.xz");
    let dst = scratch_path("extract-corrupt-bundle-dst");
    std::fs::write(&bundle_path, b"not a valid xz stream").unwrap();

    let panicked = catch_unwind(AssertUnwindSafe(|| {
        extract_xz_tar_bundle(&bundle_path, &dst)
    }))
    .is_err();

    assert!(
        panicked,
        "extract_xz_tar_bundle should panic when the bundle isn't a valid xz stream"
    );
    assert!(
        !dst.exists(),
        "a failed extraction must not leave the destination behind"
    );

    std::fs::remove_file(&bundle_path).unwrap();
}
