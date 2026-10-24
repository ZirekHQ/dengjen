#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)
)]

mod espeakng;

use ffi_support::FfiStr;
use once_cell::sync::Lazy;
use std::env;
use std::error::Error;
use std::ffi;
use std::ffi::CString;
use std::fmt;
use std::path::PathBuf;
use std::sync::Mutex;

pub type ESpeakResult<T> = Result<T, ESpeakError>;

const CLAUSE_INTONATION_FULL_STOP: i32 = 0x00000000;
const CLAUSE_INTONATION_COMMA: i32 = 0x00001000;
const CLAUSE_INTONATION_QUESTION: i32 = 0x00002000;
const CLAUSE_INTONATION_EXCLAMATION: i32 = 0x00003000;
const CLAUSE_TYPE_SENTENCE: i32 = 0x00080000;

const DENGJEN_ESPEAKNG_DATA_DIRECTORY: &str = "DENGJEN_ESPEAKNG_DATA_DIRECTORY";

/// Prefix of the error message [`init_espeakng`] produces when eSpeak-ng's own
/// initialization call fails. Callers that need to detect "eSpeak-ng data is
/// unavailable" (e.g. to skip a test) should match on this constant instead of
/// a copy-pasted string literal, so a wording change here keeps every caller's
/// match in sync automatically instead of silently desynchronizing it.
pub const ESPEAKNG_INIT_FAILURE_MARKER: &str = "Failed to initialize eSpeak-ng";

#[derive(Debug, Clone)]
pub struct ESpeakError(pub String);

impl Error for ESpeakError {}

impl fmt::Display for ESpeakError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "eSpeak-ng Error :{}", self.0)
    }
}

static ESPEAKNG_INIT: Lazy<ESpeakResult<()>> = Lazy::new(init_espeakng);

static ESPEAK_LOCK: Mutex<()> = Mutex::new(());

static ESPEAKNG_DATA_PATH: Mutex<Option<CString>> = Mutex::new(None);

type ESpeakLock<'a> = std::sync::MutexGuard<'a, ()>;

fn resolve_data_directory() -> Option<CString> {
    let base = match env::var(DENGJEN_ESPEAKNG_DATA_DIRECTORY) {
        Ok(configured) => PathBuf::from(configured),
        Err(_) => env::current_exe().ok()?.parent()?.to_path_buf(),
    };
    if !base.join("espeak-ng-data").exists() {
        return None;
    }
    CString::new(base.display().to_string()).ok()
}

fn init_espeakng() -> ESpeakResult<()> {
    let mut data_path = ESPEAKNG_DATA_PATH
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *data_path = resolve_data_directory();
    let data_path_ptr = data_path
        .as_ref()
        .map_or(std::ptr::null(), |path| path.as_ptr());

    // SAFETY: `data_path_ptr` is either null or the NUL-terminated buffer of the `CString` that
    // `ESPEAKNG_DATA_PATH` now owns. eSpeak-ng keeps reading through that pointer after this
    // call returns, and this function runs exactly once — the `ESPEAKNG_INIT` `Lazy` is its
    // only caller — so that `CString` is never replaced or dropped and its buffer stays valid
    // for the rest of the process. Being the `Lazy`'s initializer also means no other thread is
    // inside eSpeak-ng's global setup at the same time.
    let sample_rate = unsafe {
        espeakng::espeak_Initialize(
            espeakng::espeak_AUDIO_OUTPUT_AUDIO_OUTPUT_RETRIEVAL,
            0,
            data_path_ptr,
            espeakng::espeakINITIALIZE_DONT_EXIT as ffi::c_int,
        )
    };

    if sample_rate > 0 {
        return Ok(());
    }

    Err(ESpeakError(format!(
        "{ESPEAKNG_INIT_FAILURE_MARKER}, error code `{sample_rate}`. If its data files are \
         installed somewhere non-standard, point `{DENGJEN_ESPEAKNG_DATA_DIRECTORY}` at the \
         directory that holds `espeak-ng-data`."
    )))
}

