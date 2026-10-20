#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)
)]

use dengjen_voices::{Config, Progress, UreqTransport, VoiceError, VoiceManager};
use ffi_support::{call_with_result, define_string_destructor, ErrorCode, ExternError};
use std::ffi::{c_void, CStr};
use std::os::raw::c_char;
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

pub mod error_codes {
    pub const VOICES_ERR_INVALID_UTF8: i32 = 20;
    pub const VOICES_ERR_NULL_POINTER: i32 = 22;
    pub const VOICES_ERR_INVALID_CONFIGURATION: i32 = 24;
    pub const VOICES_ERR_NETWORK: i32 = 60;
    pub const VOICES_ERR_CATALOGUE_PARSE: i32 = 61;
    pub const VOICES_ERR_UNKNOWN_VOICE: i32 = 62;
    pub const VOICES_ERR_INVALID_KEY: i32 = 63;
    pub const VOICES_ERR_UNSAFE_PATH: i32 = 64;
    pub const VOICES_ERR_DIGEST_MISMATCH: i32 = 65;
    pub const VOICES_ERR_SIZE_MISMATCH: i32 = 66;
    pub const VOICES_ERR_CANCELLED: i32 = 67;
    pub const VOICES_ERR_BUSY: i32 = 68;
    pub const VOICES_ERR_NOT_INSTALLED: i32 = 69;
    pub const VOICES_ERR_IO: i32 = 70;
    pub const VOICES_ERR_NO_CATALOGUE: i32 = 71;
}

/// Bumped on any incompatible change to the exported functions or structs.
pub const VOICES_ABI_VERSION: u32 = 1;

pub struct DengjenVoices {
    manager: VoiceManager<UreqTransport>,
    cancel_epoch: AtomicU64,
}

ffi_support::implement_into_ffi_by_pointer!(DengjenVoices);
ffi_support::define_box_destructor!(DengjenVoices, _internal_libdengjenVoicesClose);
define_string_destructor!(_internal_libdengjenVoicesFreeString);

/// Options for [`libdengjenVoicesOpen`].
///
/// Callers set `struct_size` to `sizeof(VoicesConfig)` as they compiled it. Later versions only
/// append fields and read them when `struct_size` covers them.
#[repr(C)]
pub struct VoicesConfig {
    struct_size: u32,
    voices_dir: *const c_char,
    model_type: *const c_char,
    catalogue_url: *const c_char,
    files_base_url: *const c_char,
}

const VOICES_CONFIG_V1_SIZE: u32 = std::mem::size_of::<VoicesConfig>() as u32;

/// Called with the bytes written so far and the total for the voice being installed.
pub type VoicesProgressFn = Option<extern "C" fn(done: u64, total: u64, user_data: *mut c_void)>;

#[derive(Debug)]
pub(crate) struct VoicesFFIError(i32, String);

impl VoicesFFIError {
    fn with_code(code: i32, message: impl Into<String>) -> Self {
        Self(code, message.into())
    }

    fn null_pointer(param_name: &str) -> Self {
        Self::with_code(
            error_codes::VOICES_ERR_NULL_POINTER,
            format!("parameter `{param_name}` must not be null"),
        )
    }
}

impl From<VoiceError> for VoicesFFIError {
    fn from(error: VoiceError) -> Self {
        let code = match &error {
            VoiceError::Network(_) => error_codes::VOICES_ERR_NETWORK,
            VoiceError::CatalogueParse(_) => error_codes::VOICES_ERR_CATALOGUE_PARSE,
            VoiceError::NoCatalogue => error_codes::VOICES_ERR_NO_CATALOGUE,
            VoiceError::UnknownVoice(_) => error_codes::VOICES_ERR_UNKNOWN_VOICE,
            VoiceError::InvalidKey(_) => error_codes::VOICES_ERR_INVALID_KEY,
            VoiceError::UnsafePath(_) => error_codes::VOICES_ERR_UNSAFE_PATH,
            VoiceError::DigestMismatch(_) => error_codes::VOICES_ERR_DIGEST_MISMATCH,
            VoiceError::SizeMismatch(_) => error_codes::VOICES_ERR_SIZE_MISMATCH,
            VoiceError::Cancelled => error_codes::VOICES_ERR_CANCELLED,
            VoiceError::Busy(_) => error_codes::VOICES_ERR_BUSY,
            VoiceError::NotInstalled(_) => error_codes::VOICES_ERR_NOT_INSTALLED,
            VoiceError::Io(..) => error_codes::VOICES_ERR_IO,
        };
        Self::with_code(code, error.to_string())
    }
}

