use jni::errors::ThrowRuntimeExAndDefault;
use jni::objects::{JString, JThrowable, JValue};
use jni::strings::JNIString;
use jni::{jni_sig, jni_str, Env, EnvUnowned};

use crate::guard::{guarded, jni_failure, Failure};

pub fn throw(env: &mut Env, failure: &Failure) {
    if env.exception_check() {
        return;
    }
    let built = env.new_string(&failure.message).and_then(|msg| {
        env.new_object(
            jni_str!("io/github/zirekhq/dengjen/android/DengjenException"),
            jni_sig!("(ILjava/lang/String;)V"),
            &[JValue::Int(failure.code), JValue::Object(&msg)],
        )
    });
    match built.and_then(|obj| env.cast_local::<JThrowable>(obj)) {
        Ok(throwable) => {
            let _ = env.throw(throwable);
        }
        Err(_) => {
            let _ = env.throw_new(
                jni_str!("java/lang/RuntimeException"),
                JNIString::from(failure.message.as_str()),
            );
        }
    }
}

pub fn run<T: Default>(
    unowned: &mut EnvUnowned,
    f: impl FnOnce(&mut Env) -> Result<T, Failure>,
) -> T {
    unowned
        .with_env(|env| -> jni::errors::Result<T> {
            Ok(match guarded(|| f(env)) {
                Ok(value) => value,
                Err(failure) => {
                    throw(env, &failure);
                    T::default()
                }
            })
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

pub fn java_string(env: &mut Env, s: &JString) -> Result<String, Failure> {
    s.try_to_string(env).map_err(jni_failure)
}
