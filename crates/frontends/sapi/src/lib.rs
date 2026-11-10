//! A SAPI5 voice backed by dengjen.
//!
//! SAPI5 loads a voice as an in-process COM server, so this crate builds a DLL that Narrator,
//! NVDA's SAPI5 driver and any other SAPI5 client load into their own process. Everything that
//! does not touch COM lives in platform-neutral modules that forbid `unsafe` and are tested on
//! every OS; the [`windows`] module only translates between those modules and SAPI's interfaces.
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)
)]

#[cfg(feature = "engine")]
pub mod engine;
#[cfg(not(feature = "engine"))]
#[path = "engine_stub.rs"]
pub mod engine;
pub mod fragments;
pub mod language;
pub mod log;
pub mod prosody;
pub mod registration;
pub mod speak;
pub mod timing;
#[cfg(windows)]
mod windows;
