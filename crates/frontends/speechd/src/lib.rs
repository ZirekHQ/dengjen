#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)
)]

mod lines;
mod protocol;
mod reply;
mod session;
mod settings;
mod ssml;

pub use session::{run, Audio, Backend, Chunks, VoiceInfo};
pub use settings::Settings;
