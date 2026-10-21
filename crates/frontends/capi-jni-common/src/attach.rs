use jni::{JNIEnv, JavaVM};

/// Runs `f` on a thread registered with the JVM. A thread this call attached is detached again
/// when it returns; a thread that was already attached stays attached.
pub fn with_attached<T>(vm: &JavaVM, f: impl FnOnce(&mut JNIEnv) -> T) -> Option<T> {
    let mut guard = vm.attach_current_thread().ok()?;
    Some(f(&mut guard))
}
