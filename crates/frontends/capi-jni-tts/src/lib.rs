mod bridge;
mod ffi;
#[cfg(feature = "self-test")]
mod selftest;
mod voice;

use std::ffi::c_void;

use jni::sys::{jint, JNI_ERR, JNI_VERSION_1_6};
use jni::JavaVM;

#[no_mangle]
pub extern "system" fn JNI_OnLoad(vm: JavaVM, _reserved: *mut c_void) -> jint {
    match bridge::install(vm) {
        Ok(()) => JNI_VERSION_1_6,
        Err(_) => JNI_ERR,
    }
}
