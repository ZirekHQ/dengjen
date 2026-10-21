use jni::{Env, JavaVM};

/// Runs `f` on a thread registered with the JVM. A thread this call attached is detached again
/// when it returns; a thread that was already attached stays attached.
pub fn with_attached<T>(vm: &JavaVM, f: impl FnOnce(&mut Env) -> T) -> Option<T> {
    vm.attach_current_thread_for_scope(|env| Ok::<T, jni::errors::Error>(f(env)))
        .ok()
}
