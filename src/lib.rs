#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
// bindgen emits `mem::transmute` for bitfield accessors on newer clippy, which flags them
// as unnecessary_transmutes; this is generated code we don't control, not worth patching
// bindgen's output for.
#![allow(unnecessary_transmutes)]
// Same story for bindgen's raw bitfield helpers (`raw_get`/`raw_set`/...): no `# Safety`
// docs and a `usize as isize` cast into `.offset()`, both flagged by newer clippy.
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::ptr_offset_with_cast)]

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
