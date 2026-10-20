#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)
)]

use std::sync::{Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

mod cancellation;
mod confined_path;
#[cfg(feature = "ort-providers")]
mod ort_providers;

pub use cancellation::CancellationToken;
pub use confined_path::{resolve_within, PathEscapesBase};
#[cfg(feature = "ort-providers")]
pub use ort_providers::{environment, execution_providers, install_environment, session_builder};

/// Returns the guard even when a previous holder panicked; callers must
/// tolerate whatever state that holder left behind.
pub fn lock_ignoring_poison<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Returns the read guard even when a previous writer panicked; callers must
/// tolerate whatever state that writer left behind.
pub fn read_ignoring_poison<T>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(PoisonError::into_inner)
}

/// Returns the write guard even when a previous writer panicked; callers must
/// tolerate whatever state that writer left behind.
pub fn write_ignoring_poison<T>(lock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(PoisonError::into_inner)
}

/// Renders a caught panic payload as text, falling back to a fixed
/// description when the payload is neither `&str` nor `String`.
pub fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|message| (*message).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-string panic payload".to_owned())
}
#[cfg(test)]
mod lock_ignoring_poison_tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn locks_a_mutex_poisoned_by_a_panicking_holder() {
        let mutex = Arc::new(Mutex::new(7));
        let poisoner = Arc::clone(&mutex);
        let _ = std::thread::spawn(move || {
            let _guard = poisoner.lock().unwrap();
            panic!("poison the mutex");
        })
        .join();
        assert!(mutex.is_poisoned());

        assert_eq!(*lock_ignoring_poison(&mutex), 7);
    }
}

#[cfg(test)]
mod rwlock_ignoring_poison_tests {
    use super::*;
    use std::sync::Arc;

    fn poisoned_lock() -> Arc<RwLock<i32>> {
        let lock = Arc::new(RwLock::new(7));
        let poisoner = Arc::clone(&lock);
        let _ = std::thread::spawn(move || {
            let _guard = poisoner.write().unwrap();
            panic!("poison the lock");
        })
        .join();
        assert!(lock.is_poisoned());
        lock
    }

    #[test]
    fn reads_a_lock_poisoned_by_a_panicking_writer() {
        assert_eq!(*read_ignoring_poison(&poisoned_lock()), 7);
    }

    #[test]
    fn writes_a_lock_poisoned_by_a_panicking_writer() {
        let lock = poisoned_lock();
        *write_ignoring_poison(&lock) = 9;
        assert_eq!(*read_ignoring_poison(&lock), 9);
    }
}

#[cfg(test)]
mod panic_message_tests {
    use super::*;

    #[test]
    fn renders_str_and_string_payloads_and_falls_back_for_others() {
        let from_str = std::panic::catch_unwind(|| panic!("static text")).unwrap_err();
        let from_string = std::panic::catch_unwind(|| panic!("{}", "owned text")).unwrap_err();
        let from_other = std::panic::catch_unwind(|| std::panic::panic_any(42_u8)).unwrap_err();

        assert_eq!(panic_message(from_str.as_ref()), "static text");
        assert_eq!(panic_message(from_string.as_ref()), "owned text");
        assert_eq!(
            panic_message(from_other.as_ref()),
            "non-string panic payload"
        );
    }
}
