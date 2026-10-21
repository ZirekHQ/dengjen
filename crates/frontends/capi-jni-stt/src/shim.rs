use std::ffi::c_char;

extern "C" {
    #[cfg(test)]
    fn dengjen_shim_record_error(message: *const c_char);
    fn dengjen_shim_clear_error();
    fn dengjen_shim_take_error(buf: *mut c_char, cap: usize) -> usize;
    #[cfg(test)]
    fn dengjen_shim_selftest_throw() -> i32;
}

pub fn clear() {
    // SAFETY: the shim only resets its own thread-local string.
    unsafe { dengjen_shim_clear_error() };
}

/// Drains the message of the C++ exception the shim caught on this thread since the last `clear` or `take`.
pub fn take() -> Option<String> {
    let mut buf = [0u8; 512];
    // SAFETY: `buf` is writable for its full length and the shim NUL-terminates within `cap`.
    let len = unsafe { dengjen_shim_take_error(buf.as_mut_ptr().cast(), buf.len()) };
    let bytes = buf.get(..len)?;
    (len > 0).then(|| String::from_utf8_lossy(bytes).into_owned())
}

#[cfg(test)]
pub fn record_for_test(message: &str) {
    let text = std::ffi::CString::new(message).unwrap_or_default();
    // SAFETY: `text` is NUL-terminated and outlives the call; the shim copies it.
    unsafe { dengjen_shim_record_error(text.as_ptr()) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_caught_cxx_exception_is_reported_once() {
        clear();
        // SAFETY: the self-test throws and catches inside the shim.
        assert_eq!(unsafe { dengjen_shim_selftest_throw() }, 1);
        assert_eq!(take().as_deref(), Some("selftest exception"));
        assert_eq!(take(), None);
    }

    #[test]
    fn nothing_is_reported_without_an_exception() {
        clear();
        assert_eq!(take(), None);
    }
}
