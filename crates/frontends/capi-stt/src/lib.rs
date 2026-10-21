#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)
)]

use dengjen_stt::{verify_model, Recognizer, RecognizerConfig, SttError};
use ffi_support::{call_with_result, define_string_destructor, ErrorCode, ExternError};
use std::ffi::CStr;
use std::os::raw::c_char;
use std::path::Path;

pub mod error_codes {
    pub const INVALID_UTF8_SEQUENCE: i32 = 20;
    pub const NULL_POINTER: i32 = 22;
    pub const INVALID_CONFIGURATION: i32 = 24;
    pub const MISSING_MODEL_FILE: i32 = 40;
    pub const UNREADABLE_MODEL_FILE: i32 = 41;
    pub const CHECKSUM_MISMATCH: i32 = 42;
    pub const MODEL_LOAD_FAILED: i32 = 43;
    pub const INVALID_AUDIO: i32 = 44;
    pub const DECODE_FAILED: i32 = 45;
}

/// Bumped on any incompatible change to the exported functions or structs.
pub const STT_ABI_VERSION: u32 = 1;

const DEFAULT_NUM_THREADS: i32 = 2;

pub struct DengjenRecognizer(Recognizer);

ffi_support::implement_into_ffi_by_pointer!(DengjenRecognizer);
ffi_support::define_box_destructor!(DengjenRecognizer, _internal_libdengjenSttUnload);
define_string_destructor!(_internal_libdengjenSttFreeString);

/// Options for [`libdengjenSttLoad`].
///
/// Callers set `struct_size` to `sizeof(SttConfig)` as they compiled it. Later versions only
/// append fields and read them when `struct_size` covers them.
#[repr(C)]
pub struct SttConfig {
    pub struct_size: u32,
    pub num_threads: i32,
}

const STT_CONFIG_V1_SIZE: u32 =
    (std::mem::offset_of!(SttConfig, num_threads) + std::mem::size_of::<i32>()) as u32;

#[derive(Debug)]
pub struct SttFFIError(i32, String);

impl SttFFIError {
    fn with_code(code: i32, message: impl Into<String>) -> Self {
        Self(code, message.into())
    }

    fn null_pointer(param_name: &str) -> Self {
        Self::with_code(
            error_codes::NULL_POINTER,
            format!("parameter `{param_name}` must not be null"),
        )
    }
}

impl From<SttError> for SttFFIError {
    fn from(error: SttError) -> Self {
        let code = match &error {
            SttError::MissingModelFile(_) => error_codes::MISSING_MODEL_FILE,
            SttError::Unreadable(_) => error_codes::UNREADABLE_MODEL_FILE,
            SttError::ChecksumMismatch(_) => error_codes::CHECKSUM_MISMATCH,
            SttError::ModelLoad(_) => error_codes::MODEL_LOAD_FAILED,
            SttError::InvalidAudio(_) => error_codes::INVALID_AUDIO,
            SttError::Decode => error_codes::DECODE_FAILED,
        };
        Self::with_code(code, error.to_string())
    }
}

impl From<SttFFIError> for ExternError {
    fn from(error: SttFFIError) -> Self {
        ExternError::new_error(ErrorCode::new(error.0), error.1)
    }
}

type SttFFIResult<T> = Result<T, SttFFIError>;

/// # Safety
/// If non-null, `ptr` must point to a NUL-terminated string that stays valid for `'a`.
unsafe fn required_str<'a>(ptr: *const c_char, param_name: &str) -> SttFFIResult<&'a str> {
    if ptr.is_null() {
        return Err(SttFFIError::null_pointer(param_name));
    }
    // SAFETY: `ptr` is non-null and the caller vouches for the NUL-terminated string behind it.
    unsafe { CStr::from_ptr(ptr) }.to_str().map_err(|_| {
        SttFFIError::with_code(
            error_codes::INVALID_UTF8_SEQUENCE,
            format!("parameter `{param_name}` is not valid UTF-8"),
        )
    })
}

