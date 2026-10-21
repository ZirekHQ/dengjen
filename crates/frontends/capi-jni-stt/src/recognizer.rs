use std::ffi::{c_char, CStr, CString};

use dengjen_capi_jni_common::exception::{java_string, run};
use dengjen_capi_jni_common::guard::{jni_failure, Failure, OPERATION_ERROR};
use ffi_support::ExternError;
use jni::objects::{JClass, JFloatArray, JString};
use jni::sys::{jint, jlong, jstring};
use jni::{Env, EnvUnowned};
use libdengjen_stt::{DengjenRecognizer, SttConfig};

use crate::shim;

const INVALID_CONFIGURATION: i32 = 24;
const DECODE_FAILED: i32 = 45;

fn recognizer_ptr(handle: jlong) -> *mut DengjenRecognizer {
    handle as usize as *mut DengjenRecognizer
}

fn to_cstring(text: String) -> Result<CString, Failure> {
    CString::new(text).map_err(|e| Failure {
        code: OPERATION_ERROR,
        message: format!("text contains NUL at byte {}", e.nul_position()),
    })
}

fn check(err: ExternError) -> Result<(), Failure> {
    let code = err.get_code().code();
    if code == 0 {
        return Ok(());
    }
    let raw = err.get_raw_message();
    let message = if raw.is_null() {
        String::new()
    } else {
        // SAFETY: libdengjen-stt hands out NUL-terminated messages that stay valid until freed.
        let text = unsafe { CStr::from_ptr(raw) }
            .to_string_lossy()
            .into_owned();
        // SAFETY: `raw` was allocated by libdengjen-stt for this ExternError and is freed once.
        unsafe { libdengjen_stt::libdengjenSttFreeString(raw.cast_mut()) };
        text
    };
    Err(Failure { code, message })
}

/// Fails if the shim caught a C++ exception during `f`; the text is appended to any failure `f` returned.
fn with_cxx_guard<T>(code: i32, f: impl FnOnce() -> Result<T, Failure>) -> Result<T, Failure> {
    shim::clear();
    let result = f();
    match (shim::take(), result) {
        (Some(message), Ok(_)) => Err(Failure { code, message }),
        (Some(message), Err(failure)) => Err(Failure {
            code: failure.code,
            message: format!("{}: {message}", failure.message),
        }),
        (None, result) => result,
    }
}

fn threads(value: jint) -> Result<i32, Failure> {
    if value < 1 {
        return Err(Failure {
            code: INVALID_CONFIGURATION,
            message: format!("threads must be at least 1, got {value}"),
        });
    }
    Ok(value)
}

#[no_mangle]
pub extern "system" fn Java_io_github_zirekhq_dengjen_android_Recognizer_nativeLoad<'l>(
    mut env: EnvUnowned<'l>,
    _class: JClass<'l>,
    model_dir: JString<'l>,
    expected_sha256: JString<'l>,
    num_threads: jint,
) -> jlong {
    run(&mut env, |env| {
        let dir = to_cstring(java_string(env, &model_dir)?)?;
        let sha = to_cstring(java_string(env, &expected_sha256)?)?;
        let config = SttConfig {
            struct_size: std::mem::size_of::<SttConfig>() as u32,
            num_threads: threads(num_threads)?,
        };
        with_cxx_guard(OPERATION_ERROR, || {
            let mut err = ExternError::success();
            // SAFETY: both strings are NUL-terminated and outlive the call; `config` is a valid
            // struct whose `struct_size` is truthful.
            let ptr = unsafe {
                libdengjen_stt::libdengjenSttLoad(dir.as_ptr(), &config, sha.as_ptr(), &mut err)
            };
            check(err)?;
            Ok(ptr as usize as jlong)
        })
    })
}

#[no_mangle]
pub extern "system" fn Java_io_github_zirekhq_dengjen_android_Recognizer_nativeUnload<'l>(
    mut env: EnvUnowned<'l>,
    _class: JClass<'l>,
    handle: jlong,
) {
    run(&mut env, |_| {
        // SAFETY: the Java side passes a handle from nativeLoad exactly once, after all calls ended.
        unsafe { libdengjen_stt::libdengjenSttUnload(recognizer_ptr(handle)) };
        Ok::<(), Failure>(())
    })
}

fn read_samples(env: &mut Env, array: &JFloatArray) -> Result<Vec<f32>, Failure> {
    let len = array.len(env).map_err(jni_failure)?;
    let mut samples = vec![0f32; len];
    array
        .get_region(env, 0, &mut samples)
        .map_err(jni_failure)?;
    Ok(samples)
}

fn transcribe(handle: jlong, samples: &[f32], sample_rate: jint) -> Result<String, Failure> {
    let mut err = ExternError::success();
    // SAFETY: the Java side holds a read lock so `handle` is live; `samples` is a valid slice.
    let text = unsafe {
        libdengjen_stt::libdengjenSttRecognize(
            recognizer_ptr(handle),
            samples.as_ptr(),
            samples.len(),
            sample_rate,
            &mut err,
        )
    };
    check(err)?;
    owned_text(text)
}

fn owned_text(raw: *mut c_char) -> Result<String, Failure> {
    if raw.is_null() {
        return Err(Failure {
            code: DECODE_FAILED,
            message: "recognizer returned no text".to_owned(),
        });
    }
    // SAFETY: a non-null result is a NUL-terminated string owned by the caller.
    let text = unsafe { CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned();
    // SAFETY: `raw` came from libdengjenSttRecognize and is freed exactly once.
    unsafe { libdengjen_stt::libdengjenSttFreeString(raw) };
    Ok(text)
}

#[no_mangle]
pub extern "system" fn Java_io_github_zirekhq_dengjen_android_Recognizer_nativeRecognize<'l>(
    mut env: EnvUnowned<'l>,
    _class: JClass<'l>,
    handle: jlong,
    samples: JFloatArray<'l>,
    sample_rate: jint,
) -> jstring {
    run(&mut env, |env| {
        let samples = read_samples(env, &samples)?;
        let text = with_cxx_guard(DECODE_FAILED, || transcribe(handle, &samples, sample_rate))?;
        Ok(env.new_string(text).map_err(jni_failure)?.into_raw())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_threads_is_an_invalid_configuration() {
        assert_eq!(threads(0).map_err(|f| f.code), Err(INVALID_CONFIGURATION));
    }

    #[test]
    fn positive_threads_pass_through() {
        assert_eq!(threads(2).ok(), Some(2));
    }

    #[test]
    fn a_caught_exception_turns_success_into_a_failure() {
        let result = with_cxx_guard(DECODE_FAILED, || {
            crate::shim::record_for_test("boom");
            Ok(())
        });
        assert_eq!(
            result,
            Err(Failure {
                code: DECODE_FAILED,
                message: "boom".to_owned()
            })
        );
    }

    #[test]
    fn a_caught_exception_is_appended_to_an_existing_failure() {
        let result: Result<(), Failure> = with_cxx_guard(DECODE_FAILED, || {
            crate::shim::record_for_test("boom");
            Err(Failure {
                code: 43,
                message: "load failed".to_owned(),
            })
        });
        assert_eq!(
            result,
            Err(Failure {
                code: 43,
                message: "load failed: boom".to_owned()
            })
        );
    }
}