impl From<VoicesFFIError> for ExternError {
    fn from(error: VoicesFFIError) -> Self {
        ExternError::new_error(ErrorCode::new(error.0), error.1)
    }
}

type VoicesFFIResult<T> = Result<T, VoicesFFIError>;

/// # Safety
/// If non-null, `ptr` must point to a NUL-terminated string that stays valid for `'a`.
unsafe fn required_str<'a>(ptr: *const c_char, param_name: &str) -> VoicesFFIResult<&'a str> {
    if ptr.is_null() {
        return Err(VoicesFFIError::null_pointer(param_name));
    }
    // SAFETY: `ptr` is non-null and the caller vouches for the NUL-terminated string behind it.
    unsafe { CStr::from_ptr(ptr) }.to_str().map_err(|_| {
        VoicesFFIError::with_code(
            error_codes::VOICES_ERR_INVALID_UTF8,
            format!("parameter `{param_name}` is not valid UTF-8"),
        )
    })
}

fn undersized(declared: u32) -> VoicesFFIError {
    VoicesFFIError::with_code(
        error_codes::VOICES_ERR_INVALID_CONFIGURATION,
        format!(
            "config.struct_size is {declared}, but at least {VOICES_CONFIG_V1_SIZE} bytes \
             are required; set it to sizeof(VoicesConfig)"
        ),
    )
}

/// # Safety
/// `config_ptr` must be null or point to a `VoicesConfig` whose first field, `struct_size`, is
/// readable.
unsafe fn declared_size(config_ptr: *const VoicesConfig) -> VoicesFFIResult<u32> {
    if config_ptr.is_null() {
        return Err(VoicesFFIError::null_pointer("config"));
    }
    // SAFETY: `struct_size` is the first field and the caller guarantees it is readable.
    let declared = unsafe { config_ptr.cast::<u32>().read_unaligned() };
    (declared >= VOICES_CONFIG_V1_SIZE)
        .then_some(declared)
        .ok_or_else(|| undersized(declared))
}

/// # Safety
/// Every string field of `raw` must be null or a NUL-terminated string that stays valid for `'a`.
unsafe fn config_strings<'a>(raw: &VoicesConfig) -> VoicesFFIResult<[&'a str; 4]> {
    // SAFETY: the string fields satisfy `required_str`'s contract per this function's own docs.
    unsafe {
        Ok([
            required_str(raw.voices_dir, "voices_dir")?,
            required_str(raw.model_type, "model_type")?,
            required_str(raw.catalogue_url, "catalogue_url")?,
            required_str(raw.files_base_url, "files_base_url")?,
        ])
    }
}

/// # Safety
/// `config_ptr` must be null or point to a `VoicesConfig` whose first field, `struct_size`, is
/// readable and truthfully states how many bytes the caller's struct occupies, and whose string
/// fields are null or NUL-terminated strings that stay valid for the call.
unsafe fn read_config(config_ptr: *const VoicesConfig) -> VoicesFFIResult<Config> {
    // SAFETY: a non-null `config_ptr` has a readable `struct_size` per this function's own docs.
    unsafe { declared_size(config_ptr) }?;
    // SAFETY: `declared_size` checked that `struct_size` covers the whole V1 struct, so the read
    // stays inside the caller's struct.
    let raw = unsafe { config_ptr.read_unaligned() };
    // SAFETY: the string fields are null or NUL-terminated per this function's own docs.
    let [dir, model, catalogue, files] = unsafe { config_strings(&raw) }?;
    Ok(Config {
        voices_dir: PathBuf::from(dir),
        model_type: model.to_owned(),
        catalogue_url: catalogue.to_owned(),
        files_base_url: files.to_owned(),
    })
}

