use std::ffi::c_void;

use dengjen_capi_jni_common::exception::{java_string, run};
use dengjen_capi_jni_common::guard::{jni_failure, Failure};
use ffi_support::{ExternError, FfiStr};
use jni::objects::{JClass, JObject, JString};
use jni::sys::{jboolean, jint, jintArray, jlong};
use jni::JNIEnv;
use libdengjen::{AudioInfo, DengjenVoice, SynthesisEvent, SynthesisParams};

use crate::bridge::{trampoline, CallbackState};
use crate::ffi::{check, to_cstring};

type Callback = extern "C" fn(SynthesisEvent, *mut c_void) -> u8;

fn voice_ptr(handle: jlong) -> *mut DengjenVoice {
    handle as usize as *mut DengjenVoice
}

const INVALID_CONFIGURATION: i32 = 24;

fn out_of_range(name: &str, value: jint) -> Failure {
    Failure {
        code: INVALID_CONFIGURATION,
        message: format!("{name} out of range: {value}"),
    }
}

fn byte(name: &str, value: jint) -> Result<u8, Failure> {
    u8::try_from(value).map_err(|_| out_of_range(name, value))
}

fn params(
    (mode, rate, volume, pitch, silence_ms): (jint, jint, jint, jint, jint),
    callback: Option<Callback>,
    user_data: *mut c_void,
) -> Result<SynthesisParams, Failure> {
    Ok(SynthesisParams {
        struct_size: std::mem::size_of::<SynthesisParams>() as u32,
        mode,
        rate: byte("rate", rate)?,
        volume: byte("volume", volume)?,
        pitch: byte("pitch", pitch)?,
        appended_silence_ms: u32::try_from(silence_ms)
            .map_err(|_| out_of_range("appendedSilenceMs", silence_ms))?,
        callback,
        nonblocking: 0,
        user_data,
    })
}

#[no_mangle]
pub extern "system" fn Java_io_github_zirekhq_dengjen_android_Voice_nativeInit<'l>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
) {
    run(&mut env, |_| init_ort())
}

#[cfg(feature = "ort-dylib")]
fn init_ort() -> Result<(), Failure> {
    let path = to_cstring("libonnxruntime.so".to_owned())?;
    let mut err = ExternError::success();
    // SAFETY: `path` is a valid NUL-terminated string that outlives the call.
    unsafe { libdengjen::libdengjenSetOrtDylibPath(FfiStr::from_raw(path.as_ptr()), &mut err) };
    check(err)
}

#[cfg(not(feature = "ort-dylib"))]
fn init_ort() -> Result<(), Failure> {
    Ok(())
}

#[no_mangle]
pub extern "system" fn Java_io_github_zirekhq_dengjen_android_Voice_nativeLoad<'l>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
    config_path: JString<'l>,
) -> jlong {
    run(&mut env, |env| {
        let path = to_cstring(java_string(env, &config_path)?)?;
        let mut err = ExternError::success();
        // SAFETY: `path` is a valid NUL-terminated string that outlives the call.
        let ptr = unsafe {
            libdengjen::libdengjenLoadVoiceFromConfigPath(FfiStr::from_raw(path.as_ptr()), &mut err)
        };
        check(err)?;
        Ok(ptr as usize as jlong)
    })
}

#[no_mangle]
pub extern "system" fn Java_io_github_zirekhq_dengjen_android_Voice_nativeUnload<'l>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
    handle: jlong,
) {
    run(&mut env, |_| {
        // SAFETY: the Java side passes a handle from nativeLoad exactly once, after all calls finished.
        unsafe { libdengjen::libdengjenUnloadDengjenVoice(voice_ptr(handle)) };
        Ok::<(), Failure>(())
    })
}

