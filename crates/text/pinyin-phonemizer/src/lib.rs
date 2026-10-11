#![forbid(unsafe_code)]

mod dictionary;
mod g2pw;
mod numbers;
mod syllable;
mod tokenize;

pub use numbers::normalize_numbers;

use dengjen_tts_core::{DengjenError, DengjenResult, Phonemes};
use dictionary::{load_dictionaries, resolve_char, Dictionaries};
use g2pw::{create_g2pw_engine, G2pwConfig, G2pwEngine};
use std::collections::HashMap;
use std::path::Path;

pub struct PinyinEngine {
    g2pw: G2pwEngine,
    dictionaries: Dictionaries,
    bopomofo_to_pinyin: HashMap<String, String>,
}

fn load_error(cause: impl std::fmt::Display) -> DengjenError {
    DengjenError::PhonemizationError(format!(
        "Failed to load pinyin phonemizer model files: {cause}"
    ))
}

pub fn create_pinyin_engine(model_dir: &Path) -> DengjenResult<PinyinEngine> {
    let dictionaries = load_dictionaries(
        &model_dir.join("MONOPHONIC_CHARS.txt"),
        &model_dir.join("POLYPHONIC_CHARS.txt"),
        &model_dir.join("char_bopomofo_dict.json"),
    )?;
    let (labels, char2phonemes) = dictionary::get_phoneme_labels(&dictionaries.polyphonic_chars);
    let mut chars: Vec<char> = char2phonemes.keys().copied().collect();
    chars.sort();

    let g2pw = create_g2pw_engine(
        &model_dir.join("g2pw.onnx"),
        &model_dir.join("tokenizer.json"),
        G2pwConfig {
            labels,
            char2phonemes,
            window_size: 32,
            max_len: 512,
            chars,
        },
    )?;

    let raw = std::fs::read_to_string(model_dir.join("bopomofo_to_pinyin_wo_tune_dict.json"))
        .map_err(load_error)?;
    let bopomofo_to_pinyin: HashMap<String, String> =
        serde_json::from_str(&raw).map_err(load_error)?;

    Ok(PinyinEngine {
        g2pw,
        dictionaries,
        bopomofo_to_pinyin,
    })
}

fn strip_quotation_marks(text: &str) -> String {
    text.chars()
        .filter(|c| !matches!(c, '\u{201C}' | '\u{201D}' | '"'))
        .collect()
}

fn split_into_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();
    for c in text.chars() {
        current.push(c);
        if matches!(c, '\u{3002}' | '！' | '？') {
            sentences.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        sentences.push(current);
    }
    sentences
}

#[non_exhaustive]
pub enum PinyinToken {
    Syllable {
        initial: String,
        finale: String,
        tone: char,
    },
    Passthrough(String),
}

fn flatten_token(token: &PinyinToken) -> Vec<String> {
    match token {
        PinyinToken::Syllable {
            initial,
            finale,
            tone,
        } => {
            let ini = if initial.is_empty() {
                "\u{00D8}"
            } else {
                initial
            };
            vec![ini.to_string(), finale.clone(), tone.to_string()]
        }
        PinyinToken::Passthrough(s) => vec![s.clone()],
    }
}

/// Punctuation carried through unchanged when the dictionary has no bopomofo for it.
fn is_passthrough_punctuation(c: char) -> bool {
    matches!(
        c,
        '\u{3002}'
            | '.'
            | '\u{FF1F}'
            | '?'
            | '\u{FF01}'
            | '!'
            | '\u{2014}'
            | '\u{2026}'
            | '\u{3001}'
            | '\u{FF0C}'
            | ','
            | '\u{FF1A}'
            | ':'
            | '\u{FF1B}'
            | ';'
            | ' '
    )
}

fn phonemize_sentence_tokens(
    engine: &PinyinEngine,
    sentence: &str,
) -> DengjenResult<Vec<PinyinToken>> {
    let chars: Vec<char> = sentence.chars().collect();
    let mut tokens = Vec::new();
    for (i, &c) in chars.iter().enumerate() {
        let bopomofo = resolve_char(&engine.dictionaries, &engine.g2pw, &chars, i)?;
        let Some(bopomofo) = bopomofo else {
            if is_passthrough_punctuation(c) {
                tokens.push(PinyinToken::Passthrough(c.to_string()));
            }
            continue;
        };
        let Some(pinyin) =
            syllable::convert_bopomofo_to_pinyin(&bopomofo, &engine.bopomofo_to_pinyin)
        else {
            tokens.push(PinyinToken::Passthrough(bopomofo));
            continue;
        };
        let Some((initial, finale, tone)) = syllable::split_initial_final_tone(&pinyin) else {
            tokens.push(PinyinToken::Passthrough(pinyin));
            continue;
        };
        tokens.push(PinyinToken::Syllable {
            initial,
            finale,
            tone,
        });
    }
    Ok(tokens)
}

