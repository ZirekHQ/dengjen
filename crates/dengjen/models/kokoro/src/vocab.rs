use dengjen_tts_core::{DengjenError, DengjenResult};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

pub struct Vocab {
    map: HashMap<String, i64>,
    bos_id: i64,
    max_token_chars: usize,
}

impl Vocab {
    pub fn load(vocab_path: &Path) -> DengjenResult<Self> {
        let file = std::fs::File::open(vocab_path).map_err(|e| {
            DengjenError::FailedToLoadResource(format!(
                "Failed to open Kokoro vocab at `{}`: {}",
                vocab_path.display(),
                e
            ))
        })?;
        let root: Value = serde_json::from_reader(file).map_err(|e| {
            DengjenError::FailedToLoadResource(format!(
                "Failed to parse Kokoro vocab at `{}`: {}",
                vocab_path.display(),
                e
            ))
        })?;
        let vocab_obj = root
            .get("model")
            .and_then(|m| m.get("vocab"))
            .and_then(|v| v.as_object())
            .ok_or_else(|| {
                DengjenError::FailedToLoadResource(format!(
                    "No `model.vocab` object found in `{}`",
                    vocab_path.display()
                ))
            })?;
        let mut map = HashMap::with_capacity(vocab_obj.len());
        for (token, id) in vocab_obj {
            let id = id.as_i64().ok_or_else(|| {
                DengjenError::FailedToLoadResource(format!(
                    "Vocab entry `{}` has a non-integer id in `{}`",
                    token,
                    vocab_path.display()
                ))
            })?;
            map.insert(token.clone(), id);
        }
        let bos_id = *map.get("$").ok_or_else(|| {
            DengjenError::FailedToLoadResource(format!(
                "BOS token `$` not found in vocab `{}`",
                vocab_path.display()
            ))
        })?;
        let max_token_chars = map.keys().map(|k| k.chars().count()).max().unwrap_or(1);
        Ok(Self {
            map,
            bos_id,
            max_token_chars,
        })
    }

    pub fn bos_id(&self) -> i64 {
        self.bos_id
    }

    pub fn eos_id(&self) -> i64 {
        self.bos_id
    }

    pub fn tokenize(&self, phonemes: &str) -> Vec<i64> {
        let chars: Vec<char> = phonemes.chars().collect();
        let mut ids = Vec::with_capacity(chars.len());
        let mut i = 0;
        while i < chars.len() {
            let limit = self.max_token_chars.min(chars.len() - i);
            let mut matched = false;
            for len in (1..=limit).rev() {
                let candidate: String = chars.iter().skip(i).take(len).collect();
                if let Some(&id) = self.map.get(&candidate) {
                    ids.push(id);
                    i += len;
                    matched = true;
                    break;
                }
            }
            if !matched {
                i += 1;
            }
        }
        ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp_vocab(dir: &std::path::Path, contents: &str) -> std::path::PathBuf {
        let path = dir.join("tokenizer.json");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(contents.as_bytes()).unwrap();
        path
    }

    const SAMPLE_VOCAB_JSON: &str = r#"{
        "model": {
            "vocab": {
                "$": 0,
                "t": 1,
                "ɛ": 2,
                "s": 3,
                "I": 4,
                "ʤ": 5,
                " ": 6
            }
        }
    }"#;

    #[test]
    fn load_parses_model_vocab_and_finds_bos_token() {
        let dir = std::env::temp_dir().join("dengjen_kokoro_vocab_test_valid");
        std::fs::create_dir_all(&dir).unwrap();
        let path = write_temp_vocab(&dir, SAMPLE_VOCAB_JSON);
        let vocab = Vocab::load(&path).unwrap();
        assert_eq!(vocab.bos_id(), 0);
        assert_eq!(vocab.eos_id(), 0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_errors_when_model_vocab_missing() {
        let dir = std::env::temp_dir().join("dengjen_kokoro_vocab_test_missing_vocab");
        std::fs::create_dir_all(&dir).unwrap();
        let path = write_temp_vocab(&dir, r#"{"model": {}}"#);
        let result = Vocab::load(&path);
        assert!(matches!(result, Err(DengjenError::FailedToLoadResource(_))));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_errors_when_bos_token_absent() {
        let dir = std::env::temp_dir().join("dengjen_kokoro_vocab_test_no_bos");
        std::fs::create_dir_all(&dir).unwrap();
        let path = write_temp_vocab(&dir, r#"{"model": {"vocab": {"t": 1}}}"#);
        let result = Vocab::load(&path);
        assert!(matches!(result, Err(DengjenError::FailedToLoadResource(_))));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn tokenize_matches_single_char_symbols() {
        let dir = std::env::temp_dir().join("dengjen_kokoro_vocab_test_tokenize_single");
        std::fs::create_dir_all(&dir).unwrap();
        let path = write_temp_vocab(&dir, SAMPLE_VOCAB_JSON);
        let vocab = Vocab::load(&path).unwrap();
        assert_eq!(vocab.tokenize("tɛst"), vec![1, 2, 3, 1]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn tokenize_prefers_longest_match_over_single_chars() {
        let dir = std::env::temp_dir().join("dengjen_kokoro_vocab_test_tokenize_longest");
        std::fs::create_dir_all(&dir).unwrap();

        let path = write_temp_vocab(&dir, SAMPLE_VOCAB_JSON);
        let vocab = Vocab::load(&path).unwrap();
        assert_eq!(vocab.tokenize("ʤ"), vec![5]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn tokenize_skips_unknown_characters() {
        let dir = std::env::temp_dir().join("dengjen_kokoro_vocab_test_tokenize_unknown");
        std::fs::create_dir_all(&dir).unwrap();
        let path = write_temp_vocab(&dir, SAMPLE_VOCAB_JSON);
        let vocab = Vocab::load(&path).unwrap();
        assert_eq!(vocab.tokenize("tzs"), vec![1, 3]);
        std::fs::remove_dir_all(&dir).ok();
    }
}
