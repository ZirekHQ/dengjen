use std::ffi::{c_char, CStr, CString};

use dengjen_capi_jni_common::guard::{Failure, OPERATION_ERROR};
use ffi_support::ExternError;

pub fn to_cstring(text: String) -> Result<CString, Failure> {
    CString::new(text).map_err(|e| Failure {
        code: OPERATION_ERROR,
        message: format!("text contains NUL at byte {}", e.nul_position()),
    })
}

pub fn read_message(raw: *const c_char) -> String {
    if raw.is_null() {
        return String::new();
    }
    // SAFETY: libdengjen hands out NUL-terminated strings that stay valid until freed.
    unsafe { CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned()
}

pub fn check(err: ExternError) -> Result<(), Failure> {
    let code = err.get_code().code();
    if code == 0 {
        return Ok(());
    }
    let raw = err.get_raw_message();
    let message = read_message(raw);
    if !raw.is_null() {
        // SAFETY: `raw` was allocated by libdengjen for this ExternError and is freed exactly once.
        unsafe { libdengjen::libdengjenFreeString(raw.cast_mut()) };
    }
    Err(Failure { code, message })
}

#[cfg(test)]
mod tests {
    use super::*;
    use dengjen_capi_jni_common::guard::Failure;
    use ffi_support::{ErrorCode, ExternError};

    #[test]
    fn nul_in_text_is_a_failure() {
        let r = to_cstring("a\0b".to_owned());
        assert_eq!(r.map_err(|f| f.code), Err(19));
    }

    #[test]
    fn plain_text_converts() {
        assert!(to_cstring("hello".to_owned()).is_ok());
    }

    #[test]
    fn success_error_is_ok() {
        assert_eq!(check(ExternError::success()), Ok(()));
    }

    #[test]
    fn extern_error_becomes_failure_and_message_is_freed() {
        let err = ExternError::new_error(ErrorCode::new(23), "boom");
        let f = check(err);
        assert_eq!(
            f,
            Err(Failure {
                code: 23,
                message: "boom".into()
            })
        );
    }
}
