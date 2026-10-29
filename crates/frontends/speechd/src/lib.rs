#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)
)]

mod protocol;
mod reply;
mod session;
mod settings;

pub use session::{run, Audio, Backend, VoiceInfo};
pub use settings::Settings;
