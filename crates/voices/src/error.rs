use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceError {
    Network(String),
    CatalogueParse(String),
    NoCatalogue,
    UnknownVoice(String),
    InvalidKey(String),
    UnsafePath(String),
    DigestMismatch(PathBuf),
    SizeMismatch(PathBuf),
    Cancelled,
    Busy(String),
    NotInstalled(String),
    Io(PathBuf, io::ErrorKind),
}

pub type VoiceResult<T> = Result<T, VoiceError>;

impl VoiceError {
    pub(crate) fn io(path: &Path, error: &io::Error) -> Self {
        Self::Io(path.to_path_buf(), error.kind())
    }
}

pub(crate) fn io_at(path: &Path) -> impl Fn(io::Error) -> VoiceError + '_ {
    move |error| VoiceError::io(path, &error)
}

impl fmt::Display for VoiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Network(reason) => write!(f, "network error: {reason}"),
            Self::CatalogueParse(reason) => write!(f, "invalid catalogue: {reason}"),
            Self::NoCatalogue => f.write_str("no catalogue loaded"),
            Self::UnknownVoice(key) => write!(f, "unknown voice: {key}"),
            Self::InvalidKey(key) => write!(f, "invalid voice key: {key}"),
            Self::UnsafePath(path) => write!(f, "unsafe file path in catalogue: {path}"),
            Self::DigestMismatch(path) => write!(f, "digest mismatch for {}", path.display()),
            Self::SizeMismatch(path) => write!(f, "size mismatch for {}", path.display()),
            Self::Cancelled => f.write_str("cancelled"),
            Self::Busy(key) => write!(f, "voice {key} is being changed by another call"),
            Self::NotInstalled(key) => write!(f, "voice not installed: {key}"),
            Self::Io(path, kind) => write!(f, "{kind} on {}", path.display()),
        }
    }
}

impl std::error::Error for VoiceError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn display_names_the_failing_path_and_key() {
        let mismatch = VoiceError::DigestMismatch(PathBuf::from("/v/a.onnx"));
        assert_eq!(mismatch.to_string(), "digest mismatch for /v/a.onnx");
        assert_eq!(
            VoiceError::Busy("ku".into()).to_string(),
            "voice ku is being changed by another call"
        );
    }

    #[test]
    fn io_errors_keep_the_path_and_kind() {
        let error = std::io::Error::from(std::io::ErrorKind::NotFound);
        assert_eq!(
            VoiceError::io(Path::new("/x"), &error),
            VoiceError::Io(PathBuf::from("/x"), std::io::ErrorKind::NotFound)
        );
    }
}