fn clause_break_suffix(terminator: ffi::c_int) -> &'static str {
    match terminator & 0x0000_F000 {
        CLAUSE_INTONATION_FULL_STOP => ".",
        CLAUSE_INTONATION_COMMA => ",",
        CLAUSE_INTONATION_QUESTION => "?",
        CLAUSE_INTONATION_EXCLAMATION => "!",
        _ => "",
    }
}

pub fn text_to_phonemes(
    text: &str,
    language: &str,
    phoneme_separator: Option<char>,
    remove_lang_switch_flags: bool,
    remove_stress: bool,
) -> ESpeakResult<Vec<String>> {
    let per_line: Vec<Vec<String>> = text
        .lines()
        .map(|line| {
            phonemize_line(
                line,
                language,
                phoneme_separator,
                remove_lang_switch_flags,
                remove_stress,
            )
        })
        .collect::<ESpeakResult<_>>()?;
    Ok(per_line.concat())
}

fn phonemize_line(
    text: &str,
    language: &str,
    phoneme_separator: Option<char>,
    remove_lang_switch_flags: bool,
    remove_stress: bool,
) -> ESpeakResult<Vec<String>> {
    let espeak = ESPEAK_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    Lazy::force(&ESPEAKNG_INIT).clone()?;
    select_voice(&espeak, language)?;
    let line = CString::new(text).map_err(|_| {
        ESpeakError("Input holds a NUL byte, which eSpeak-ng cannot read".to_string())
    })?;
    let sentences = read_clauses(&espeak, &line, phoneme_mode(phoneme_separator));
    let sentences = strip_matches(remove_lang_switch_flags, sentences, strip_language_switches);
    Ok(strip_matches(
        remove_stress,
        sentences,
        strip_stress_markers,
    ))
}

fn select_voice(_espeak: &ESpeakLock<'_>, language: &str) -> ESpeakResult<()> {
    let rejected = || ESpeakError(format!("eSpeak-ng has no voice named `{language}`"));
    let voice = CString::new(language).map_err(|_| rejected())?;

    // SAFETY: the pointer addresses `voice`'s NUL-terminated buffer, and `voice` is still alive
    // when the call returns; eSpeak-ng copies the name it keeps rather than retaining this
    // pointer. `_espeak` witnesses that the caller holds `ESPEAK_LOCK`, and that caller forced
    // `ESPEAKNG_INIT` first, so eSpeak-ng is initialized and no other thread is inside it.
    let status = unsafe { espeakng::espeak_SetVoiceByName(voice.as_ptr()) };
    if status == espeakng::espeak_ERROR_EE_OK {
        Ok(())
    } else {
        Err(rejected())
    }
}

fn phoneme_mode(phoneme_separator: Option<char>) -> ffi::c_int {
    let separator_bits = phoneme_separator.map_or(0, |separator| (separator as u32) << 8);
    (espeakng::espeakINITIALIZE_PHONEME_IPA | separator_bits) as ffi::c_int
}

fn read_clauses(
    _espeak: &ESpeakLock<'_>,
    line: &ffi::CStr,
    phoneme_mode: ffi::c_int,
) -> Vec<String> {
    let mut cursor: *const ffi::c_char = line.as_ptr();
    let mut terminator: ffi::c_int = 0;
    let mut sentences = Vec::new();
    let mut pending = String::new();
    while !cursor.is_null() {
        // SAFETY: `cursor` starts at `line`'s NUL-terminated buffer, which is borrowed for the
        // whole of this function and so outlives every iteration; eSpeak-ng writes back either
        // a position inside that same buffer or null once it has consumed all of it, and null
        // is what ends this loop, so every iteration reads within `line`. `&mut terminator`
        // borrows a live local. The returned pointer is eSpeak-ng's own phoneme buffer, valid
        // until the next call into it: `_espeak` witnesses that the caller holds `ESPEAK_LOCK`
        // (so no other thread can make that call) and that it forced `ESPEAKNG_INIT` first,
        // and this thread copies the bytes out before its own next iteration.
        let clause = unsafe {
            let phonemes = espeakng::espeak_TextToPhonemesWithTerminator(
                &mut cursor,
                espeakng::espeakCHARS_UTF8 as ffi::c_int,
                phoneme_mode,
                &mut terminator,
            );
            FfiStr::from_raw(phonemes)
        };
        pending.push_str(&clause.into_string());
        pending.push_str(clause_break_suffix(terminator));
        if terminator & CLAUSE_TYPE_SENTENCE == CLAUSE_TYPE_SENTENCE {
            sentences.push(std::mem::take(&mut pending));
        }
    }
    if !pending.is_empty() {
        sentences.push(pending);
    }
    sentences
}

