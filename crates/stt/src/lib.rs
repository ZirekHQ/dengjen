#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)
)]

use sha2::{Digest, Sha256};
use sherpa_onnx::{
    OfflineModelConfig, OfflineOmnilingualAsrCtcModelConfig, OfflineRecognizer,
    OfflineRecognizerConfig,
};
use std::fmt::{self, Write as _};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

pub const MODEL_FILE: &str = "model.int8.onnx";
pub const TOKENS_FILE: &str = "tokens.txt";

#[derive(Debug, PartialEq, Eq)]
pub enum SttError {
    MissingModelFile(PathBuf),
    ModelLoad(PathBuf),
    Unreadable(PathBuf),
    ChecksumMismatch(PathBuf),
    InvalidAudio(&'static str),
    Decode,
}

impl fmt::Display for SttError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingModelFile(path) => write!(f, "model file not found: {}", path.display()),
            Self::ModelLoad(dir) => write!(f, "failed to load the model in {}", dir.display()),
            Self::Unreadable(path) => write!(f, "cannot read {}", path.display()),
            Self::ChecksumMismatch(path) => write!(f, "checksum mismatch for {}", path.display()),
            Self::InvalidAudio(reason) => write!(f, "invalid audio: {reason}"),
            Self::Decode => f.write_str("recognition produced no result"),
        }
    }
}

impl std::error::Error for SttError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecognizerConfig {
    pub num_threads: i32,
}

impl Default for RecognizerConfig {
    fn default() -> Self {
        Self { num_threads: 2 }
    }
}

/// Offline recognizer over an omnilingual CTC model directory holding
/// [`MODEL_FILE`] and [`TOKENS_FILE`].
///
/// Loading a corrupt model file aborts the process; call [`verify_model`] first.
pub struct Recognizer(OfflineRecognizer);

fn require_file(dir: &Path, name: &str) -> Result<String, SttError> {
    let path = dir.join(name);
    path.is_file()
        .then(|| path.to_string_lossy().into_owned())
        .ok_or(SttError::MissingModelFile(path))
}

fn validate(samples: &[f32], sample_rate: i32) -> Result<(), SttError> {
    match (samples.is_empty(), sample_rate > 0) {
        (true, _) => Err(SttError::InvalidAudio("no samples")),
        (_, false) => Err(SttError::InvalidAudio("sample rate must be positive")),
        _ => Ok(()),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

fn sha256_hex(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        match file.read(&mut buffer)? {
            0 => return Ok(hex(&hasher.finalize())),
            n => hasher.update(buffer.get(..n).unwrap_or_default()),
        }
    }
}

/// Checks that `model_dir`'s [`MODEL_FILE`] hashes to `expected_sha256` (hex, case-insensitive).
pub fn verify_model(model_dir: &Path, expected_sha256: &str) -> Result<(), SttError> {
    let path = model_dir.join(MODEL_FILE);
    let actual = sha256_hex(&path).map_err(|_| SttError::Unreadable(path.clone()))?;
    actual
        .eq_ignore_ascii_case(expected_sha256.trim())
        .then_some(())
        .ok_or(SttError::ChecksumMismatch(path))
}

impl Recognizer {
    pub fn new(model_dir: &Path, config: RecognizerConfig) -> Result<Self, SttError> {
        let model = require_file(model_dir, MODEL_FILE)?;
        let tokens = require_file(model_dir, TOKENS_FILE)?;
        let config = OfflineRecognizerConfig {
            model_config: OfflineModelConfig {
                omnilingual: OfflineOmnilingualAsrCtcModelConfig { model: Some(model) },
                tokens: Some(tokens),
                num_threads: config.num_threads,
                ..Default::default()
            },
            ..Default::default()
        };
        OfflineRecognizer::create(&config)
            .map(Self)
            .ok_or_else(|| SttError::ModelLoad(model_dir.to_path_buf()))
    }

    /// Transcribes mono `samples` recorded at `sample_rate` Hz; the engine resamples
    /// to the model's rate.
    pub fn recognize(&self, samples: &[f32], sample_rate: i32) -> Result<String, SttError> {
        validate(samples, sample_rate)?;
        let stream = self.0.create_stream();
        stream.accept_waveform(sample_rate, samples);
        self.0.decode(&stream);
        stream.get_result().map(|r| r.text).ok_or(SttError::Decode)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_model_file_names_the_expected_path() {
        let dir = tempfile::tempdir().unwrap();
        let error = Recognizer::new(dir.path(), RecognizerConfig::default()).err();
        assert_eq!(
            error,
            Some(SttError::MissingModelFile(dir.path().join(MODEL_FILE)))
        );
    }

    #[test]
    fn missing_tokens_file_is_reported_after_the_model_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(MODEL_FILE), b"").unwrap();
        let error = Recognizer::new(dir.path(), RecognizerConfig::default()).err();
        assert_eq!(
            error,
            Some(SttError::MissingModelFile(dir.path().join(TOKENS_FILE)))
        );
    }

    #[test]
    fn empty_samples_and_bad_rates_are_invalid_audio() {
        assert_eq!(
            validate(&[], 16_000),
            Err(SttError::InvalidAudio("no samples"))
        );
        assert!(validate(&[0.0], 0).is_err());
    }

    const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    #[test]
    fn verify_model_accepts_a_matching_digest_in_any_case() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(MODEL_FILE), b"").unwrap();
        assert_eq!(verify_model(dir.path(), EMPTY_SHA256), Ok(()));
        assert_eq!(
            verify_model(dir.path(), &EMPTY_SHA256.to_uppercase()),
            Ok(())
        );
    }

    #[test]
    fn verify_model_rejects_other_content() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(MODEL_FILE), b"x").unwrap();
        assert_eq!(
            verify_model(dir.path(), EMPTY_SHA256),
            Err(SttError::ChecksumMismatch(dir.path().join(MODEL_FILE)))
        );
    }

    #[test]
    fn verify_model_reports_a_missing_file_as_unreadable() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            verify_model(dir.path(), EMPTY_SHA256),
            Err(SttError::Unreadable(dir.path().join(MODEL_FILE)))
        );
    }
}