/// # Safety
/// `config_ptr` must be null or point to an `SttConfig` whose first field, `struct_size`, is
/// readable and truthfully states how many bytes the caller's struct occupies.
unsafe fn read_config(config_ptr: *const SttConfig) -> SttFFIResult<RecognizerConfig> {
    if config_ptr.is_null() {
        return Ok(RecognizerConfig {
            num_threads: DEFAULT_NUM_THREADS,
        });
    }
    // SAFETY: `struct_size` is the first field and the caller guarantees it is readable.
    let declared = unsafe { config_ptr.cast::<u32>().read_unaligned() };
    if declared < STT_CONFIG_V1_SIZE {
        return Err(SttFFIError::with_code(
            error_codes::INVALID_CONFIGURATION,
            format!(
                "config.struct_size is {declared}, but at least {STT_CONFIG_V1_SIZE} bytes are \
                 required; set it to sizeof(SttConfig)"
            ),
        ));
    }
    // SAFETY: `declared` covers `num_threads`, so the field lies within the caller's struct.
    let num_threads = unsafe {
        config_ptr
            .cast::<u8>()
            .add(std::mem::offset_of!(SttConfig, num_threads))
            .cast::<i32>()
            .read_unaligned()
    };
    if num_threads < 1 {
        return Err(SttFFIError::with_code(
            error_codes::INVALID_CONFIGURATION,
            format!("config.num_threads is {num_threads}, but it must be at least 1"),
        ));
    }
    Ok(RecognizerConfig { num_threads })
}

/// # Safety
/// The pointers must satisfy the contracts of [`required_str`] and [`read_config`].
unsafe fn load(
    model_dir_ptr: *const c_char,
    config_ptr: *const SttConfig,
    expected_sha256_ptr: *const c_char,
) -> SttFFIResult<DengjenRecognizer> {
    // SAFETY: forwarded from this function's own `# Safety` contract.
    let model_dir = Path::new(unsafe { required_str(model_dir_ptr, "model_dir")? });
    // SAFETY: forwarded from this function's own `# Safety` contract.
    let expected_sha256 = unsafe { required_str(expected_sha256_ptr, "expected_sha256")? };
    // SAFETY: forwarded from this function's own `# Safety` contract.
    let config = unsafe { read_config(config_ptr)? };
    verify_model(model_dir, expected_sha256)?;
    Ok(DengjenRecognizer(Recognizer::new(model_dir, config)?))
}

/// The ABI version this library was built with; bindings compare it with the one they were
/// generated against before making any other call.
#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn libdengjenSttAbiVersion() -> u32 {
    STT_ABI_VERSION
}

/// Loads the recognizer from `model_dir` after checking that its model file hashes to
/// `expected_sha256` (hex SHA-256). A mismatch is reported as `CHECKSUM_MISMATCH` instead of
/// loading the file, because sherpa-onnx aborts the process on a corrupt model. The checksummed
/// file being absent or unreadable is `UNREADABLE_MODEL_FILE`; `MISSING_MODEL_FILE` means the
/// tokens file is absent. `config` may be NULL for the defaults. Returns NULL and writes
/// `out_error` on failure.
///
/// # Safety
/// `model_dir` and `expected_sha256` must be null or NUL-terminated strings; `config` must be
/// null or point to an `SttConfig` whose `struct_size` is truthful.
/// `out_error` must be non-null, aligned, initialized, writable and not accessed by anything else
/// during the call.
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn libdengjenSttLoad(
    model_dir: *const c_char,
    config: *const SttConfig,
    expected_sha256: *const c_char,
    out_error: &mut ExternError,
) -> *mut DengjenRecognizer {
    call_with_result(out_error, || {
        // SAFETY: forwarded from this function's own `# Safety` contract.
        unsafe { load(model_dir, config, expected_sha256) }
    })
}