/// # Safety
/// `handle` must be null or a live handle from [`libdengjenVoicesOpen`] that is not closed while
/// the returned reference is used.
unsafe fn voices<'a>(handle: *const DengjenVoices) -> VoicesFFIResult<&'a DengjenVoices> {
    // SAFETY: a non-null `handle` is live per this function's `# Safety` doc.
    unsafe { handle.as_ref() }.ok_or_else(|| VoicesFFIError::null_pointer("handle"))
}

fn json(value: &impl serde::Serialize) -> VoicesFFIResult<String> {
    serde_json::to_string(value)
        .map_err(|error| VoicesFFIError::with_code(error_codes::VOICES_ERR_IO, error.to_string()))
}

/// The ABI version this library was built with; bindings compare it with the one they were
/// generated against before making any other call.
#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn libdengjenVoicesAbiVersion() -> u32 {
    VOICES_ABI_VERSION
}

/// Creates a voice manager for `config`. Nothing is read from the network or the disk yet.
/// Returns NULL and writes `out_error` on failure.
///
/// # Safety
/// `config` must be null or point to a `VoicesConfig` whose `struct_size` is truthful and whose
/// string fields are null or NUL-terminated strings that stay valid for the call.
/// `out_error` must be non-null, aligned, initialized, writable and not accessed by anything else
/// during the call.
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn libdengjenVoicesOpen(
    config: *const VoicesConfig,
    out_error: &mut ExternError,
) -> *mut DengjenVoices {
    call_with_result(
        out_error,
        AssertUnwindSafe(|| {
            // SAFETY: forwarded from this function's own `# Safety` contract.
            let config = unsafe { read_config(config)? };
            Ok::<_, VoicesFFIError>(DengjenVoices {
                manager: VoiceManager::new(config, UreqTransport::new())?,
                cancel_epoch: AtomicU64::new(0),
            })
        }),
    )
}

/// Downloads the catalogue from `catalogue_url`, caches it on disk and returns the number of
/// voices. Blocks until the transfer ends; call it from a worker thread.
///
/// # Safety
/// `handle` must be null or a live handle from [`libdengjenVoicesOpen`].
/// `out_error` must be non-null, aligned, initialized, writable and not accessed by anything else
/// during the call.
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn libdengjenVoicesFetchCatalogue(
    handle: *const DengjenVoices,
    out_error: &mut ExternError,
) -> i32 {
    call_with_result(
        out_error,
        AssertUnwindSafe(|| {
            // SAFETY: forwarded from this function's own `# Safety` contract.
            let count = unsafe { voices(handle)? }.manager.fetch_catalogue()?;
            i32::try_from(count).map_err(|_| {
                VoicesFFIError::with_code(
                    error_codes::VOICES_ERR_CATALOGUE_PARSE,
                    "too many voices",
                )
            })
        }),
    )
}

/// Loads the catalogue cached by an earlier fetch, for offline use, and returns the number of
/// voices. `VOICES_ERR_NO_CATALOGUE` means nothing is cached.
///
/// # Safety
/// Same contract as [`libdengjenVoicesFetchCatalogue`].
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn libdengjenVoicesLoadCachedCatalogue(
    handle: *const DengjenVoices,
    out_error: &mut ExternError,
) -> i32 {
    call_with_result(
        out_error,
        AssertUnwindSafe(|| {
            // SAFETY: forwarded from this function's own `# Safety` contract.
            let count = unsafe { voices(handle)? }.manager.load_cached_catalogue()?;
            i32::try_from(count).map_err(|_| {
                VoicesFFIError::with_code(
                    error_codes::VOICES_ERR_CATALOGUE_PARSE,
                    "too many voices",
                )
            })
        }),
    )
}

