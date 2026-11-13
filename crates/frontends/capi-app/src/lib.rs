#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)
)]

use dengjen_app::import::{text_from_bytes, Format, ImportError};
use ffi_support::{define_string_destructor, ErrorCode, ExternError};
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;

pub mod error_codes {
    pub const APP_ERR_INVALID_UTF8: i32 = 20;
    pub const APP_ERR_UNKNOWN: i32 = 21;
    pub const APP_ERR_NULL_POINTER: i32 = 22;
    pub const APP_ERR_IO: i32 = 70;
    pub const APP_ERR_UNSUPPORTED_FORMAT: i32 = 80;
    pub const APP_ERR_NOT_TEXT: i32 = 81;
}

/// Bumped on any incompatible change to the exported functions or structs.
pub const APP_ABI_VERSION: u32 = 1;

/// One sentence: `[byte_start, byte_end)` in the UTF-8 text passed in, and
/// `[utf16_start, utf16_end)` in the same text as UTF-16.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppSentence {
    pub byte_start: usize,
    pub byte_end: usize,
    pub utf16_start: usize,
    pub utf16_end: usize,
}

define_string_destructor!(_internal_libdengjenAppFreeString);

struct AppError(i32, String);

type AppResult<T> = Result<T, AppError>;

fn report<T>(out_error: &mut ExternError, fallback: T, body: impl FnOnce() -> AppResult<T>) -> T {
    let result = catch_unwind(AssertUnwindSafe(body)).unwrap_or_else(|_| {
        Err(AppError(
            error_codes::APP_ERR_UNKNOWN,
            "panic inside libdengjen_app".to_owned(),
        ))
    });
    match result {
        Ok(value) => {
            *out_error = ExternError::success();
            value
        }
        Err(AppError(code, message)) => {
            *out_error = ExternError::new_error(ErrorCode::new(code), message);
            fallback
        }
    }
}

/// # Safety
/// If non-null, `ptr` must point to a NUL-terminated string that stays valid for `'a`.
unsafe fn required_str<'a>(ptr: *const c_char, param_name: &str) -> AppResult<&'a str> {
    if ptr.is_null() {
        return Err(AppError(
            error_codes::APP_ERR_NULL_POINTER,
            format!("parameter `{param_name}` must not be null"),
        ));
    }
    // SAFETY: `ptr` is non-null and the caller vouches for the NUL-terminated string behind it.
    unsafe { CStr::from_ptr(ptr) }.to_str().map_err(|_| {
        AppError(
            error_codes::APP_ERR_INVALID_UTF8,
            format!("parameter `{param_name}` is not valid UTF-8"),
        )
    })
}

/// The ABI version this library was built with.
#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn libdengjenAppAbiVersion() -> u32 {
    APP_ABI_VERSION
}

/// Splits `text` into sentences and returns them as an array of `*out_len` items, or null when
/// there are none or on error. Free a non-null result with `libdengjenAppFreeSentences`.
///
/// # Safety
/// `text` must be null or a NUL-terminated string; `out_len` must be a valid pointer.
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn libdengjenAppSentences(
    text: *const c_char,
    out_len: &mut usize,
    out_error: &mut ExternError,
) -> *mut AppSentence {
    *out_len = 0;
    report(out_error, std::ptr::null_mut(), || {
        // SAFETY: `text` carries this function's own null-or-valid contract.
        let text = unsafe { required_str(text, "text") }?;
        let sentences: Box<[AppSentence]> = dengjen_app::sentences(text)
            .into_iter()
            .map(|sentence| AppSentence {
                byte_start: sentence.bytes.start,
                byte_end: sentence.bytes.end,
                utf16_start: sentence.utf16.start,
                utf16_end: sentence.utf16.end,
            })
            .collect();
        if sentences.is_empty() {
            return Ok(std::ptr::null_mut());
        }
        *out_len = sentences.len();
        Ok(Box::into_raw(sentences).cast::<AppSentence>())
    })
}

/// # Safety
/// `sentences` and `len` must be a pointer and length returned together by
/// `libdengjenAppSentences`, passed here at most once. Null is ignored.
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn libdengjenAppFreeSentences(sentences: *mut AppSentence, len: usize) {
    if sentences.is_null() {
        return;
    }
    // SAFETY: the pointer and length come from `Box::into_raw` of a boxed slice of exactly `len`
    // items in `libdengjenAppSentences`, and the caller frees it only once.
    drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(sentences, len)) });
}

/// Reads a `.txt`, `.md`, `.markdown`, `.htm`, `.html` or `.xhtml` file and returns its plain
/// text, or null on error. Free a non-null result with `libdengjenAppFreeString`.
///
/// # Safety
/// `path` must be null or a NUL-terminated UTF-8 string.
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn libdengjenAppTextFromFile(
    path: *const c_char,
    out_error: &mut ExternError,
) -> *mut c_char {
    report(out_error, std::ptr::null_mut(), || {
        // SAFETY: `path` carries this function's own null-or-valid contract.
        let path = Path::new(unsafe { required_str(path, "path") }?);
        let text = text_from_file(path)?;
        // Text read from a file can hold NUL; C strings cannot, so it is dropped.
        let text = CString::new(text.replace('\0', ""))
            .map_err(|error| AppError(error_codes::APP_ERR_UNKNOWN, error.to_string()))?;
        Ok(text.into_raw())
    })
}