/// Transcribes `len` mono samples recorded at `sample_rate` Hz, which the engine resamples to the
/// model's rate, and returns the text, to be freed with [`libdengjenSttFreeString`]. Empty
/// audio, a non-positive rate and audio shorter than 25 ms are `INVALID_AUDIO`; a NULL `samples`
/// with `len == 0` is therefore `INVALID_AUDIO`, not `NULL_POINTER`. Blocks until decoding ends
/// and cannot be cancelled, so call it from a worker thread. Several threads may call it on one
/// recognizer at once.
///
/// # Safety
/// `recognizer` must be null or a live handle from [`libdengjenSttLoad`] that is not unloaded
/// during the call; `samples` must be null or point to `len` readable floats.
/// `out_error` must be non-null, aligned, initialized, writable and not accessed by anything else
/// during the call.
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn libdengjenSttRecognize(
    recognizer: *const DengjenRecognizer,
    samples: *const f32,
    len: usize,
    sample_rate: i32,
    out_error: &mut ExternError,
) -> *mut c_char {
    call_with_result(out_error, || {
        // SAFETY: a non-null `recognizer` is a live handle per this function's `# Safety` doc.
        let recognizer = unsafe { recognizer.as_ref() }
            .ok_or_else(|| SttFFIError::null_pointer("recognizer"))?;
        let samples: &[f32] = match (samples.is_null(), len) {
            (_, 0) => &[],
            (true, _) => return Err(SttFFIError::null_pointer("samples")),
            // SAFETY: `samples` is non-null and the caller vouches for `len` readable floats.
            (false, _) => unsafe { std::slice::from_raw_parts(samples, len) },
        };
        Ok(recognizer.0.recognize(samples, sample_rate)?)
    })
}

/// # Safety
/// `string_ptr` must be null or a string returned by [`libdengjenSttRecognize`], or an
/// `ExternError` message from this library, that was not freed before.
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn libdengjenSttFreeString(string_ptr: *mut c_char) {
    // SAFETY: this function's own `# Safety` doc is the destructor's contract.
    unsafe { _internal_libdengjenSttFreeString(string_ptr) };
}

