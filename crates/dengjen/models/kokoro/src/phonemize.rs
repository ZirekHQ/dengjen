use dengjen_tts_core::{DengjenError, DengjenResult};

#[cfg_attr(not(feature = "espeak"), allow(dead_code))]
const SUBSTITUTIONS: &[(&str, &str)] = &[
    ("aɪ", "I"),
    ("aʊ", "W"),
    ("dʒ", "ʤ"),
    ("eɪ", "A"),
    ("tʃ", "ʧ"),
    ("ɔɪ", "Y"),
    ("oʊ", "O"),
    ("ɚ", "əɹ"),
    ("r", "ɹ"),
    ("x", "k"),
    ("ç", "k"),
    ("ɐ", "ə"),
    ("ɬ", "l"),
    ("ʔ", "t"),
    ("n\u{0329}", "ᵊn"),
    ("ʲ", ""),
    ("ː", ""),
];

#[cfg_attr(not(feature = "espeak"), allow(dead_code))]
fn espeak_ipa_to_kokoro(ipa: &str) -> String {
    let mut result = ipa.to_string();
    for (from, to) in SUBSTITUTIONS {
        result = result.replace(from, to);
    }
    result
}

const PRESET_LANGUAGES: &[(char, &str)] = &[
    ('a', "en-US"),
    ('b', "en-GB-x-rp"),
    ('j', "ja"),
    ('z', "cmn"),
    ('e', "es"),
    ('f', "fr"),
    ('h', "hi"),
    ('i', "it"),
    ('p', "pt-BR"),
];

/// Returns the eSpeak language for a Kokoro preset name, or `PhonemizationError` if the name is
/// shorter than two characters or its first letter is unmapped.
pub fn espeak_language_for_voice(voice: &str) -> DengjenResult<&'static str> {
    let mut letters = voice.chars();
    letters
        .next()
        .filter(|_| letters.next().is_some())
        .and_then(|prefix| PRESET_LANGUAGES.iter().find(|(p, _)| *p == prefix))
        .map(|(_, language)| *language)
        .ok_or_else(|| {
            DengjenError::PhonemizationError(format!(
                "Kokoro preset `{voice}` has no known language prefix"
            ))
        })
}

#[cfg(feature = "espeak")]
pub fn text_to_kokoro_phonemes(text: &str, language: &str) -> DengjenResult<Vec<String>> {
    let sentences = dengjen_espeak_phonemizer::text_to_phonemes(text, language, None, true, false)
        .map_err(|e| DengjenError::PhonemizationError(e.to_string()))?;
    Ok(sentences.iter().map(|s| espeak_ipa_to_kokoro(s)).collect())
}