fn text_from_file(path: &Path) -> AppResult<String> {
    let format = path
        .extension()
        .and_then(|extension| extension.to_str())
        .and_then(Format::from_extension)
        .ok_or_else(|| {
            AppError(
                error_codes::APP_ERR_UNSUPPORTED_FORMAT,
                format!("`{}` is not a .txt, .md or .html file", path.display()),
            )
        })?;
    let bytes = std::fs::read(path).map_err(|error| {
        AppError(
            error_codes::APP_ERR_IO,
            format!("could not read `{}`: {error}", path.display()),
        )
    })?;
    text_from_bytes(&bytes, format).map_err(|error| match error {
        ImportError::NotText => AppError(error_codes::APP_ERR_NOT_TEXT, error.to_string()),
    })
}

/// # Safety
/// `string` must be null or a string returned by this library, passed here at most once.
#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn libdengjenAppFreeString(string: *mut c_char) {
    if string.is_null() {
        return;
    }
    // SAFETY: this function's own `# Safety` doc is the destructor's contract.
    unsafe { _internal_libdengjenAppFreeString(string) };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sentences_of(text: &str) -> (Vec<AppSentence>, ExternError) {
        let text = CString::new(text).unwrap();
        let mut len = 0;
        let mut error = ExternError::default();
        // SAFETY: `text` is a live NUL-terminated string.
        let ptr = unsafe { libdengjenAppSentences(text.as_ptr(), &mut len, &mut error) };
        let found = if ptr.is_null() {
            Vec::new()
        } else {
            // SAFETY: `ptr` and `len` were just returned together.
            let copied = unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec();
            // SAFETY: freed once, with the length it came with.
            unsafe { libdengjenAppFreeSentences(ptr, len) };
            copied
        };
        (found, error)
    }

    fn message(error: ExternError) -> String {
        let text = error.get_message().as_str().to_owned();
        // SAFETY: the message is a library-owned string released exactly once here.
        unsafe { error.manually_release() };
        text
    }

    #[test]
    fn sentences_come_back_with_both_ranges() {
        let (found, error) = sentences_of("Silav 👋. Baş e.");
        assert!(error.get_code().is_success());
        assert_eq!(
            found,
            [
                AppSentence {
                    byte_start: 0,
                    byte_end: 11,
                    utf16_start: 0,
                    utf16_end: 9
                },
                AppSentence {
                    byte_start: 12,
                    byte_end: 19,
                    utf16_start: 10,
                    utf16_end: 16
                },
            ]
        );
    }

    #[test]
    fn empty_text_returns_null_and_zero_length() {
        let (found, error) = sentences_of("   ");
        assert!(found.is_empty());
        assert!(error.get_code().is_success());
    }

    #[test]
    fn null_text_is_an_error() {
        let mut len = 7;
        let mut error = ExternError::default();
        // SAFETY: null is part of the contract.
        let ptr = unsafe { libdengjenAppSentences(std::ptr::null(), &mut len, &mut error) };
        assert!(ptr.is_null());
        assert_eq!(len, 0);
        assert_eq!(error.get_code().code(), error_codes::APP_ERR_NULL_POINTER);
        assert!(message(error).contains("text"));
    }

    fn read(path: &Path) -> Result<String, (i32, String)> {
        let path = CString::new(path.to_str().unwrap()).unwrap();
        let mut error = ExternError::default();
        // SAFETY: `path` is a live NUL-terminated string.
        let text = unsafe { libdengjenAppTextFromFile(path.as_ptr(), &mut error) };
        if text.is_null() {
            let code = error.get_code().code();
            return Err((code, message(error)));
        }
        // SAFETY: a non-null result is a library-owned C string.
        let copied = unsafe { CStr::from_ptr(text) }.to_str().unwrap().to_owned();
        // SAFETY: freed once.
        unsafe { libdengjenAppFreeString(text) };
        Ok(copied)
    }

    #[test]
    fn text_from_file_picks_the_format_from_the_extension() {
        let dir = tempfile::tempdir().unwrap();
        let html = dir.path().join("page.HTML");
        std::fs::write(&html, "<p>Roj <b>baş</b>.</p><p>Silav.</p>").unwrap();
        assert_eq!(read(&html).unwrap(), "Roj baş.\nSilav.");
    }

    #[test]
    fn text_from_file_reports_unsupported_missing_and_binary_files() {
        let dir = tempfile::tempdir().unwrap();
        let pdf = dir.path().join("a.pdf");
        assert_eq!(
            read(&pdf).unwrap_err().0,
            error_codes::APP_ERR_UNSUPPORTED_FORMAT
        );
        let missing = dir.path().join("missing.txt");
        assert_eq!(read(&missing).unwrap_err().0, error_codes::APP_ERR_IO);
        let binary = dir.path().join("b.txt");
        std::fs::write(&binary, [0xFF, 0x00, 0xFE]).unwrap();
        assert_eq!(read(&binary).unwrap_err().0, error_codes::APP_ERR_NOT_TEXT);
    }

    #[test]
    fn abi_version_is_exported() {
        assert_eq!(libdengjenAppAbiVersion(), APP_ABI_VERSION);
    }
}