/// Returns the catalogue's voices as a JSON array, to be freed with
/// [`libdengjenVoicesFreeString`]. `language` is NULL for all voices, or a language code (`ku_TR`
/// or `ku-TR`) or family (`ku`).
///
/// # Safety
/// `handle` must be null or a live handle; `language` must be null or a NUL-terminated string.
/// `out_error` must be non-null, aligned, initialized, writable and not accessed by anything else
/// during the call.
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn libdengjenVoicesListJson(
    handle: *const DengjenVoices,
    language: *const c_char,
    out_error: &mut ExternError,
) -> *mut c_char {
    call_with_result(
        out_error,
        AssertUnwindSafe(|| {
            // SAFETY: forwarded from this function's own `# Safety` contract.
            let voices = unsafe { voices(handle)? };
            let language = if language.is_null() {
                None
            } else {
                // SAFETY: non-null `language` is a NUL-terminated string per the `# Safety` doc.
                Some(unsafe { required_str(language, "language")? })
            };
            json(&voices.manager.list(language)?)
        }),
    )
}

/// Returns the installed voices as a JSON array, to be freed with
/// [`libdengjenVoicesFreeString`].
///
/// # Safety
/// Same contract as [`libdengjenVoicesFetchCatalogue`].
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn libdengjenVoicesInstalledJson(
    handle: *const DengjenVoices,
    out_error: &mut ExternError,
) -> *mut c_char {
    call_with_result(
        out_error,
        AssertUnwindSafe(|| {
            // SAFETY: forwarded from this function's own `# Safety` contract.
            json(&unsafe { voices(handle)? }.manager.installed()?)
        }),
    )
}

/// Downloads, verifies and installs the voice `key`, resuming a partial download left by an
/// earlier call. Blocks until done; call it from a worker thread.
///
/// `progress` may be NULL; otherwise it runs on the calling thread, with the bytes written so far,
/// at most about ten times a second.
///
/// [`libdengjenVoicesCancel`] on the same handle, from any thread or from the progress callback,
/// stops this install at the next chunk with `VOICES_ERR_CANCELLED` and keeps the partial download.
/// A cancel issued before this install begins is not remembered and does not affect it.
/// A cancel that arrives after the last data chunk has been read does not stop the install.
///
/// A second install or removal of the same voice, in this or another process, fails with
/// `VOICES_ERR_BUSY`. On failure `out_error.message` must be freed with
/// [`libdengjenVoicesFreeString`].
///
/// # Safety
/// `handle` must be null or a live handle that is not closed during the call; `key` must be null
/// or a NUL-terminated string; `user_data` is passed to `progress` untouched.
/// `out_error` must be non-null, aligned, initialized, writable and not accessed by anything else
/// during the call.
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn libdengjenVoicesInstall(
    handle: *const DengjenVoices,
    key: *const c_char,
    progress: VoicesProgressFn,
    user_data: *mut c_void,
    out_error: &mut ExternError,
) {
    call_with_result(
        out_error,
        AssertUnwindSafe(|| {
            // SAFETY: forwarded from this function's own `# Safety` contract.
            let (voices, key) = unsafe { (voices(handle)?, required_str(key, "key")?) };
            let epoch = voices.cancel_epoch.load(Ordering::Acquire);
            let mut report = |p: Progress| {
                if let Some(callback) = progress {
                    callback(p.done, p.total, user_data);
                }
            };
            let cancelled = || voices.cancel_epoch.load(Ordering::Acquire) != epoch;
            Ok::<_, VoicesFFIError>(voices.manager.install(key, &mut report, &cancelled)?)
        }),
    )
}

/// Stops the installs that are already running on `handle` when this is called, each at its next
/// chunk with `VOICES_ERR_CANCELLED`, keeping their partial downloads. An install started
/// afterwards is unaffected, and a cancel issued before an install begins is not remembered.
/// Safe to call from any thread, including the progress callback. A NULL handle is ignored.
/// A cancel that arrives after the last data chunk has been read does not stop the install.
///
/// # Safety
/// `handle` must be null or a live handle that is not closed during the call.
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn libdengjenVoicesCancel(handle: *const DengjenVoices) {
    // SAFETY: a non-null `handle` is live per this function's `# Safety` doc.
    if let Some(voices) = unsafe { handle.as_ref() } {
        voices.cancel_epoch.fetch_add(1, Ordering::AcqRel);
    }
}

