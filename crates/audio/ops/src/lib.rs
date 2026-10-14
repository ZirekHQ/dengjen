#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)
)]

pub(crate) mod hanning_window;
mod samples;
mod wave_writer;

pub use hanning_window::AudioOpsError;
pub use libsonic::{SonicError, SonicStream};
pub use samples::{Audio, AudioInfo, AudioSamples};
pub use wave_writer::{write_wave_samples_to_buffer, write_wave_samples_to_file, WaveWriterError};