#[cfg(not(feature = "espeak"))]
pub fn text_to_kokoro_phonemes(_text: &str, _language: &str) -> DengjenResult<Vec<String>> {
    Err(DengjenError::PhonemizationError(
        "Kokoro phonemization requires the `espeak` feature (GPL-3.0-or-later, via espeak-ng), but it is disabled".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn espeak_ipa_to_kokoro_composes_ai_diphthong() {
        assert_eq!(espeak_ipa_to_kokoro("tˈaɪm"), "tˈIm");
    }

    #[test]
    fn espeak_ipa_to_kokoro_composes_dz_affricate() {
        assert_eq!(espeak_ipa_to_kokoro("dʒˈɑːb"), "ʤˈɑb");
    }

    #[test]
    fn espeak_ipa_to_kokoro_composes_oi_diphthong() {
        assert_eq!(espeak_ipa_to_kokoro("tˈɔɪ"), "tˈY");
    }

    #[test]
    fn espeak_ipa_to_kokoro_composes_au_diphthong() {
        assert_eq!(espeak_ipa_to_kokoro("hˈaʊs"), "hˈWs");
    }

    #[test]
    fn espeak_ipa_to_kokoro_leaves_plain_phonemes_unchanged() {
        assert_eq!(espeak_ipa_to_kokoro("tˈɛst"), "tˈɛst");
    }

    #[cfg(feature = "espeak")]
    static ESPEAK_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[cfg(feature = "espeak")]
    fn lock_espeak() -> std::sync::MutexGuard<'static, ()> {
        ESPEAK_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// `None` means espeak-ng has no data directory here, so the caller must skip
    /// rather than fail - any other error is a real failure and panics.
    #[cfg(feature = "espeak")]
    fn phonemize_or_skip(text: &str, language: &str) -> Option<Vec<String>> {
        match text_to_kokoro_phonemes(text, language) {
            Ok(sentences) => Some(sentences),
            Err(DengjenError::PhonemizationError(msg))
                if msg.contains(dengjen_espeak_phonemizer::ESPEAKNG_INIT_FAILURE_MARKER) =>
            {
                eprintln!(
                    "Skipping: no espeak-ng data available. Set DENGJEN_ESPEAKNG_DATA_DIRECTORY to the directory containing `espeak-ng-data`."
                );
                None
            }
            Err(e) => panic!("phonemization failed unexpectedly: {e}"),
        }
    }

    #[cfg(feature = "espeak")]
    #[test]
    fn text_to_kokoro_phonemes_returns_error_for_unset_voice() {
        let _guard = lock_espeak();
        let result = text_to_kokoro_phonemes("hello", "not-a-real-language-code");
        assert!(matches!(result, Err(DengjenError::PhonemizationError(_))));
    }

    #[cfg(feature = "espeak")]
    #[test]
    fn text_to_kokoro_phonemes_returns_one_entry_per_sentence() {
        let _guard = lock_espeak();
        let Some(result) = phonemize_or_skip("Hello there. General Kenobi.", "en-US") else {
            return;
        };
        assert_eq!(
            result.len(),
            2,
            "expected one entry per sentence, got {result:?}"
        );
        assert!(result.iter().all(|s| !s.trim().is_empty()));
    }

    #[cfg(feature = "espeak")]
    #[test]
    fn text_to_kokoro_phonemes_strips_language_switch_flags() {
        let _guard = lock_espeak();
        // Same mixed-script input as dengjen-espeak-phonemizer's own
        // remove_lang_switch_flags_strips_language_switch_markers.
        let Some(result) = phonemize_or_skip("Hello معناها مرحباً", "ar") else {
            return;
        };
        let joined = result.join("");
        assert!(
            !joined.contains('('),
            "unstripped lang-switch flag in {joined:?}"
        );
    }

    #[test]
    fn espeak_language_for_voice_maps_each_preset_prefix() {
        let cases = [
            ("af_heart", "en-US"),
            ("bf_emma", "en-GB-x-rp"),
            ("jf_alpha", "ja"),
            ("zf_xiaobei", "cmn"),
            ("ef_dora", "es"),
            ("ff_siwis", "fr"),
            ("hf_alpha", "hi"),
            ("if_sara", "it"),
            ("pf_dora", "pt-BR"),
            ("af", "en-US"),
        ];
        for (voice, language) in cases {
            assert_eq!(
                espeak_language_for_voice(voice).unwrap(),
                language,
                "{voice}"
            );
        }
    }

    #[test]
    fn espeak_language_for_voice_rejects_an_unknown_prefix() {
        for voice in ["test_voice", "xf_nobody", "", "a"] {
            let result = espeak_language_for_voice(voice);
            assert!(
                matches!(&result, Err(DengjenError::PhonemizationError(msg)) if msg.contains(voice)),
                "{voice:?} -> {result:?}"
            );
        }
    }

    #[cfg(feature = "espeak")]
    #[test]
    fn every_mapped_espeak_language_loads_in_espeak() {
        let _guard = lock_espeak();
        for voice in [
            "af_x", "bf_x", "jf_x", "zf_x", "ef_x", "ff_x", "hf_x", "if_x", "pf_x",
        ] {
            let language = espeak_language_for_voice(voice).unwrap();
            if phonemize_or_skip("hello", language).is_none() {
                return;
            }
        }
    }

    #[cfg(feature = "espeak")]
    #[test]
    fn japanese_text_is_not_read_out_as_character_descriptions() {
        let _guard = lock_espeak();
        let language = espeak_language_for_voice("jf_alpha").unwrap();
        let Some(result) = phonemize_or_skip("こんにちは", language) else {
            return;
        };
        let english = phonemize_or_skip("こんにちは", "en-US").unwrap();
        assert_ne!(
            result, english,
            "ja phonemes match the English readout: {result:?}"
        );
    }

    #[cfg(not(feature = "espeak"))]
    #[test]
    fn text_to_kokoro_phonemes_returns_error_when_espeak_disabled() {
        let result = text_to_kokoro_phonemes("hello", "en-US");
        assert!(matches!(result, Err(DengjenError::PhonemizationError(_))));
    }

    #[test]
    fn espeak_ipa_to_kokoro_composes_syllabic_consonant() {
        assert_eq!(espeak_ipa_to_kokoro("bˈʌʔn\u{0329}."), "bˈʌtᵊn.");
    }
}
