//! Stands in for [`crate::engine`] when the crate is built without the `engine` feature, so the
//! SAPI plumbing type-checks for Windows from any host. Every call fails with the same message.
#![forbid(unsafe_code)]

use std::path::Path;

use crate::prosody::EngineProsody;
use crate::speak::Synth;

const MISSING: &str = "dengjen-sapi was built without the `engine` feature";

pub fn init_runtime(_dll_dir: &Path) -> Result<(), String> {
    Err(MISSING.to_owned())
}

pub struct Engine;

impl Engine {
    pub fn load(_config_path: &Path) -> Result<Self, String> {
        Err(MISSING.to_owned())
    }
}

impl Synth for Engine {
    fn sample_rate(&self) -> u32 {
        0
    }

    fn synthesize(
        &self,
        _text: &str,
        _prosody: EngineProsody,
        _chunk: &mut dyn FnMut(&[i16]) -> bool,
    ) -> Result<(), String> {
        Err(MISSING.to_owned())
    }

    /// Nothing runs without the native library, so there is nothing to stop.
    fn interrupt(&self) {}

    fn language(&self) -> &str {
        ""
    }
}
