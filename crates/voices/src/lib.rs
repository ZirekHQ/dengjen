#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)
)]

pub mod catalogue;
pub mod digest;
pub mod error;
pub(crate) mod idle;
pub mod manager;
pub(crate) mod store;
pub mod transport;
pub(crate) mod validate;

pub use catalogue::{Catalogue, Language, Voice, VoiceFile};
pub use digest::Digest;
pub use error::{VoiceError, VoiceResult};
pub use manager::{Config, Progress, VoiceManager};
pub use store::InstalledVoice;
pub use transport::{Response, Transport, UreqTransport};
