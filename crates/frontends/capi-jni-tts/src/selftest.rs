use std::sync::Arc;

use dengjen_capi_jni_common::exception::run;
use dengjen_capi_jni_common::guard::{jni_failure, Failure};
use jni::objects::{JClass, JObject};
use jni::sys::{jboolean, jint};
use jni::JNIEnv;

use crate::bridge::{deliver, CallbackState, Event};

fn speech() -> Event {
    Event {
        kind: 0,
        data: vec![1, 2, 3],
        code: 0,
        message: String::new(),
    }
}

#[no_mangle]
pub extern "system" fn Java_io_github_zirekhq_dengjen_android_SelfTest_deliverFromNativeThread<
    'l,
>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
    handler: JObject<'l>,
) -> jboolean {
    run(&mut env, |env| {
        let state = Arc::new(CallbackState {
            handler: env.new_global_ref(&handler).map_err(jni_failure)?,
        });
        let worker = Arc::clone(&state);
        let kept = std::thread::spawn(move || deliver(&speech(), &worker))
            .join()
            .map_err(|_| Failure {
                code: -1,
                message: "worker panicked".to_owned(),
            })?;
        Ok(u8::from(kept))
    })
}

#[no_mangle]
pub extern "system" fn Java_io_github_zirekhq_dengjen_android_SelfTest_deliverMany<'l>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
    handler: JObject<'l>,
    n: jint,
) -> jint {
    run(&mut env, |env| {
        let state = CallbackState {
            handler: env.new_global_ref(&handler).map_err(jni_failure)?,
        };
        let delivered = (0..n).filter(|_| deliver(&speech(), &state)).count();
        Ok(delivered as jint)
    })
}

#[no_mangle]
pub extern "system" fn Java_io_github_zirekhq_dengjen_android_SelfTest_panic<'l>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
) {
    run(&mut env, |_| -> Result<(), Failure> {
        panic!("self-test panic")
    })
}
