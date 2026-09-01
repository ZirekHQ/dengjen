#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
// bindgen emits `mem::transmute` for bitfield accessors on newer clippy, which flags them
// as unnecessary_transmutes; this is generated code we don't control, not worth patching
// bindgen's output for.
#![allow(unnecessary_transmutes)]

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