/// Frees the recognizer. A NULL handle is ignored.
///
/// # Safety
/// `recognizer` must be null or a handle from [`libdengjenSttLoad`], and no other call may use it
/// concurrently with or after this one.
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn libdengjenSttUnload(recognizer: *mut DengjenRecognizer) {
    // SAFETY: this function's own `# Safety` doc is the destructor's contract.
    unsafe { _internal_libdengjenSttUnload(recognizer) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;
    use std::ptr;

    const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    fn c(text: &str) -> CString {
        CString::new(text).unwrap()
    }

    fn load_error(
        model_dir: *const c_char,
        config: *const SttConfig,
        sha: *const c_char,
    ) -> ExternError {
        let mut error = ExternError::success();
        // SAFETY: every pointer is null or a NUL-terminated string/valid config owned by the test.
        let handle = unsafe { libdengjenSttLoad(model_dir, config, sha, &mut error) };
        assert!(handle.is_null());
        error
    }

    fn code_of(error: &ExternError) -> i32 {
        error.get_code().code()
    }

    fn release(error: ExternError) {
        // SAFETY: the message was allocated by `call_with_result` and is released once.
        unsafe { error.manually_release() };
    }

    #[test]
    fn abi_version_matches_the_exported_constant() {
        assert_eq!(libdengjenSttAbiVersion(), STT_ABI_VERSION);
    }

    #[test]
    fn null_model_dir_and_null_checksum_are_null_pointer_errors() {
        let dir = c("/nonexistent");
        let sha = c(EMPTY_SHA256);
        let no_dir = load_error(ptr::null(), ptr::null(), sha.as_ptr());
        let no_sha = load_error(dir.as_ptr(), ptr::null(), ptr::null());
        assert_eq!(code_of(&no_dir), error_codes::NULL_POINTER);
        assert_eq!(code_of(&no_sha), error_codes::NULL_POINTER);
        release(no_dir);
        release(no_sha);
    }

    #[test]
    fn invalid_utf8_model_dir_is_reported() {
        let dir = CString::new(vec![0xff, 0xfe]).unwrap();
        let sha = c(EMPTY_SHA256);
        let error = load_error(dir.as_ptr(), ptr::null(), sha.as_ptr());
        assert_eq!(code_of(&error), error_codes::INVALID_UTF8_SEQUENCE);
        release(error);
    }

    #[test]
    fn undersized_or_zero_thread_configs_are_invalid_configuration() {
        let dir = c("/nonexistent");
        let sha = c(EMPTY_SHA256);
        let small = SttConfig {
            struct_size: 4,
            num_threads: 2,
        };
        let zero = SttConfig {
            struct_size: STT_CONFIG_V1_SIZE,
            num_threads: 0,
        };
        let undersized = load_error(dir.as_ptr(), &small, sha.as_ptr());
        let no_threads = load_error(dir.as_ptr(), &zero, sha.as_ptr());
        assert_eq!(code_of(&undersized), error_codes::INVALID_CONFIGURATION);
        assert_eq!(code_of(&no_threads), error_codes::INVALID_CONFIGURATION);
        release(undersized);
        release(no_threads);
    }

    #[test]
    fn missing_model_file_is_unreadable() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = c(tmp.path().to_str().unwrap());
        let sha = c(EMPTY_SHA256);
        let error = load_error(dir.as_ptr(), ptr::null(), sha.as_ptr());
        assert_eq!(code_of(&error), error_codes::UNREADABLE_MODEL_FILE);
        release(error);
    }

    #[test]
    fn wrong_checksum_is_rejected_before_the_model_is_loaded() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(dengjen_stt::MODEL_FILE), b"not a model").unwrap();
        let dir = c(tmp.path().to_str().unwrap());
        let sha = c(EMPTY_SHA256);
        let error = load_error(dir.as_ptr(), ptr::null(), sha.as_ptr());
        assert_eq!(code_of(&error), error_codes::CHECKSUM_MISMATCH);
        release(error);
    }

    #[test]
    fn recognize_rejects_null_handles_and_null_samples() {
        let mut error = ExternError::success();
        // SAFETY: a null handle is the case under test; no other pointer is dereferenced.
        let text =
            unsafe { libdengjenSttRecognize(ptr::null(), ptr::null(), 0, 16_000, &mut error) };
        assert!(text.is_null());
        assert_eq!(code_of(&error), error_codes::NULL_POINTER);
        release(error);
    }

    #[test]
    fn unloading_a_null_handle_is_a_no_op() {
        // SAFETY: null is explicitly allowed by `libdengjenSttUnload`.
        unsafe { libdengjenSttUnload(ptr::null_mut()) };
    }

    // Self-skips without a model: DENGJEN_STT_MODEL_DIR holds the model files,
    // DENGJEN_STT_MODEL_SHA256 the hex digest of model.int8.onnx, and DENGJEN_STT_CLIP raw
    // little-endian f32 mono samples at 16 kHz.
    #[test]
    fn transcribes_a_clip_through_the_c_api() {
        let (Ok(dir), Ok(sha), Ok(clip)) = (
            std::env::var("DENGJEN_STT_MODEL_DIR"),
            std::env::var("DENGJEN_STT_MODEL_SHA256"),
            std::env::var("DENGJEN_STT_CLIP"),
        ) else {
            eprintln!("skipping: DENGJEN_STT_MODEL_DIR/_SHA256/DENGJEN_STT_CLIP not set");
            return;
        };
        let bytes = std::fs::read(clip).unwrap();
        let (chunks, _) = bytes.as_chunks::<4>();
        let samples: Vec<f32> = chunks.iter().map(|b| f32::from_le_bytes(*b)).collect();
        let (dir, sha) = (c(&dir), c(&sha));
        let mut error = ExternError::success();
        // SAFETY: both strings are NUL-terminated and outlive the call; a null config selects defaults.
        let handle =
            unsafe { libdengjenSttLoad(dir.as_ptr(), ptr::null(), sha.as_ptr(), &mut error) };
        assert!(
            !handle.is_null(),
            "load failed with code {}",
            code_of(&error)
        );
        // SAFETY: `handle` is live and `samples` holds `samples.len()` floats.
        let text = unsafe {
            libdengjenSttRecognize(handle, samples.as_ptr(), samples.len(), 16_000, &mut error)
        };
        assert!(
            !text.is_null(),
            "recognize failed with code {}",
            code_of(&error)
        );
        // SAFETY: `text` is the NUL-terminated string just returned.
        let transcript = unsafe { CStr::from_ptr(text) }.to_str().unwrap().to_owned();
        eprintln!("transcript: {transcript}");
        assert!(!transcript.trim().is_empty());
        // SAFETY: each pointer is released once and not used afterwards.
        unsafe {
            libdengjenSttFreeString(text);
            libdengjenSttUnload(handle);
        }
    }
}
