use crate::config::PhonemizerConfig;
use dengjen_tts_core::{DengjenError, DengjenResult};

#[cfg(feature = "pinyin")]
pub(crate) type PinyinBackend = dengjen_pinyin_phonemizer::PinyinEngine;
#[cfg(not(feature = "pinyin"))]
pub(crate) type PinyinBackend = ();

#[cfg(feature = "pinyin")]
pub(crate) fn create_pinyin_backend(model_dir: &std::path::Path) -> DengjenResult<PinyinBackend> {
    dengjen_pinyin_phonemizer::create_pinyin_engine(model_dir)
}
#[cfg(not(feature = "pinyin"))]
pub(crate) fn create_pinyin_backend(_model_dir: &std::path::Path) -> DengjenResult<PinyinBackend> {
    Err(DengjenError::PhonemizationError(
        "MeloTTS pinyin phonemization requires the `pinyin` feature, but it is disabled"
            .to_string(),
    ))
}

#[cfg(feature = "pinyin")]
pub(crate) fn pinyin_phone_tone_pairs(
    engine: &PinyinBackend,
    text: &str,
) -> DengjenResult<Vec<Vec<(String, String)>>> {
    let token_sentences = dengjen_pinyin_phonemizer::text_to_pinyin_tokens(engine, text)?;
    Ok(token_sentences
        .into_iter()
        .map(|tokens| {
            tokens
                .into_iter()
                .flat_map(|token| match token {
                    dengjen_pinyin_phonemizer::PinyinToken::Syllable {
                        initial,
                        finale,
                        tone,
                    } => {
                        let tone_symbol = tone.to_string();
                        let mut pairs = Vec::new();
                        if !initial.is_empty() {
                            pairs.push((initial, tone_symbol.clone()));
                        }
                        pairs.push((finale, tone_symbol));
                        pairs
                    }
                    dengjen_pinyin_phonemizer::PinyinToken::Passthrough(s) => {
                        vec![(s, "_".to_string())]
                    }
                    // `PinyinToken` is #[non_exhaustive]; a token kind this crate predates has no
                    // phone mapping.
                    _ => Vec::new(),
                })
                .collect()
        })
        .collect())
}
#[cfg(not(feature = "pinyin"))]
pub(crate) fn pinyin_phone_tone_pairs(
    _engine: &PinyinBackend,
    _text: &str,
) -> DengjenResult<Vec<Vec<(String, String)>>> {
    Err(DengjenError::PhonemizationError(
        "MeloTTS pinyin phonemization requires the `pinyin` feature, but it is disabled"
            .to_string(),
    ))
}

#[cfg(feature = "espeak")]
pub(crate) fn espeak_phone_tone_pairs(
    text: &str,
    voice: &str,
) -> DengjenResult<Vec<Vec<(String, String)>>> {
    let sentences = dengjen_espeak_phonemizer::text_to_phonemes(text, voice, None, true, false)
        .map_err(|e| DengjenError::PhonemizationError(e.to_string()))?;
    Ok(sentences
        .into_iter()
        .map(|sentence| vec![(sentence, "_".to_string())])
        .collect())
}

#[cfg(not(feature = "espeak"))]
pub(crate) fn espeak_phone_tone_pairs(
    _text: &str,
    _voice: &str,
) -> DengjenResult<Vec<Vec<(String, String)>>> {
    Err(DengjenError::PhonemizationError(
        "MeloTTS espeak phonemization requires the `espeak` feature (GPL-3.0-or-later, via espeak-ng), but it is disabled".to_string(),
    ))
}

pub(crate) enum PhonemizerBackend {
    Espeak { voice: String },
    Pinyin(Box<PinyinBackend>),
}

pub(crate) fn create_backend(config: &PhonemizerConfig) -> DengjenResult<PhonemizerBackend> {
    match config {
        PhonemizerConfig::Espeak { voice } => Ok(PhonemizerBackend::Espeak {
            voice: voice.clone(),
        }),
        PhonemizerConfig::Pinyin { model_dir } => Ok(PhonemizerBackend::Pinyin(Box::new(
            create_pinyin_backend(model_dir)?,
        ))),
    }
}

