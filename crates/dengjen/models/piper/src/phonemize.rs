use crate::config::{ModelConfig, PhonemeType};
use dengjen_tts_core::{DengjenError, DengjenResult, Phonemes};
use std::path::Path;

#[cfg(feature = "tashkeel")]
pub(crate) type TashkeelEngine = dengjen_tashkeel::DynamicInferenceEngine;
#[cfg(not(feature = "tashkeel"))]
pub(crate) type TashkeelEngine = ();

#[cfg(feature = "hebrew")]
pub(crate) type HebrewEngine = dengjen_hebrew_phonemizer::NakdimonEngine;
#[cfg(not(feature = "hebrew"))]
pub(crate) type HebrewEngine = ();

#[cfg(feature = "pinyin")]
pub(crate) type PinyinEngine = dengjen_pinyin_phonemizer::PinyinEngine;
#[cfg(not(feature = "pinyin"))]
pub(crate) type PinyinEngine = ();

#[cfg(feature = "tashkeel")]
pub(crate) fn should_diacritize(voice: &str) -> bool {
    voice == "ar"
}
#[cfg(not(feature = "tashkeel"))]
#[cfg_attr(not(feature = "espeak"), allow(dead_code))]
pub(crate) fn should_diacritize(_voice: &str) -> bool {
    false
}

pub(crate) fn phonemize_dispatch(
    phoneme_type: PhonemeType,
    text: &str,
    #[cfg_attr(not(feature = "hebrew"), allow(unused_variables))] hebrew_engine: Option<
        &HebrewEngine,
    >,
    #[cfg_attr(not(feature = "pinyin"), allow(unused_variables))] pinyin_engine: Option<
        &PinyinEngine,
    >,
) -> Option<DengjenResult<Phonemes>> {
    match phoneme_type {
        PhonemeType::Espeak => None,
        PhonemeType::Text => Some(Ok(vec![text.to_string()].into())),
        #[cfg(feature = "hebrew")]
        PhonemeType::Hebrew => Some(match hebrew_engine {
            Some(engine) => dengjen_hebrew_phonemizer::text_to_hebrew_phonemes(engine, text),
            None => Err(DengjenError::PhonemizationError(
                "This voice's phoneme_type is `hebrew` but no Hebrew engine was initialized"
                    .to_string(),
            )),
        }),
        #[cfg(not(feature = "hebrew"))]
        PhonemeType::Hebrew => Some(Err(DengjenError::PhonemizationError(
            "Phonemization for phoneme_type `Hebrew` requires the `hebrew` feature".to_string(),
        ))),
        #[cfg(feature = "pinyin")]
        PhonemeType::Pinyin => Some(match pinyin_engine {
            Some(engine) => dengjen_pinyin_phonemizer::text_to_pinyin_phonemes(engine, text),
            None => Err(DengjenError::PhonemizationError(
                "This voice's phoneme_type is `pinyin` but no pinyin engine was initialized"
                    .to_string(),
            )),
        }),
        #[cfg(not(feature = "pinyin"))]
        PhonemeType::Pinyin => Some(Err(DengjenError::PhonemizationError(
            "Phonemization for phoneme_type `Pinyin` requires the `pinyin` feature".to_string(),
        ))),
    }
}

#[cfg(feature = "tashkeel")]
fn share_ort_environment() -> DengjenResult<()> {
    static SHARED: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
    SHARED
        .get_or_init(|| {
            let environment = dengjen_tts_core::environment().map_err(|e| e.to_string())?;
            dengjen_tashkeel::install_environment(environment.clone()).map_err(|e| e.to_string())
        })
        .clone()
        .map_err(DengjenError::InferenceError)
}

#[cfg(feature = "tashkeel")]
pub(crate) fn create_tashkeel_engine(
    config: &ModelConfig,
) -> DengjenResult<Option<TashkeelEngine>> {
    if !should_diacritize(&config.espeak.voice) {
        return Ok(None);
    }
    share_ort_environment()?;
    dengjen_tashkeel::create_inference_engine(None)
        .map(Some)
        .map_err(|msg| {
            DengjenError::InferenceError(format!(
                "Failed to create inference engine for libtashkeel. {}",
                msg
            ))
        })
}
#[cfg(not(feature = "tashkeel"))]
pub(crate) fn create_tashkeel_engine(
    _config: &ModelConfig,
) -> DengjenResult<Option<TashkeelEngine>> {
    Ok(None)
}

#[cfg(feature = "hebrew")]
pub(crate) fn create_hebrew_engine(
    config: &ModelConfig,
    config_path: &Path,
) -> DengjenResult<Option<HebrewEngine>> {
    if config.phoneme_type != Some(PhonemeType::Hebrew) {
        return Ok(None);
    }
    let Some(model_path) = config.hebrew_model_path.as_ref() else {
        return Err(DengjenError::InvalidConfiguration(
            "This voice's phoneme_type is `hebrew` but no Nakdimon model path was configured"
                .to_string(),
        ));
    };

    let resolved_model_path = resolve_hebrew_model_path(config_path, model_path);
    dengjen_hebrew_phonemizer::create_nakdimon_engine(&resolved_model_path).map(Some)
}

