//! Speech recognition behind the `Recognize` RPC: PCM decoding, the recognizer seam and its env config.

use std::path::PathBuf;
use std::sync::Arc;

pub(crate) const MODEL_DIR_VAR: &str = "DENGJEN_GRPC_STT_MODEL_DIR";
pub(crate) const MODEL_SHA256_VAR: &str = "DENGJEN_GRPC_STT_MODEL_SHA256";

// `Failed` is only constructed by the `stt` adapter and by tests.
#[cfg_attr(not(any(test, feature = "stt")), allow(dead_code))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RecognitionError {
    InvalidAudio(String),
    Failed,
}

pub(crate) trait Transcriber: Send + Sync {
    fn transcribe(&self, samples: &[f32], sample_rate: i32) -> Result<String, RecognitionError>;
}

pub(crate) fn pcm_s16le_to_f32(pcm: &[u8]) -> Result<Vec<f32>, RecognitionError> {
    let (pairs, rest) = pcm.as_chunks::<2>();
    if !rest.is_empty() {
        return Err(RecognitionError::InvalidAudio(
            "PCM byte count must be even".to_string(),
        ));
    }
    Ok(pairs
        .iter()
        .map(|bytes| f32::from(i16::from_le_bytes(*bytes)) / 32768.0)
        .collect())
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct SttConfig {
    pub model_dir: PathBuf,
    pub sha256: String,
}

pub(crate) fn stt_config_from_lookup(
    lookup: impl Fn(&str) -> Option<String>,
) -> Result<Option<SttConfig>, String> {
    let get = |name: &str| {
        lookup(name)
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    };
    match (get(MODEL_DIR_VAR), get(MODEL_SHA256_VAR)) {
        (None, None) => Ok(None),
        (Some(dir), Some(sha256)) => Ok(Some(SttConfig {
            model_dir: PathBuf::from(dir),
            sha256,
        })),
        _ => Err(format!(
            "{MODEL_DIR_VAR} and {MODEL_SHA256_VAR} must be set together"
        )),
    }
}

#[cfg(feature = "stt")]
impl Transcriber for dengjen_stt::Recognizer {
    fn transcribe(&self, samples: &[f32], sample_rate: i32) -> Result<String, RecognitionError> {
        self.recognize(samples, sample_rate)
            .map_err(|err| match err {
                dengjen_stt::SttError::InvalidAudio(reason) => {
                    RecognitionError::InvalidAudio(reason.to_string())
                }
                other => {
                    log::warn!("recognition failed: {other}");
                    RecognitionError::Failed
                }
            })
    }
}

#[cfg(feature = "stt")]
pub(crate) fn load_transcriber(
    config: Option<SttConfig>,
) -> Result<Option<Arc<dyn Transcriber>>, String> {
    let Some(config) = config else {
        return Ok(None);
    };
    dengjen_stt::verify_model(&config.model_dir, &config.sha256).map_err(|e| e.to_string())?;
    let recognizer =
        dengjen_stt::Recognizer::new(&config.model_dir, dengjen_stt::RecognizerConfig::default())
            .map_err(|e| e.to_string())?;
    Ok(Some(Arc::new(recognizer)))
}

#[cfg(not(feature = "stt"))]
pub(crate) fn load_transcriber(
    config: Option<SttConfig>,
) -> Result<Option<Arc<dyn Transcriber>>, String> {
    config.map_or(Ok(None), |_| {
        Err(format!(
            "{MODEL_DIR_VAR} is set but this build lacks the `stt` feature"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn lookup_from(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |name| map.get(name).cloned()
    }

    #[test]
    fn pcm_decodes_little_endian_samples_to_unit_range() {
        let pcm = [0x00, 0x40, 0x00, 0xC0];
        assert_eq!(pcm_s16le_to_f32(&pcm), Ok(vec![0.5, -0.5]));
    }

    #[test]
    fn pcm_extremes_stay_within_unit_range() {
        let pcm = [0x00, 0x80, 0xFF, 0x7F];
        let samples = pcm_s16le_to_f32(&pcm).unwrap();
        assert_eq!(samples.first(), Some(&-1.0));
        assert!(samples.get(1).is_some_and(|s| *s < 1.0 && *s > 0.999));
    }

    #[test]
    fn pcm_with_an_odd_byte_count_is_invalid_audio() {
        assert!(matches!(
            pcm_s16le_to_f32(&[0x00, 0x01, 0x02]),
            Err(RecognitionError::InvalidAudio(_))
        ));
    }

    #[test]
    fn empty_pcm_decodes_to_no_samples() {
        assert_eq!(pcm_s16le_to_f32(&[]), Ok(vec![]));
    }

    #[test]
    fn neither_variable_means_recognition_is_not_configured() {
        assert_eq!(stt_config_from_lookup(lookup_from(&[])), Ok(None));
    }

    #[test]
    fn both_variables_build_the_config() {
        let config = stt_config_from_lookup(lookup_from(&[
            (MODEL_DIR_VAR, " /models/ku "),
            (MODEL_SHA256_VAR, "ab12"),
        ]));
        assert_eq!(
            config,
            Ok(Some(SttConfig {
                model_dir: PathBuf::from("/models/ku"),
                sha256: "ab12".to_string(),
            }))
        );
    }

    #[test]
    fn only_one_variable_is_a_config_error() {
        let only_dir = stt_config_from_lookup(lookup_from(&[(MODEL_DIR_VAR, "/m")]));
        let only_sha = stt_config_from_lookup(lookup_from(&[(MODEL_SHA256_VAR, "ab")]));
        assert!(only_dir.is_err());
        assert!(only_sha.is_err());
    }

    #[test]
    fn whitespace_only_values_count_as_unset() {
        let config = stt_config_from_lookup(lookup_from(&[
            (MODEL_DIR_VAR, "  "),
            (MODEL_SHA256_VAR, ""),
        ]));
        assert_eq!(config, Ok(None));
    }
}

#[cfg(test)]
mod loading_tests {
    use super::*;

    #[test]
    fn no_config_loads_no_transcriber() {
        assert!(matches!(load_transcriber(None), Ok(None)));
    }

    #[cfg(not(feature = "stt"))]
    #[test]
    fn config_in_a_build_without_stt_fails_startup() {
        let config = SttConfig {
            model_dir: PathBuf::from("/m"),
            sha256: "ab".to_string(),
        };
        assert!(load_transcriber(Some(config)).is_err());
    }

    #[cfg(feature = "stt")]
    #[test]
    fn a_checksum_mismatch_fails_startup() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(dengjen_stt::MODEL_FILE), b"not a model").unwrap();
        let config = SttConfig {
            model_dir: dir.path().to_path_buf(),
            sha256: "0".repeat(64),
        };
        let result = load_transcriber(Some(config));
        assert!(matches!(result, Err(message) if message.contains("checksum mismatch")));
    }

    #[cfg(feature = "stt")]
    #[test]
    fn a_missing_model_fails_startup() {
        let dir = tempfile::tempdir().unwrap();
        let config = SttConfig {
            model_dir: dir.path().to_path_buf(),
            sha256: "0".repeat(64),
        };
        assert!(load_transcriber(Some(config)).is_err());
    }

    // Needs a real model: DENGJEN_STT_MODEL_DIR must be a directory `Recognizer::new` accepts.
    #[cfg(feature = "stt")]
    #[test]
    fn the_real_recognizer_rejects_empty_and_too_short_audio() {
        let Some(model_dir) = std::env::var_os("DENGJEN_STT_MODEL_DIR") else {
            eprintln!("skipping: DENGJEN_STT_MODEL_DIR not set");
            return;
        };
        let recognizer = dengjen_stt::Recognizer::new(
            std::path::Path::new(&model_dir),
            dengjen_stt::RecognizerConfig::default(),
        )
        .unwrap();
        assert!(matches!(
            Transcriber::transcribe(&recognizer, &[], 16_000),
            Err(RecognitionError::InvalidAudio(_))
        ));
        assert!(matches!(
            Transcriber::transcribe(&recognizer, &[0.0; 100], 16_000),
            Err(RecognitionError::InvalidAudio(_))
        ));
        assert!(matches!(
            Transcriber::transcribe(&recognizer, &[0.0; 1600], 0),
            Err(RecognitionError::InvalidAudio(_))
        ));
    }
}
