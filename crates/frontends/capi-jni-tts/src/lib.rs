mod bridge;
mod ffi;
#[cfg(feature = "self-test")]
mod selftest;
mod voice;

use std::ffi::c_void;

use jni::sys::{jint, JNI_ERR, JNI_VERSION_1_6};
use jni::JavaVM;

/// # Safety
/// `vm` must be a non-null pointer to a JavaVM that stays valid for the life of the process.
#[no_mangle]
pub unsafe extern "system" fn JNI_OnLoad(
    vm: *mut jni::sys::JavaVM,
    _reserved: *mut c_void,
) -> jint {
    // SAFETY: the JVM passes its own JavaVM pointer to JNI_OnLoad.
    let vm = unsafe { JavaVM::from_raw(vm) };
    match bridge::install(vm) {
        Ok(()) => JNI_VERSION_1_6,
        Err(_) => JNI_ERR,
    }
}