#[cfg(feature = "hebrew")]
fn resolve_hebrew_model_path(config_path: &Path, model_path: &Path) -> std::path::PathBuf {
    config_path.with_file_name(model_path)
}
#[cfg(not(feature = "hebrew"))]
pub(crate) fn create_hebrew_engine(
    _config: &ModelConfig,
    _config_path: &Path,
) -> DengjenResult<Option<HebrewEngine>> {
    Ok(None)
}

#[cfg(feature = "pinyin")]
pub(crate) fn create_pinyin_engine(
    config: &ModelConfig,
    config_path: &Path,
) -> DengjenResult<Option<PinyinEngine>> {
    if config.phoneme_type != Some(PhonemeType::Pinyin) {
        return Ok(None);
    }
    let Some(model_dir) = config.pinyin_model_dir.as_ref() else {
        return Err(DengjenError::InvalidConfiguration(
            "This voice's phoneme_type is `pinyin` but no g2pW model directory was configured"
                .to_string(),
        ));
    };

    let resolved_model_dir = resolve_pinyin_model_dir(config_path, model_dir);
    dengjen_pinyin_phonemizer::create_pinyin_engine(&resolved_model_dir).map(Some)
}

#[cfg(feature = "pinyin")]
fn resolve_pinyin_model_dir(config_path: &Path, model_dir: &Path) -> std::path::PathBuf {
    config_path.with_file_name(model_dir)
}
#[cfg(not(feature = "pinyin"))]
pub(crate) fn create_pinyin_engine(
    _config: &ModelConfig,
    _config_path: &Path,
) -> DengjenResult<Option<PinyinEngine>> {
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phonemize_dispatch_falls_through_to_espeak_for_espeak_phoneme_type() {
        assert!(phonemize_dispatch(PhonemeType::Espeak, "hello", None, None).is_none());
    }

    #[cfg(feature = "hebrew")]
    #[test]
    fn hebrew_model_path_is_resolved_relative_to_the_config_file() {
        let config_path = Path::new("voices/he-IL/voice.onnx.json");
        let model_path = Path::new("nakdimon.onnx");

        assert_eq!(
            resolve_hebrew_model_path(config_path, model_path),
            Path::new("voices/he-IL/nakdimon.onnx")
        );
    }

    #[test]
    fn phonemize_dispatch_passes_text_through_unchanged_for_text_phoneme_type() {
        let result = phonemize_dispatch(PhonemeType::Text, "hello", None, None)
            .unwrap()
            .unwrap();
        assert_eq!(result.sentences(), &vec!["hello".to_string()]);
    }

    #[cfg(feature = "hebrew")]
    #[test]
    fn phonemize_dispatch_delegates_hebrew_to_the_hebrew_engine_when_present() {
        let result = phonemize_dispatch(
            PhonemeType::Hebrew,
            "\u{05E9}\u{05DC}\u{05D5}\u{05DD}",
            None,
            None,
        );
        assert!(result.unwrap().is_err());
    }

    #[cfg(not(feature = "hebrew"))]
    #[test]
    fn phonemize_dispatch_still_errors_on_hebrew_when_feature_disabled() {
        let result = phonemize_dispatch(PhonemeType::Hebrew, "hello", None, None).unwrap();
        assert!(result.is_err());
    }

    #[cfg(feature = "pinyin")]
    #[test]
    fn phonemize_dispatch_delegates_pinyin_to_the_pinyin_engine_when_present() {
        let result = phonemize_dispatch(PhonemeType::Pinyin, "\u{4F60}\u{597D}", None, None);
        assert!(result.unwrap().is_err());
    }

    #[cfg(not(feature = "pinyin"))]
    #[test]
    fn phonemize_dispatch_still_errors_on_pinyin_when_feature_disabled() {
        let result = phonemize_dispatch(PhonemeType::Pinyin, "hello", None, None).unwrap();
        assert!(result.is_err());
    }

    #[cfg(feature = "tashkeel")]
    #[test]
    fn should_diacritize_true_for_arabic_voice_when_tashkeel_enabled() {
        assert!(should_diacritize("ar"));
    }

    #[cfg(feature = "tashkeel")]
    #[test]
    fn should_diacritize_false_for_non_arabic_voice_when_tashkeel_enabled() {
        assert!(!should_diacritize("en-us"));
    }

    #[cfg(not(feature = "tashkeel"))]
    #[test]
    fn should_diacritize_always_false_when_tashkeel_disabled() {
        assert!(!should_diacritize("ar"));
        assert!(!should_diacritize("en-us"));
    }
}