pub(crate) fn phone_tone_pairs(
    backend: &PhonemizerBackend,
    text: &str,
) -> DengjenResult<Vec<Vec<(String, String)>>> {
    match backend {
        PhonemizerBackend::Espeak { voice } => espeak_phone_tone_pairs(text, voice),
        PhonemizerBackend::Pinyin(engine) => pinyin_phone_tone_pairs(engine, text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_backend_and_dispatch_route_espeak_config_to_the_espeak_path() {
        let config = crate::config::PhonemizerConfig::Espeak {
            voice: "en-us".to_string(),
        };
        let backend = create_backend(&config).unwrap();
        assert!(matches!(backend, PhonemizerBackend::Espeak { .. }));
    }

    #[cfg(feature = "pinyin")]
    #[test]
    fn pinyin_phone_tone_pairs_emits_a_separate_pair_per_initial_and_finale() {
        let Ok(model_dir) = std::env::var("DENGJEN_PINYIN_TEST_MODEL_DIR") else {
            eprintln!("skipping: DENGJEN_PINYIN_TEST_MODEL_DIR not set");
            return;
        };
        let engine = create_pinyin_backend(std::path::Path::new(&model_dir)).unwrap();
        let result = pinyin_phone_tone_pairs(&engine, "你好").unwrap();
        assert_eq!(
            result.len(),
            1,
            "expected one sentence for a single short phrase"
        );
        assert!(
            result[0].iter().all(|(_, tone)| tone != "_" || result[0].len() == 1),
            "every real syllable's pairs should carry a real tone digit, not the passthrough sentinel: {:?}",
            result[0]
        );
    }

    #[cfg(not(feature = "pinyin"))]
    #[test]
    fn pinyin_phone_tone_pairs_errors_cleanly_when_pinyin_disabled() {
        let result = pinyin_phone_tone_pairs(&(), "你好");
        assert!(matches!(result, Err(DengjenError::PhonemizationError(_))));
    }

    #[cfg(feature = "espeak")]
    static ESPEAK_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[cfg(feature = "espeak")]
    fn lock_espeak() -> std::sync::MutexGuard<'static, ()> {
        ESPEAK_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[cfg(feature = "espeak")]
    fn phonemize_or_skip(text: &str, voice: &str) -> Option<Vec<Vec<(String, String)>>> {
        match espeak_phone_tone_pairs(text, voice) {
            Ok(pairs) => Some(pairs),
            Err(DengjenError::PhonemizationError(msg))
                if msg.contains(dengjen_espeak_phonemizer::ESPEAKNG_INIT_FAILURE_MARKER) =>
            {
                eprintln!(
                    "Skipping: no espeak-ng data available. Set DENGJEN_ESPEAKNG_DATA_DIRECTORY."
                );
                None
            }
            Err(e) => panic!("phonemization failed unexpectedly: {e}"),
        }
    }

    #[cfg(feature = "espeak")]
    #[test]
    fn espeak_phone_tone_pairs_returns_one_sentence_with_the_no_tone_sentinel() {
        let _guard = lock_espeak();
        let Some(result) = phonemize_or_skip("Hello there.", "en-US") else {
            return;
        };
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].len(), 1);
        assert_eq!(result[0][0].1, "_");
        assert!(!result[0][0].0.is_empty());
    }

    #[cfg(feature = "espeak")]
    #[test]
    fn espeak_phone_tone_pairs_errors_for_an_unrecognized_voice() {
        let _guard = lock_espeak();
        let result = espeak_phone_tone_pairs("hello", "not-a-real-language-code");
        assert!(matches!(result, Err(DengjenError::PhonemizationError(_))));
    }

    #[cfg(not(feature = "espeak"))]
    #[test]
    fn espeak_phone_tone_pairs_errors_cleanly_when_espeak_disabled() {
        let result = espeak_phone_tone_pairs("hello", "en-US");
        assert!(matches!(result, Err(DengjenError::PhonemizationError(_))));
    }
}