/// Deletes the installed voice `key`. `VOICES_ERR_NOT_INSTALLED` when it is not installed.
///
/// # Safety
/// `handle` must be null or a live handle; `key` must be null or a NUL-terminated string.
/// `out_error` must be non-null, aligned, initialized, writable and not accessed by anything else
/// during the call.
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn libdengjenVoicesRemove(
    handle: *const DengjenVoices,
    key: *const c_char,
    out_error: &mut ExternError,
) {
    call_with_result(
        out_error,
        AssertUnwindSafe(|| {
            // SAFETY: forwarded from this function's own `# Safety` contract.
            let (voices, key) = unsafe { (voices(handle)?, required_str(key, "key")?) };
            Ok::<_, VoicesFFIError>(voices.manager.remove(key)?)
        }),
    )
}

/// Frees a string returned by this library, or the `message` of an `ExternError` it filled.
/// A NULL pointer is ignored.
///
/// # Safety
/// `string_ptr` must be null or a string returned by this library, or an `ExternError` message
/// from this library, that was not freed before.
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn libdengjenVoicesFreeString(string_ptr: *mut c_char) {
    // SAFETY: this function's own `# Safety` doc is the destructor's contract.
    unsafe { _internal_libdengjenVoicesFreeString(string_ptr) };
}