fn strip_matches(enabled: bool, sentences: Vec<String>, strip: fn(&str) -> String) -> Vec<String> {
    if !enabled {
        return sentences;
    }
    sentences.iter().map(|sentence| strip(sentence)).collect()
}

// Removes the parenthesized language-switch annotations eSpeak-ng inserts into phoneme output
// when it detects a mid-utterance language change, e.g. `(en)`. An unclosed `(` stays.
fn strip_language_switches(text: &str) -> String {
    let mut stripped = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('(') {
        let (head, from_open) = rest.split_at(open);
        stripped.push_str(head);
        match from_open.find(')') {
            Some(close) => rest = from_open.split_at(close + 1).1,
            None => {
                stripped.push_str(from_open);
                return stripped;
            }
        }
    }
    stripped.push_str(rest);
    stripped
}

fn strip_stress_markers(text: &str) -> String {
    text.replace(['ˈ', 'ˌ'], "")
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT_ALICE: &str =
        "Who are you? said the Caterpillar. Replied Alice , rather shyly, I hardly know, sir!";

    #[test]
    fn basic_english_text_produces_expected_phonemes() -> ESpeakResult<()> {
        let text = "test";
        let expected = "tˈɛst.";
        let phonemes = text_to_phonemes(text, "en-US", None, false, false)?.join("");
        assert_eq!(phonemes, expected);
        Ok(())
    }

    #[test]
    #[ignore = "needs the data built from source: scripts/stage-espeak-data.sh, then --include-ignored"]
    fn kurmanji_words_carry_the_stress_fixed_in_espeak_ng_2500() -> ESpeakResult<()> {
        let cases = [
            ("mafan", "mafˈan."),
            ("malên", "mˈalen."),
            ("kirin", "kɪɾˈɪn."),
            ("hatin", "hatˈɪn."),
        ];
        for (word, expected) in cases {
            let phonemes = text_to_phonemes(word, "ku", None, false, false)?.join("");
            assert_eq!(phonemes, expected, "{word}");
        }
        Ok(())
    }

    #[test]
    fn splits_multiple_sentences_into_separate_phoneme_entries() -> ESpeakResult<()> {
        let phonemes = text_to_phonemes(TEXT_ALICE, "en-US", None, false, false)?;
        assert_eq!(phonemes.len(), 3);
        Ok(())
    }

    #[test]
    fn phoneme_separator_is_inserted_between_phonemes() -> ESpeakResult<()> {
        let text = "test";
        let expected = "t_ˈɛ_s_t.";
        let phonemes = text_to_phonemes(text, "en-US", Some('_'), false, false)
            .unwrap()
            .join("");
        assert_eq!(phonemes, expected);
        Ok(())
    }

    #[test]
    fn clause_breaker_punctuation_is_preserved_in_output() -> ESpeakResult<()> {
        let phonemes = text_to_phonemes(TEXT_ALICE, "en-US", None, false, false)?.join("");
        let clause_breakers = ['.', ',', '?', '!'];
        for c in clause_breakers {
            assert!(phonemes.contains(c), "Clause breaker `{}` not preserved", c);
        }
        Ok(())
    }

    #[test]
    fn arabic_text_produces_expected_phonemes() -> ESpeakResult<()> {
        let text = "مَرْحَبَاً بِكَ أَيُّهَا الْرَّجُلْ";
        let expected = "mˈarħabˌaː bikˌa ʔaˈiːuhˌaː alrrˈadʒul.";
        let phonemes = text_to_phonemes(text, "ar", None, false, false)?.join("");
        assert_eq!(phonemes, expected);
        Ok(())
    }

    #[test]
    fn strip_language_switches_removes_each_parenthesized_marker() {
        assert_eq!(strip_language_switches("(en)hə(ar)lo"), "həlo");
        assert_eq!(strip_language_switches("a(b)c(d)"), "ac");
    }

    #[test]
    fn strip_language_switches_keeps_an_unclosed_parenthesis() {
        assert_eq!(strip_language_switches("a(b)c(d"), "ac(d");
        assert_eq!(strip_language_switches("no markers"), "no markers");
    }

    #[test]
    fn strip_stress_markers_removes_primary_and_secondary_stress() {
        assert_eq!(strip_stress_markers("ˈhɛˌloʊ"), "hɛloʊ");
    }

    #[test]
    fn remove_lang_switch_flags_strips_language_switch_markers() -> ESpeakResult<()> {
        let text = "Hello معناها مرحباً";

        let with_lang_switch = text_to_phonemes(text, "ar", None, false, false)?.join("");
        assert!(with_lang_switch.contains("(en)"));
        assert!(with_lang_switch.contains("(ar)"));

        let without_lang_switch = text_to_phonemes(text, "ar", None, true, false)?.join("");
        assert!(!without_lang_switch.contains("(en)"));
        assert!(!without_lang_switch.contains("(ar)"));

        Ok(())
    }

    #[test]
    fn remove_stress_strips_stress_markers() -> ESpeakResult<()> {
        let stress_markers = ['ˈ', 'ˌ'];

        let with_stress = text_to_phonemes(TEXT_ALICE, "en-US", None, false, false)?.join("");
        assert!(with_stress.contains(stress_markers));

        let without_stress = text_to_phonemes(TEXT_ALICE, "en-US", None, false, true)?.join("");
        assert!(!without_stress.contains(stress_markers));

        Ok(())
    }
    #[test]
    fn each_input_line_produces_a_separate_phoneme_paragraph() -> ESpeakResult<()> {
        let text = "Hello\nThere\nAnd\nWelcome";
        let phoneme_paragraphs = text_to_phonemes(text, "en-US", None, false, false)?;
        assert_eq!(phoneme_paragraphs.len(), 4);
        Ok(())
    }

    #[test]
    fn empty_input_returns_no_phonemes() -> ESpeakResult<()> {
        let phonemes = text_to_phonemes("", "en-US", None, false, false)?;
        assert_eq!(phonemes, Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn interior_nul_byte_returns_err_instead_of_panicking() {
        let result = text_to_phonemes("hello\0world", "en-US", None, false, false);
        assert!(result.is_err());

        let result = text_to_phonemes("hello", "en\0US", None, false, false);
        assert!(result.is_err());
    }

    #[test]
    fn concurrent_calls_do_not_corrupt_each_others_output() {
        use std::thread;

        let en_text = "test";
        let en_expected = "tˈɛst.";
        let ar_text = "مَرْحَبَاً بِكَ أَيُّهَا الْرَّجُلْ";
        let ar_expected = "mˈarħabˌaː bikˌa ʔaˈiːuhˌaː alrrˈadʒul.";

        let handles: Vec<_> = (0..8)
            .map(|i| {
                thread::spawn(move || {
                    for _ in 0..25 {
                        if i % 2 == 0 {
                            let result = text_to_phonemes(en_text, "en-US", None, false, false)
                                .unwrap()
                                .join("");
                            assert_eq!(result, en_expected);
                        } else {
                            let result = text_to_phonemes(ar_text, "ar", None, false, false)
                                .unwrap()
                                .join("");
                            assert_eq!(result, ar_expected);
                        }
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().expect("worker thread panicked");
        }
    }

    #[test]
    fn espeak_error_display_includes_the_message() {
        let err = ESpeakError("boom".to_string());
        assert_eq!(err.to_string(), "eSpeak-ng Error :boom");
    }
}
