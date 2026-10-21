use jni::objects::{JString, JThrowable, JValue};
use jni::JNIEnv;

use crate::guard::{guarded, jni_failure, Failure};

pub const EXCEPTION_CLASS: &str = "io/github/zirekhq/dengjen/android/DengjenException";

pub fn throw(env: &mut JNIEnv, failure: &Failure) {
    if env.exception_check().unwrap_or(true) {
        return;
    }
    let built = env.new_string(&failure.message).and_then(|msg| {
        env.new_object(
            EXCEPTION_CLASS,
            "(ILjava/lang/String;)V",
            &[JValue::Int(failure.code), JValue::Object(&msg)],
        )
    });
    match built {
        Ok(obj) => {
            let _ = env.throw(JThrowable::from(obj));
        }
        Err(_) => {
            let _ = env.throw_new("java/lang/RuntimeException", &failure.message);
        }
    }
}

pub fn run<T: Default>(env: &mut JNIEnv, f: impl FnOnce(&mut JNIEnv) -> Result<T, Failure>) -> T {
    match guarded(|| f(env)) {
        Ok(value) => value,
        Err(failure) => {
            throw(env, &failure);
            T::default()
        }
    }
}

pub fn java_string(env: &mut JNIEnv, s: &JString) -> Result<String, Failure> {
    env.get_string(s).map(String::from).map_err(jni_failure)
}