pub fn text_to_pinyin_tokens(
    engine: &PinyinEngine,
    text: &str,
) -> DengjenResult<Vec<Vec<PinyinToken>>> {
    let stripped = strip_quotation_marks(text);
    let sentences: Vec<String> = split_into_sentences(&stripped)
        .into_iter()
        .map(|s| numbers::normalize_numbers(&s))
        .collect();

    sentences
        .iter()
        .map(|sentence| phonemize_sentence_tokens(engine, sentence))
        .collect()
}

pub fn text_to_pinyin_phonemes(engine: &PinyinEngine, text: &str) -> DengjenResult<Phonemes> {
    let token_sentences = text_to_pinyin_tokens(engine, text)?;
    Ok(Phonemes::from(
        token_sentences
            .iter()
            .map(|tokens| tokens.iter().flat_map(flatten_token).collect::<String>())
            .collect::<Vec<String>>(),
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    fn split_into_sentences_splits_on_chinese_terminal_punctuation_keeping_the_punctuation() {
        let sentences = super::split_into_sentences("你好。再见！");
        assert_eq!(sentences, vec!["你好。".to_string(), "再见！".to_string()]);
    }

    #[test]
    fn split_into_sentences_keeps_a_trailing_fragment_with_no_terminal_punctuation() {
        let sentences = super::split_into_sentences("你好。还有更多");
        assert_eq!(
            sentences,
            vec!["你好。".to_string(), "还有更多".to_string()]
        );
    }

    #[test]
    fn split_into_sentences_on_empty_input_yields_no_sentences() {
        assert_eq!(super::split_into_sentences(""), Vec::<String>::new());
    }

    #[test]
    fn strip_quotation_marks_removes_curly_and_straight_double_quotes() {
        assert_eq!(super::strip_quotation_marks("“你好”\"再见\""), "你好再见");
    }

    #[test]
    fn load_error_wraps_the_cause_with_context() {
        let err = super::load_error("boom");
        assert!(err.to_string().contains("pinyin phonemizer model files"));
        assert!(err.to_string().contains("boom"));
    }

    #[test]
    fn is_passthrough_punctuation_accepts_terminal_and_pause_punctuation() {
        for c in ['\u{3002}', '.', '?', '!', ',', ' ', '\u{FF1A}'] {
            assert!(
                super::is_passthrough_punctuation(c),
                "expected {c:?} to pass through"
            );
        }
    }

    #[test]
    fn is_passthrough_punctuation_rejects_ordinary_characters() {
        for c in ['a', '你', '3'] {
            assert!(
                !super::is_passthrough_punctuation(c),
                "did not expect {c:?} to pass through"
            );
        }
    }

    #[test]
    fn flatten_token_emits_zero_initial_marker_for_a_zero_initial_syllable() {
        let token = super::PinyinToken::Syllable {
            initial: String::new(),
            finale: "ai".to_string(),
            tone: '3',
        };
        assert_eq!(
            super::flatten_token(&token),
            vec!["\u{00D8}".to_string(), "ai".to_string(), "3".to_string()]
        );
    }

    #[test]
    fn flatten_token_emits_a_real_initial_when_present() {
        let token = super::PinyinToken::Syllable {
            initial: "zh".to_string(),
            finale: "ang".to_string(),
            tone: '2',
        };
        assert_eq!(
            super::flatten_token(&token),
            vec!["zh".to_string(), "ang".to_string(), "2".to_string()]
        );
    }

    #[test]
    fn flatten_token_passes_through_punctuation_unchanged() {
        let token = super::PinyinToken::Passthrough("\u{3002}".to_string());
        assert_eq!(super::flatten_token(&token), vec!["\u{3002}".to_string()]);
    }

    #[test]
    fn text_to_pinyin_phonemes_produces_output_for_a_simple_sentence_with_a_real_model() {
        let Ok(model_dir) = std::env::var("DENGJEN_PINYIN_TEST_MODEL_DIR") else {
            eprintln!("skipping: DENGJEN_PINYIN_TEST_MODEL_DIR not set");
            return;
        };
        let engine = super::create_pinyin_engine(std::path::Path::new(&model_dir)).unwrap();
        let phonemes = super::text_to_pinyin_phonemes(&engine, "你好").unwrap();
        assert_eq!(phonemes.num_sentences(), 1);
        assert!(!phonemes.sentences()[0].is_empty());
    }

    #[test]
    fn text_to_pinyin_tokens_flattens_to_the_same_string_as_text_to_pinyin_phonemes() {
        let Ok(model_dir) = std::env::var("DENGJEN_PINYIN_TEST_MODEL_DIR") else {
            eprintln!("skipping: DENGJEN_PINYIN_TEST_MODEL_DIR not set");
            return;
        };
        let engine = super::create_pinyin_engine(std::path::Path::new(&model_dir)).unwrap();
        let via_tokens = super::text_to_pinyin_tokens(&engine, "你好").unwrap();
        let flattened: Vec<String> = via_tokens
            .iter()
            .map(|tokens| {
                tokens
                    .iter()
                    .flat_map(super::flatten_token)
                    .collect::<String>()
            })
            .collect();
        let via_phonemes = super::text_to_pinyin_phonemes(&engine, "你好").unwrap();
        assert_eq!(flattened, *via_phonemes.sentences());
    }
}
