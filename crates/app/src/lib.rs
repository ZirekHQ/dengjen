//! Logic shared by the Dengjen apps: the sentences a read-aloud session steps
//! through, and plain text from the files a person opens.
#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)
)]

pub mod import;
mod sentences;

pub use sentences::{sentence_at, sentences, Sentence};
