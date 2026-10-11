#![forbid(unsafe_code)]

mod chars;
mod ipa;
mod nakdimon;

pub use nakdimon::{create_nakdimon_engine, NakdimonEngine};

pub use nakdimon::num_classes;

use dengjen_tts_core::{DengjenResult, Phonemes};

fn already_diacritized(text: &str) -> bool {
    text.chars().any(chars::is_niqqud)
}

fn phonemes_from_diacritized(diacritized: &str) -> Phonemes {
    let ipa = ipa::hebrew_to_ipa(diacritized);
    if ipa.is_empty() {
        Phonemes::from(Vec::<String>::new())
    } else {
        Phonemes::from(vec![ipa])
    }
}

pub fn text_to_hebrew_phonemes(engine: &NakdimonEngine, text: &str) -> DengjenResult<Phonemes> {
    let diacritized = if already_diacritized(text) {
        text.to_string()
    } else {
        engine.diacritize(text)?
    };
    Ok(phonemes_from_diacritized(&diacritized))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn already_diacritized_is_true_for_text_carrying_niqqud() {
        let pointed = "\u{05E9}\u{05B8}\u{05DC}\u{05D5}\u{05DD}";
        assert!(already_diacritized(pointed));
    }

    #[test]
    fn already_diacritized_is_false_for_undotted_consonants_only() {
        let undotted = "\u{05E9}\u{05DC}\u{05D5}\u{05DD}";
        assert!(!already_diacritized(undotted));
    }

    #[test]
    fn already_diacritized_is_false_for_undotted_text_with_maqaf_or_sof_pasuq() {
        assert!(!already_diacritized(
            "\u{05E2}\u{05DC}\u{05BE}\u{05D9}\u{05D3}\u{05D9}"
        ));
        assert!(!already_diacritized(
            "\u{05E9}\u{05DC}\u{05D5}\u{05DD}\u{05C3}"
        ));
    }

    #[test]
    fn phonemes_from_diacritized_produces_one_sentence_for_pointed_text() {
        let pointed = "\u{05E9}\u{05B8}\u{05DC}\u{05D5}\u{05DD}";
        let phonemes = phonemes_from_diacritized(pointed);
        assert_eq!(phonemes.num_sentences(), 1);
        assert!(!phonemes.sentences()[0].is_empty());
    }

    #[test]
    fn phonemes_from_diacritized_on_empty_input_yields_empty_phonemes() {
        let phonemes = phonemes_from_diacritized("");
        assert_eq!(phonemes.num_sentences(), 0);
    }

    #[test]
    fn text_to_hebrew_phonemes_diacritizes_undotted_text_with_a_real_model() {
        let Ok(model_path) = std::env::var("DENGJEN_NAKDIMON_TEST_MODEL_PATH") else {
            eprintln!("skipping: DENGJEN_NAKDIMON_TEST_MODEL_PATH not set");
            return;
        };
        let engine = create_nakdimon_engine(std::path::Path::new(&model_path)).unwrap();
        let phonemes =
            text_to_hebrew_phonemes(&engine, "\u{05E9}\u{05DC}\u{05D5}\u{05DD}").unwrap();
        assert_eq!(phonemes.num_sentences(), 1);
        assert!(!phonemes.sentences()[0].is_empty());
    }
}