#[no_mangle]
pub extern "system" fn Java_io_github_zirekhq_dengjen_android_Voice_nativeAudioInfo<'l>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
    handle: jlong,
) -> jintArray {
    run(&mut env, |env| {
        let mut info = AudioInfo {
            sample_rate: 0,
            num_channels: 0,
            sample_width: 0,
        };
        let mut err = ExternError::success();
        // SAFETY: the Java side holds a read lock so `handle` is live; `info` is a valid out pointer.
        unsafe { libdengjen::libdengjenGetAudioInfo(voice_ptr(handle), &mut info, &mut err) };
        check(err)?;
        let values = [
            info.sample_rate as i32,
            info.num_channels as i32,
            info.sample_width as i32,
        ];
        let array = env.new_int_array(3).map_err(jni_failure)?;
        env.set_int_array_region(&array, 0, &values)
            .map_err(jni_failure)?;
        Ok(array.into_raw())
    })
}

#[no_mangle]
pub extern "system" fn Java_io_github_zirekhq_dengjen_android_Voice_nativeSpeak<'l>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
    handle: jlong,
    text: JString<'l>,
    mode: jint,
    rate: jint,
    volume: jint,
    pitch: jint,
    silence_ms: jint,
    handler: JObject<'l>,
) {
    run(&mut env, |env| {
        let text = to_cstring(java_string(env, &text)?)?;
        let state = CallbackState {
            handler: env.new_global_ref(&handler).map_err(jni_failure)?,
        };
        let user_data = std::ptr::from_ref(&state).cast_mut().cast::<c_void>();
        let p = params(
            (mode, rate, volume, pitch, silence_ms),
            Some(trampoline),
            user_data,
        )?;
        let mut err = ExternError::success();
        // SAFETY: `handle` is live (read lock held in Java); `text`, `p` and `state` outlive this blocking call.
        unsafe {
            libdengjen::libdengjenSpeak(
                voice_ptr(handle),
                FfiStr::from_raw(text.as_ptr()),
                &p,
                &mut err,
            )
        };
        check(err)
    })
}

#[no_mangle]
pub extern "system" fn Java_io_github_zirekhq_dengjen_android_Voice_nativeSpeakToFile<'l>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
    handle: jlong,
    text: JString<'l>,
    mode: jint,
    rate: jint,
    volume: jint,
    pitch: jint,
    silence_ms: jint,
    out_path: JString<'l>,
) -> jboolean {
    run(&mut env, |env| {
        let text = to_cstring(java_string(env, &text)?)?;
        let out = to_cstring(java_string(env, &out_path)?)?;
        let p = params(
            (mode, rate, volume, pitch, silence_ms),
            None,
            std::ptr::null_mut(),
        )?;
        let mut err = ExternError::success();
        // SAFETY: `handle` is live (read lock held in Java); every pointer outlives this call.
        let wrote = unsafe {
            libdengjen::libdengjenSpeakToFile(
                voice_ptr(handle),
                FfiStr::from_raw(text.as_ptr()),
                &p,
                FfiStr::from_raw(out.as_ptr()),
                &mut err,
            )
        };
        check(err)?;
        Ok(wrote)
    })
}

#[no_mangle]
pub extern "system" fn Java_io_github_zirekhq_dengjen_android_Voice_nativeCancel<'l>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
    handle: jlong,
) {
    run(&mut env, |_| {
        let mut err = ExternError::success();
        // SAFETY: Java holds `cancelLock`, and `close()` takes it before unloading, so `handle` stays live.
        unsafe { libdengjen::libdengjenCancel(voice_ptr(handle), &mut err) };
        check(err)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(result: Result<SynthesisParams, Failure>) -> Result<(), i32> {
        result.map(|_| ()).map_err(|f| f.code)
    }

    #[test]
    fn rate_above_255_is_rejected() {
        assert_eq!(
            code(params((0, 256, 100, 50, 0), None, std::ptr::null_mut())),
            Err(24)
        );
    }

    #[test]
    fn negative_pitch_is_rejected() {
        assert_eq!(
            code(params((0, 10, 100, -1, 0), None, std::ptr::null_mut())),
            Err(24)
        );
    }

    #[test]
    fn negative_silence_is_rejected() {
        assert_eq!(
            code(params((0, 10, 100, 50, -1), None, std::ptr::null_mut())),
            Err(24)
        );
    }

    #[test]
    fn in_range_values_pass() {
        assert_eq!(
            code(params((0, 255, 0, 50, 0), None, std::ptr::null_mut())),
            Ok(())
        );
    }
}
