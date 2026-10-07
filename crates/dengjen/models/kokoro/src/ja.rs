use crate::ja_map::{fit_to_vocab, is_tail, moras_to_phonemes, pron_to_moras};
use dengjen_tts_core::{DengjenError, DengjenResult};
use jpreprocess::{JPreprocess, SystemDictionaryConfig};
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};

pub(crate) const JAPANESE_DICT_ENV: &str = "DENGJEN_JA_DICT_DIR";

const PUNCT_STARTS: &str = "(“";
const PUNCT_STOPS: &str = "!),.:;?”";
const PUNCT_VALUES: &str = "!\"(),.:;?—“”…";

pub(crate) struct Word {
    pub surface: String,
    pub pron: String,
    pub mora_size: usize,
    pub chain_flag: bool,
}

struct Token {
    phonemes: Option<String>,
    whitespace: String,
    voiced: bool,
    chained: bool,
    starts_with_n: bool,
}

fn map_punct(surface: &str) -> String {
    surface
        .chars()
        .map(|c| match c {
            '«' | '〈' | '《' | '「' | '『' | '【' => '“',
            '»' | '〉' | '》' | '」' | '』' | '】' => '”',
            '、' | '，' => ',',
            '。' | '．' => '.',
            '！' => '!',
            '（' => '(',
            '）' => ')',
            '：' => ':',
            '；' => ';',
            '？' => '?',
            other => other,
        })
        .collect()
}

fn is_punct_only(surface: &str) -> bool {
    !surface.is_empty() && surface.chars().all(|c| PUNCT_VALUES.contains(c))
}

fn set_last_whitespace(tokens: &mut [Token], whitespace: &str) {
    if let Some(last) = tokens.last_mut() {
        last.whitespace = whitespace.to_string();
    }
}

fn push_punct(tokens: &mut Vec<Token>, surface: String) {
    let last_char = surface.chars().last().unwrap_or(' ');
    let whitespace = if PUNCT_STOPS.contains(last_char) {
        set_last_whitespace(tokens, "");
        " "
    } else {
        if PUNCT_STARTS.contains(last_char)
            && tokens.last().is_some_and(|t| t.whitespace.is_empty())
        {
            set_last_whitespace(tokens, " ");
        }
        ""
    };
    tokens.push(Token {
        phonemes: Some(surface),
        whitespace: whitespace.to_string(),
        voiced: false,
        chained: false,
        starts_with_n: false,
    });
}

fn push_word(tokens: &mut Vec<Token>, word: &Word) {
    let surface = map_punct(&word.surface);
    let moras = if word.mora_size > 0 {
        pron_to_moras(&word.pron)
    } else {
        Vec::new()
    };
    if moras.is_empty() && is_punct_only(&surface) {
        return push_punct(tokens, surface);
    }
    if surface.trim().is_empty() || (moras.is_empty() && surface == "・") {
        return set_last_whitespace(tokens, " ");
    }
    let chained = !moras.is_empty()
        && tokens.last().is_some_and(|t| t.voiced)
        && (word.chain_flag || moras.first().is_some_and(|m| m == "ー"));
    tokens.push(Token {
        phonemes: (!moras.is_empty()).then(|| moras_to_phonemes(&moras)),
        whitespace: String::new(),
        voiced: !moras.is_empty(),
        chained,
        starts_with_n: moras.first().is_some_and(|m| m == "ン"),
    });
}

fn needs_separator(token: &Token, out: &str) -> bool {
    token.voiced
        && !token.chained
        && !token.starts_with_n
        && out.chars().last().is_some_and(is_tail)
}

pub(crate) fn assemble(words: &[Word]) -> String {
    let tokens = words.iter().fold(Vec::new(), |mut tokens, word| {
        push_word(&mut tokens, word);
        tokens
    });
    let joined = tokens.iter().fold(String::new(), |mut out, token| {
        let Some(phonemes) = &token.phonemes else {
            return out;
        };
        if needs_separator(token, &out) {
            out.push(' ');
        }
        out.push_str(phonemes);
        out.push_str(&token.whitespace);
        out
    });
    joined.trim_end().to_string()
}

