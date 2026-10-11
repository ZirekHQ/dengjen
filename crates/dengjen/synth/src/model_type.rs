use std::fmt;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;

use dengjen_tts_core::{DengjenError, DengjenModel, DengjenResult};

/// Voice backend selected by the `model_type` field of a voice config.
///
/// Parsing is exact and case-sensitive. A config without `model_type` is a
/// Piper voice; an unrecognised value is an error, never a silent Piper
/// fallback.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ModelType {
    /// `"piper"` or `"vits"`.
    Piper,
    /// `"kokoro"`.
    Kokoro,
    /// `"melotts"`.
    MeloTts,
}

impl ModelType {
    /// Canonical config spelling of this backend.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Piper => "piper",
            Self::Kokoro => "kokoro",
            Self::MeloTts => "melotts",
        }
    }
}

impl fmt::Display for ModelType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ModelType {
    type Err = DengjenError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "piper" | "vits" => Ok(Self::Piper),
            "kokoro" => Ok(Self::Kokoro),
            "melotts" => Ok(Self::MeloTts),
            other => Err(DengjenError::InvalidConfiguration(format!(
                "Unsupported model_type `{other}`: expected one of `piper`, `vits`, `kokoro`, `melotts`"
            ))),
        }
    }
}

/// Reads and parses the `model_type` of the voice config at `config_path`.
///
/// # Errors
///
/// Returns an error when the file cannot be read or parsed, or when
/// `model_type` is not a [`ModelType`] spelling.
pub fn read_model_type(config_path: &Path) -> DengjenResult<ModelType> {
    crate::detect_model_type(config_path)?.parse()
}

/// Detects the backend of the voice config at `config_path` and hands it to
/// `load`, which builds the model.
///
/// `load` receives the detected [`ModelType`] and `config_path`.
///
/// # Errors
///
/// Returns the detection error, or whatever `load` returns.
pub fn load_voice_with<F>(
    config_path: &Path,
    load: F,
) -> DengjenResult<Arc<dyn DengjenModel + Send + Sync>>
where
    F: FnOnce(ModelType, &Path) -> DengjenResult<Arc<dyn DengjenModel + Send + Sync>>,
{
    load(read_model_type(config_path)?, config_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_config(name: &str, contents: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("dengjen_model_type_{name}"));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn from_str_accepts_every_supported_spelling() {
        assert_eq!("piper".parse::<ModelType>().unwrap(), ModelType::Piper);
        assert_eq!("vits".parse::<ModelType>().unwrap(), ModelType::Piper);
        assert_eq!("kokoro".parse::<ModelType>().unwrap(), ModelType::Kokoro);
        assert_eq!("melotts".parse::<ModelType>().unwrap(), ModelType::MeloTts);
    }

    #[test]
    fn from_str_rejects_unknown_and_wrongly_cased_values() {
        for value in ["", "Kokoro", "melo", "piperr"] {
            let result = value.parse::<ModelType>();
            assert!(
                matches!(result, Err(DengjenError::InvalidConfiguration(ref msg)) if msg.contains(value)),
                "`{value}` must be rejected"
            );
        }
    }

    #[test]
    fn display_round_trips_through_from_str() {
        for model_type in [ModelType::Piper, ModelType::Kokoro, ModelType::MeloTts] {
            assert_eq!(
                model_type.to_string().parse::<ModelType>().unwrap(),
                model_type
            );
        }
    }

    #[test]
    fn read_model_type_defaults_to_piper_when_the_field_is_absent() {
        let path = write_config("absent", "{}");
        assert_eq!(read_model_type(&path).unwrap(), ModelType::Piper);
    }

    #[test]
    fn read_model_type_rejects_an_unknown_model_type() {
        let path = write_config("unknown", r#"{"model_type": "bark"}"#);
        assert!(matches!(
            read_model_type(&path),
            Err(DengjenError::InvalidConfiguration(_))
        ));
    }

    #[test]
    fn load_voice_with_passes_the_detected_type_to_the_loader() {
        let path = write_config("dispatch", r#"{"model_type": "kokoro"}"#);
        let result = load_voice_with(&path, |model_type, _| {
            Err(DengjenError::OperationError(format!("saw {model_type}")))
        });
        assert!(matches!(result, Err(DengjenError::OperationError(msg)) if msg == "saw kokoro"));
    }

    #[test]
    fn load_voice_with_skips_the_loader_when_detection_fails() {
        let path = write_config("skip", r#"{"model_type": "bark"}"#);
        let result = load_voice_with(&path, |_, _| {
            Err(DengjenError::OperationError("loader ran".to_string()))
        });
        assert!(matches!(result, Err(DengjenError::InvalidConfiguration(_))));
    }
}
