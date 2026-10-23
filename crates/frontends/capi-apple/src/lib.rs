//! Links the C API crates into one archive. Two Rust static libraries in one app each carry a copy
//! of the Rust runtime and collide at link time.

extern crate libdengjen;
#[cfg(feature = "stt")]
extern crate libdengjen_stt;