/// Frees the handle. A NULL handle is ignored. Must not be called while another call on the same
/// handle is in flight on any thread: cancel running installs and wait for them to return first.
///
/// # Safety
/// `handle` must be null or a handle from [`libdengjenVoicesOpen`], and no other call may use it
/// concurrently with or after this one.
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn libdengjenVoicesClose(handle: *mut DengjenVoices) {
    // SAFETY: this function's own `# Safety` doc is the destructor's contract.
    unsafe { _internal_libdengjenVoicesClose(handle) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::ffi::{c_void, CString};
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::ptr;
    use std::sync::atomic::{AtomicU64, Ordering};

    const KEY: &str = "ku_TR-test-medium";

    fn c(text: &str) -> CString {
        CString::new(text).unwrap()
    }

    fn code_of(error: &ExternError) -> i32 {
        error.get_code().code()
    }

    fn release(error: ExternError) {
        // SAFETY: the message was allocated by `call_with_result` and is released once.
        unsafe { error.manually_release() };
    }

    struct Strings {
        dir: CString,
        model: CString,
        catalogue: CString,
        files: CString,
    }

    impl Strings {
        fn new(dir: &str, base: &str) -> Self {
            Self {
                dir: c(dir),
                model: c("piper"),
                catalogue: c(&format!("{base}/voices.json")),
                files: c(&format!("{base}/files")),
            }
        }

        fn config(&self) -> VoicesConfig {
            VoicesConfig {
                struct_size: VOICES_CONFIG_V1_SIZE,
                voices_dir: self.dir.as_ptr(),
                model_type: self.model.as_ptr(),
                catalogue_url: self.catalogue.as_ptr(),
                files_base_url: self.files.as_ptr(),
            }
        }
    }

    fn open(config: *const VoicesConfig) -> (*mut DengjenVoices, ExternError) {
        let mut error = ExternError::success();
        // SAFETY: `config` is null or points to a config whose strings outlive the call.
        let handle = unsafe { libdengjenVoicesOpen(config, &mut error) };
        (handle, error)
    }

    fn open_ok(strings: &Strings) -> *mut DengjenVoices {
        let config = strings.config();
        let (handle, error) = open(&config);
        assert!(
            !handle.is_null(),
            "open failed with code {}",
            code_of(&error)
        );
        handle
    }

    fn answer(mut stream: TcpStream, routes: &HashMap<String, Vec<u8>>) {
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            if stream.read(&mut byte).unwrap_or(0) == 0 {
                break;
            }
            head.push(byte[0]);
        }
        let head = String::from_utf8_lossy(&head);
        let path = head.split_whitespace().nth(1).unwrap_or("/");
        let (status, body) = routes
            .get(path)
            .map_or(("404 Not Found", &b""[..]), |bytes| {
                ("200 OK", bytes.as_slice())
            });
        let header = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(header.as_bytes());
        let _ = stream.write_all(body);
    }

    fn serve() -> String {
        let catalogue = serde_json::json!({ KEY: {
            "key": KEY, "name": "test", "quality": "medium", "num_speakers": 1,
            "language": {"code": "ku_TR", "family": "ku"},
            "files": {
                "ku/test/a.onnx": {"size_bytes": 3, "md5_digest": "900150983cd24fb0d6963f7d28e17f72"},
                "ku/test/a.onnx.json": {"size_bytes": 3, "md5_digest": "900150983cd24fb0d6963f7d28e17f72"},
                "ku/test/MODEL_CARD": {"size_bytes": 3, "md5_digest": "900150983cd24fb0d6963f7d28e17f72"}
            }
        }});
        let routes: HashMap<String, Vec<u8>> = HashMap::from([
            (
                "/voices.json".to_owned(),
                serde_json::to_vec(&catalogue).unwrap(),
            ),
            ("/files/ku/test/a.onnx".to_owned(), b"abc".to_vec()),
            ("/files/ku/test/a.onnx.json".to_owned(), b"abc".to_vec()),
            ("/files/ku/test/MODEL_CARD".to_owned(), b"abc".to_vec()),
        ]);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            listener
                .incoming()
                .flatten()
                .for_each(|stream| answer(stream, &routes));
        });
        base
    }

    extern "C" fn record_progress(done: u64, _total: u64, user_data: *mut c_void) {
        // SAFETY: the tests pass a pointer to a live `AtomicU64` as `user_data`.
        let last = unsafe { &*user_data.cast::<AtomicU64>() };
        last.store(done, Ordering::Relaxed);
    }

    fn json_of(text: *mut c_char) -> serde_json::Value {
        assert!(!text.is_null());
        // SAFETY: `text` is the NUL-terminated string just returned by the library.
        let parsed = serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap());
        // SAFETY: `text` is released once and not used afterwards.
        unsafe { libdengjenVoicesFreeString(text) };
        parsed.unwrap()
    }

    #[test]
    fn abi_version_matches_the_exported_constant() {
        assert_eq!(libdengjenVoicesAbiVersion(), VOICES_ABI_VERSION);
    }

    #[test]
    fn a_null_config_is_a_null_pointer_error() {
        let (handle, error) = open(ptr::null());
        assert!(handle.is_null());
        assert_eq!(code_of(&error), error_codes::VOICES_ERR_NULL_POINTER);
        release(error);
    }

    #[test]
    fn an_undersized_struct_size_is_invalid_configuration() {
        let strings = Strings::new("/tmp/x", "http://127.0.0.1:1");
        let mut config = strings.config();
        config.struct_size = 4;
        let (handle, error) = open(&config);
        assert!(handle.is_null());
        assert_eq!(
            code_of(&error),
            error_codes::VOICES_ERR_INVALID_CONFIGURATION
        );
        release(error);
    }

    #[test]
    fn a_null_string_field_is_a_null_pointer_error() {
        let strings = Strings::new("/tmp/x", "http://127.0.0.1:1");
        let mut config = strings.config();
        config.voices_dir = ptr::null();
        let (_, error) = open(&config);
        assert_eq!(code_of(&error), error_codes::VOICES_ERR_NULL_POINTER);
        release(error);
    }

    #[test]
    fn invalid_utf8_and_unsafe_model_types_are_reported() {
        let mut strings = Strings::new("/tmp/x", "http://127.0.0.1:1");
        strings.dir = CString::new(vec![0xff, 0xfe]).unwrap();
        let (_, utf8) = open(&strings.config());
        assert_eq!(code_of(&utf8), error_codes::VOICES_ERR_INVALID_UTF8);
        release(utf8);
        let mut strings = Strings::new("/tmp/x", "http://127.0.0.1:1");
        strings.model = c("../piper");
        let (_, key) = open(&strings.config());
        assert_eq!(code_of(&key), error_codes::VOICES_ERR_INVALID_KEY);
        release(key);
    }

    #[test]
    fn every_core_error_maps_to_a_distinct_code() {
        use std::path::PathBuf;
        let errors = [
            VoiceError::Network(String::new()),
            VoiceError::CatalogueParse(String::new()),
            VoiceError::NoCatalogue,
            VoiceError::UnknownVoice(String::new()),
            VoiceError::InvalidKey(String::new()),
            VoiceError::UnsafePath(String::new()),
            VoiceError::DigestMismatch(PathBuf::new()),
            VoiceError::SizeMismatch(PathBuf::new()),
            VoiceError::Cancelled,
            VoiceError::Busy(String::new()),
            VoiceError::NotInstalled(String::new()),
            VoiceError::Io(PathBuf::new(), std::io::ErrorKind::NotFound),
        ];
        let mut codes: Vec<i32> = errors
            .into_iter()
            .map(|e| VoicesFFIError::from(e).0)
            .collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), 12);
    }

    #[test]
    fn listing_before_a_catalogue_is_loaded_is_no_catalogue() {
        let dir = tempfile::tempdir().unwrap();
        let strings = Strings::new(dir.path().to_str().unwrap(), "http://127.0.0.1:1");
        let handle = open_ok(&strings);
        let mut error = ExternError::success();
        // SAFETY: `handle` is live; a null language is allowed.
        let text = unsafe { libdengjenVoicesListJson(handle, ptr::null(), &mut error) };
        assert!(text.is_null());
        assert_eq!(code_of(&error), error_codes::VOICES_ERR_NO_CATALOGUE);
        release(error);
        // SAFETY: `handle` is released once and not used afterwards.
        unsafe { libdengjenVoicesClose(handle) };
    }

    #[test]
    fn null_handles_and_null_keys_are_null_pointer_errors() {
        let mut error = ExternError::success();
        let key = c(KEY);
        // SAFETY: a null handle is the case under test.
        unsafe {
            libdengjenVoicesInstall(ptr::null(), key.as_ptr(), None, ptr::null_mut(), &mut error);
        }
        assert_eq!(code_of(&error), error_codes::VOICES_ERR_NULL_POINTER);
        release(error);
        let dir = tempfile::tempdir().unwrap();
        let handle = open_ok(&Strings::new(
            dir.path().to_str().unwrap(),
            "http://127.0.0.1:1",
        ));
        let mut error = ExternError::success();
        // SAFETY: `handle` is live; the null key is the case under test.
        unsafe {
            libdengjenVoicesInstall(handle, ptr::null(), None, ptr::null_mut(), &mut error);
        }
        assert_eq!(code_of(&error), error_codes::VOICES_ERR_NULL_POINTER);
        release(error);
        // SAFETY: `handle` is released once and not used afterwards.
        unsafe { libdengjenVoicesClose(handle) };
    }

    #[test]
    fn closing_a_null_handle_is_a_no_op() {
        // SAFETY: null is explicitly allowed by `libdengjenVoicesClose`.
        unsafe { libdengjenVoicesClose(ptr::null_mut()) };
    }

    #[test]
    fn fetch_list_install_and_remove_through_the_c_api() {
        let dir = tempfile::tempdir().unwrap();
        let handle = open_ok(&Strings::new(dir.path().to_str().unwrap(), &serve()));
        let mut error = ExternError::success();
        // SAFETY: `handle` is live and `error` is a fresh success value.
        let count = unsafe { libdengjenVoicesFetchCatalogue(handle, &mut error) };
        assert_eq!(count, 1, "fetch failed with code {}", code_of(&error));
        let ku = c("ku-TR");
        // SAFETY: `handle` is live and `ku` is a NUL-terminated string.
        let listed = json_of(unsafe { libdengjenVoicesListJson(handle, ku.as_ptr(), &mut error) });
        assert_eq!(listed[0]["key"], KEY);
        let files = listed[0]["files"].as_array().unwrap();
        assert!(!files.is_empty());
        assert!(files
            .iter()
            .all(|file| file["path"].is_string() && file.get("digest").is_none()));
        let (key, last) = (c(KEY), AtomicU64::new(0));
        // SAFETY: `handle` is live; `last` outlives the call and is the callback's `user_data`.
        unsafe {
            libdengjenVoicesInstall(
                handle,
                key.as_ptr(),
                Some(record_progress),
                ptr::from_ref(&last).cast_mut().cast(),
                &mut error,
            );
        }
        assert_eq!(code_of(&error), 0, "install failed");
        assert_eq!(last.load(Ordering::Relaxed), 9);
        // SAFETY: `handle` is live.
        let installed = json_of(unsafe { libdengjenVoicesInstalledJson(handle, &mut error) });
        assert_eq!(installed[0]["key"], KEY);
        // SAFETY: `handle` and `key` are valid.
        unsafe { libdengjenVoicesRemove(handle, key.as_ptr(), &mut error) };
        assert_eq!(code_of(&error), 0);
        // SAFETY: `handle` and `key` are valid.
        unsafe { libdengjenVoicesRemove(handle, key.as_ptr(), &mut error) };
        assert_eq!(code_of(&error), error_codes::VOICES_ERR_NOT_INSTALLED);
        release(error);
        // SAFETY: `handle` is released once and not used afterwards.
        unsafe { libdengjenVoicesClose(handle) };
    }

    extern "C" fn cancel_from_progress(_done: u64, _total: u64, user_data: *mut c_void) {
        // SAFETY: the test passes a live handle as `user_data`.
        unsafe { libdengjenVoicesCancel(user_data.cast::<DengjenVoices>()) };
    }

    fn fetched(dir: &tempfile::TempDir) -> *mut DengjenVoices {
        let handle = open_ok(&Strings::new(dir.path().to_str().unwrap(), &serve()));
        let mut error = ExternError::success();
        // SAFETY: `handle` is live.
        unsafe { libdengjenVoicesFetchCatalogue(handle, &mut error) };
        assert_eq!(code_of(&error), 0);
        handle
    }

    #[test]
    fn cancelling_from_the_progress_callback_stops_the_install() {
        let dir = tempfile::tempdir().unwrap();
        let handle = fetched(&dir);
        let (key, mut error) = (c(KEY), ExternError::success());
        // SAFETY: `handle` is live and is also passed as the callback's `user_data`.
        unsafe {
            libdengjenVoicesInstall(
                handle,
                key.as_ptr(),
                Some(cancel_from_progress),
                handle.cast(),
                &mut error,
            );
        }
        assert_eq!(code_of(&error), error_codes::VOICES_ERR_CANCELLED);
        release(error);
        // SAFETY: `handle` is released once and not used afterwards.
        unsafe { libdengjenVoicesClose(handle) };
    }

    #[test]
    fn a_cancel_before_an_install_starts_does_not_affect_it() {
        let dir = tempfile::tempdir().unwrap();
        let handle = fetched(&dir);
        let (key, mut error) = (c(KEY), ExternError::success());
        // SAFETY: `handle` is live; a NULL handle is also allowed.
        unsafe {
            libdengjenVoicesCancel(handle);
            libdengjenVoicesCancel(ptr::null());
            libdengjenVoicesInstall(handle, key.as_ptr(), None, ptr::null_mut(), &mut error);
        }
        assert_eq!(
            code_of(&error),
            0,
            "install after an earlier cancel must succeed"
        );
        // SAFETY: `handle` is released once and not used afterwards.
        unsafe { libdengjenVoicesClose(handle) };
    }
}