pub(crate) fn split_sentences(text: &str) -> Vec<&str> {
    text.split_inclusive(['。', '！', '？', '!', '?', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect()
}

pub(crate) struct JapaneseG2p {
    // Keeps KokoroModel UnwindSafe: lindera's boxed filters are not, but text_to_njd only reads the engine, so a panic leaves no torn state.
    engine: AssertUnwindSafe<JPreprocess<jpreprocess::DefaultTokenizer>>,
}

fn catch_phonemization<T>(f: impl FnOnce() -> DengjenResult<T>) -> DengjenResult<T> {
    std::panic::catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| {
        Err(DengjenError::PhonemizationError(
            "the Japanese phonemizer panicked on this text".to_string(),
        ))
    })
}

pub(crate) enum JapaneseDictionary {
    Loaded(Box<JapaneseG2p>),
    Unset,
}

impl JapaneseDictionary {
    pub(crate) fn from_env() -> DengjenResult<Self> {
        match std::env::var_os(JAPANESE_DICT_ENV) {
            None => Ok(Self::Unset),
            value => JapaneseG2p::from_env_value(value).map(|g2p| Self::Loaded(Box::new(g2p))),
        }
    }

    pub(crate) fn phonemize(&self, text: &str) -> DengjenResult<Vec<String>> {
        match self {
            Self::Loaded(g2p) => g2p.phonemize(text),
            Self::Unset => Err(DengjenError::PhonemizationError(unset_dictionary_message())),
        }
    }
}

fn unset_dictionary_message() -> String {
    format!(
        "Japanese presets need a jpreprocess dictionary: set {JAPANESE_DICT_ENV} to its directory"
    )
}

fn dictionary_error(path: &Path, cause: impl std::fmt::Display) -> DengjenError {
    DengjenError::FailedToLoadResource(format!(
        "Failed to load the Japanese dictionary at `{}` (set by {JAPANESE_DICT_ENV}): {cause}",
        path.display()
    ))
}

fn load_engine(dir: &Path) -> Result<JPreprocess<jpreprocess::DefaultTokenizer>, String> {
    let dictionary = SystemDictionaryConfig::File(dir.to_path_buf())
        .load()
        .map_err(|e| e.to_string())?;
    let engine = JPreprocess::with_dictionaries(dictionary, None);
    engine.text_to_njd("日本").map_err(|e| e.to_string())?;
    Ok(engine)
}

impl JapaneseG2p {
    pub(crate) fn from_env_value(value: Option<std::ffi::OsString>) -> DengjenResult<Self> {
        let dir = value
            .map(PathBuf::from)
            .ok_or_else(|| DengjenError::FailedToLoadResource(unset_dictionary_message()))?;
        let engine = std::panic::catch_unwind(|| load_engine(&dir))
            .unwrap_or_else(|_| {
                Err("the dictionary is corrupt or from another jpreprocess version".to_string())
            })
            .map_err(|cause| dictionary_error(&dir, cause))?;
        Ok(Self {
            engine: AssertUnwindSafe(engine),
        })
    }

    pub(crate) fn phonemize(&self, text: &str) -> DengjenResult<Vec<String>> {
        split_sentences(text)
            .into_iter()
            .map(|sentence| catch_phonemization(|| self.phonemize_sentence(sentence)))
            .filter(|result| result.as_ref().map_or(true, |p| !p.is_empty()))
            .collect()
    }

    fn phonemize_sentence(&self, sentence: &str) -> DengjenResult<String> {
        let mut njd = self
            .engine
            .text_to_njd(sentence)
            .map_err(|e| DengjenError::PhonemizationError(e.to_string()))?;
        njd.preprocess();
        let words: Vec<Word> = njd
            .nodes
            .iter()
            .map(|node| Word {
                surface: node.get_string().to_string(),
                pron: node.get_pron().to_pure_string(),
                mora_size: node.get_pron().mora_size(),
                chain_flag: node.get_chain_flag().unwrap_or(false),
            })
            .collect();
        Ok(fit_to_vocab(&assemble(&words)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(surface: &str, pron: &str, mora_size: usize, chain_flag: bool) -> Word {
        Word {
            surface: surface.into(),
            pron: pron.into(),
            mora_size,
            chain_flag,
        }
    }

    #[test]
    fn a_single_word_becomes_its_phonemes() {
        assert_eq!(
            assemble(&[word("こんにちは", "コンニチワ", 5, false)]),
            "koɴniʨiwa"
        );
    }

    #[test]
    fn unchained_words_are_separated_by_a_space() {
        let words = [word("私", "ワタシ", 3, false), word("猫", "ネコ", 2, false)];
        assert_eq!(assemble(&words), "wataɕi neko");
    }

    #[test]
    fn chained_words_are_joined_without_a_space() {
        let words = [
            word("食べ", "タベ", 2, false),
            word("ます", "マス", 2, true),
        ];
        assert_eq!(assemble(&words), "tabemasu");
    }

    #[test]
    fn a_word_starting_with_n_attaches_to_the_previous_word() {
        let words = [word("本", "ホン", 2, false), word("ん", "ン", 1, false)];
        assert_eq!(assemble(&words), "hoɴɴ");
    }

    #[test]
    fn punctuation_maps_and_a_stop_adds_trailing_then_trims_space() {
        let words = [word("はい", "ハイ", 2, false), word("。", "、", 0, false)];
        assert_eq!(assemble(&words), "hai.");
    }

    #[test]
    fn a_comma_is_followed_by_a_space() {
        let words = [
            word("はい", "ハイ", 2, false),
            word("、", "、", 0, false),
            word("はい", "ハイ", 2, false),
        ];
        assert_eq!(assemble(&words), "hai, hai");
    }

    #[test]
    fn full_width_comma_and_period_map_to_ascii() {
        let words = [
            word("はい", "ハイ", 2, false),
            word("，", "、", 0, false),
            word("はい", "ハイ", 2, false),
            word("．", "、", 0, false),
        ];
        assert_eq!(assemble(&words), "hai, hai.");
    }

    #[test]
    fn a_regular_file_as_dictionary_names_variable_and_path() {
        let path = std::env::temp_dir().join("dengjen_ja_dict_is_a_file");
        std::fs::write(&path, b"x").unwrap();
        let err = JapaneseG2p::from_env_value(Some(path.clone().into()))
            .err()
            .map(|e| e.to_string());
        std::fs::remove_file(&path).ok();
        assert!(err.is_some_and(|m| m.contains(JAPANESE_DICT_ENV)));
    }

    #[test]
    fn a_panic_while_phonemizing_becomes_a_phonemization_error() {
        let result: DengjenResult<()> = catch_phonemization(|| panic!("tokenizer bug"));
        assert!(matches!(result, Err(DengjenError::PhonemizationError(_))));
    }

    #[test]
    fn a_phonemization_error_passes_through_the_guard_unchanged() {
        let result: DengjenResult<()> =
            catch_phonemization(|| Err(DengjenError::PhonemizationError("bad".into())));
        assert!(matches!(result, Err(DengjenError::PhonemizationError(m)) if m == "bad"));
    }

    #[test]
    fn the_public_model_stays_unwind_safe() {
        fn assert_unwind_safe<T: std::panic::UnwindSafe + std::panic::RefUnwindSafe>() {}
        assert_unwind_safe::<crate::KokoroModel>();
    }

    #[test]
    #[cfg(unix)]
    fn a_corrupt_dictionary_fails_at_load_naming_variable_and_path() {
        let Some(real) = std::env::var_os(JAPANESE_DICT_ENV).map(PathBuf::from) else {
            return;
        };
        let broken = std::env::temp_dir().join("dengjen_ja_dict_corrupt");
        std::fs::remove_dir_all(&broken).ok();
        std::fs::create_dir_all(&broken).unwrap();
        for entry in std::fs::read_dir(&real).unwrap().flatten() {
            let name = entry.file_name();
            let target = broken.join(&name);
            if name == "dict.words" {
                std::fs::write(&target, b"garbage").unwrap();
            } else {
                std::os::unix::fs::symlink(entry.path(), &target).unwrap();
            }
        }
        let err = JapaneseG2p::from_env_value(Some(broken.clone().into()))
            .err()
            .map(|e| e.to_string());
        std::fs::remove_dir_all(&broken).ok();
        assert!(err.is_some_and(|m| m.contains(JAPANESE_DICT_ENV)));
    }

    #[test]
    fn an_empty_directory_as_dictionary_names_variable_and_path() {
        let path = std::env::temp_dir().join("dengjen_ja_dict_empty_dir");
        std::fs::create_dir_all(&path).unwrap();
        let err = JapaneseG2p::from_env_value(Some(path.clone().into()))
            .err()
            .map(|e| e.to_string());
        std::fs::remove_dir(&path).ok();
        assert!(err.is_some_and(|m| m.contains(JAPANESE_DICT_ENV)));
    }

    #[test]
    fn words_without_a_pronunciation_are_dropped() {
        let words = [word("ABC", "", 0, false), word("はい", "ハイ", 2, false)];
        assert_eq!(assemble(&words), "hai");
    }

    #[test]
    fn empty_input_assembles_to_an_empty_string() {
        assert_eq!(assemble(&[]), "");
    }

    #[test]
    fn punctuation_only_has_no_leading_space() {
        assert_eq!(assemble(&[word("。", "、", 0, false)]), ".");
    }

    #[test]
    fn a_long_vowel_mark_chains_to_the_previous_word() {
        let words = [word("あり", "アリ", 2, false), word("ー", "ー", 1, false)];
        assert_eq!(assemble(&words), "ariː");
    }

    #[test]
    fn sentences_split_after_terminators_and_newlines() {
        assert_eq!(
            split_sentences("はい。いいえ！\nそう？うん"),
            vec!["はい。", "いいえ！", "そう？", "うん"]
        );
    }

    #[test]
    fn blank_text_has_no_sentences() {
        assert!(split_sentences("  \n ").is_empty());
    }

    #[test]
    fn an_unset_dictionary_variable_names_the_variable() {
        let err = JapaneseG2p::from_env_value(None)
            .err()
            .map(|e| e.to_string());
        assert!(err.is_some_and(|m| m.contains(JAPANESE_DICT_ENV)));
    }

    #[test]
    fn a_missing_dictionary_directory_names_variable_and_path() {
        let value = Some("/nonexistent/ja-dict".into());
        let err = JapaneseG2p::from_env_value(value)
            .err()
            .map(|e| e.to_string());
        assert!(err
            .is_some_and(|m| m.contains(JAPANESE_DICT_ENV) && m.contains("/nonexistent/ja-dict")));
    }

    fn dictionary() -> Option<JapaneseG2p> {
        let value = std::env::var_os(JAPANESE_DICT_ENV)?;
        Some(
            JapaneseG2p::from_env_value(Some(value))
                .expect("DENGJEN_JA_DICT_DIR is set but does not load"),
        )
    }

    #[test]
    fn a_real_dictionary_phonemizes_a_greeting() {
        let Some(g2p) = dictionary() else { return };
        assert_eq!(g2p.phonemize("こんにちは。").unwrap(), vec!["koɴniʨiwa."]);
    }

    #[test]
    fn a_real_dictionary_returns_nothing_for_blank_text() {
        let Some(g2p) = dictionary() else { return };
        assert!(g2p.phonemize("   ").unwrap().is_empty());
    }

    #[test]
    fn a_real_dictionary_drops_latin_words_and_keeps_the_rest() {
        let Some(g2p) = dictionary() else { return };
        let phonemes = g2p.phonemize("ABCはい。").unwrap();
        assert!(phonemes
            .iter()
            .all(|p| p.chars().all(|c| !c.is_ascii_uppercase())));
        assert!(phonemes.concat().contains("hai"));
    }

    #[test]
    fn a_real_dictionary_matches_misaki_on_fixed_sentences() {
        let Some(g2p) = dictionary() else { return };
        let cases = [
            ("今日は日本に行きます。", "ᶄoːwa niʔpoɴni ikimasu."),
            ("日本一になる。", "niʔpoɴiʨini naru."),
            ("明日は雨です。", "aɕitawa amedesu."),
            ("ありがとうございます。", "arigatoː gozaimasu."),
            ("東京で5時に会います。", "toːᶄoːde goʥini aimasu."),
            (
                "2025年10月7日です。",
                "niseɴ niʥuː goneɴ ʥuːgaʦu nanokadesu.",
            ),
            ("コーヒーをください。", "koːhiːo kudasai."),
            ("ちょっと待って。", "ʨoʔto maʔte."),
            ("はい、わかりました！", "hai, wakarimaɕita!"),
        ];
        let mismatches: Vec<String> = cases
            .iter()
            .filter_map(|(text, expected)| {
                let actual = g2p.phonemize(text).unwrap().join(" ");
                let expected = fit_to_vocab(expected);
                (actual != expected)
                    .then(|| format!("{text}: expected `{expected}`, got `{actual}`"))
            })
            .collect();
        assert!(mismatches.is_empty(), "{mismatches:#?}");
    }
}
