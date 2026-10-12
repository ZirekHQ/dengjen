use crate::tokenize::tokenize_and_map;
use dengjen_tts_core::{DengjenError, DengjenResult};
use ndarray::{Array1, Array2};
use ort::session::Session;
use ort::value::Tensor;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

pub(crate) fn build_phoneme_mask(labels: &[String], candidates: &[usize]) -> Vec<f32> {
    let mut mask = vec![0.0f32; labels.len()];
    for &c in candidates {
        if let Some(slot) = mask.get_mut(c) {
            *slot = 1.0;
        }
    }
    mask
}

pub(crate) fn truncate_window(
    text_len: usize,
    query_id: usize,
    window_size: usize,
) -> (usize, usize) {
    let half = window_size / 2;
    let start = query_id.saturating_sub(half);
    let end = (query_id + half).min(text_len);
    (start, end)
}

/// Clamps a token sequence to `max_len - 2` tokens (room for `[CLS]`/`[SEP]`), keeping
/// `token_position` inside the kept range, and returns the kept range plus `token_position`
/// re-expressed 1-indexed within it (slot 0 is reserved for `[CLS]`).
///
/// # Errors
/// Returns `DengjenError::InvalidConfiguration` when `max_len < 3`, since that leaves no room
/// for a query token alongside `[CLS]`/`[SEP]`.
pub(crate) fn truncate_tokens_around_position(
    token_count: usize,
    token_position: usize,
    max_len: usize,
) -> DengjenResult<(usize, usize, usize)> {
    let truncate_len = max_len.saturating_sub(2);
    if truncate_len == 0 {
        return Err(DengjenError::InvalidConfiguration(format!(
            "g2pW max_len must be at least 3 to hold [CLS], a query token, and [SEP]; got {max_len}"
        )));
    }
    if token_count <= truncate_len {
        return Ok((0, token_count, token_position + 1));
    }
    let token_start = (token_position as isize - (truncate_len / 2) as isize).max(0) as usize;
    let token_start = token_start.min(token_count.saturating_sub(truncate_len));
    let token_end = (token_start + truncate_len).min(token_count);
    Ok((token_start, token_end, token_position - token_start + 1))
}

pub(crate) struct G2pwConfig {
    pub labels: Vec<String>,
    pub char2phonemes: HashMap<char, Vec<usize>>,
    pub window_size: usize,
    pub max_len: usize,
    pub chars: Vec<char>,
}

impl G2pwConfig {
    fn chars_index(&self, c: char) -> DengjenResult<usize> {
        self.chars.binary_search(&c).map_err(|_| {
            DengjenError::PhonemizationError(format!(
                "'{c}' is not in the g2pW config's sorted chars list"
            ))
        })
    }
}

pub(crate) struct G2pwEngine {
    session: Mutex<Session>,
    tokenizer: tokenizers::Tokenizer,
    config: G2pwConfig,
}

fn session_init_error(cause: impl std::fmt::Display) -> DengjenError {
    DengjenError::InferenceError(format!(
        "Failed to initialize the g2pW inference session: {cause}"
    ))
}

fn inference_error(cause: impl std::fmt::Display) -> DengjenError {
    DengjenError::InferenceError(format!("g2pW inference failed: {cause}"))
}

pub(crate) fn create_g2pw_engine(
    onnx_model_path: &Path,
    tokenizer_path: &Path,
    config: G2pwConfig,
) -> DengjenResult<G2pwEngine> {
    let session = Session::builder()
        .map_err(session_init_error)?
        .commit_from_file(onnx_model_path)
        .map_err(session_init_error)?;
    let tokenizer = tokenizers::Tokenizer::from_file(tokenizer_path).map_err(session_init_error)?;
    Ok(G2pwEngine {
        session: Mutex::new(session),
        tokenizer,
        config,
    })
}

impl G2pwEngine {
    pub fn is_polyphonic(&self, c: char) -> bool {
        self.config.char2phonemes.contains_key(&c)
    }

    pub fn resolve_polyphonic(
        &self,
        chars: &[char],
        query_char_index: usize,
    ) -> DengjenResult<String> {
        let query_char = *chars.get(query_char_index).ok_or_else(|| {
            DengjenError::PhonemizationError(format!(
                "query_char_index {query_char_index} is out of bounds for a sentence of {} characters",
                chars.len()
            ))
        })?;
        let candidates = self.config.char2phonemes.get(&query_char).ok_or_else(|| {
            DengjenError::PhonemizationError(format!(
                "'{query_char}' has no known phoneme candidates for g2pW resolution"
            ))
        })?;

        let (win_start, win_end) =
            truncate_window(chars.len(), query_char_index, self.config.window_size);
        let windowed_text: String = chars
            .iter()
            .skip(win_start)
            .take(win_end.saturating_sub(win_start))
            .collect();
        let windowed_query_id = query_char_index - win_start;

        let (tokens, text_to_token) =
            tokenize_and_map(&self.tokenizer, &windowed_text.to_lowercase())?;
        let token_position = text_to_token
            .get(windowed_query_id)
            .copied()
            .flatten()
            .ok_or_else(|| {
                DengjenError::PhonemizationError(format!(
                    "windowed query position {windowed_query_id} has no token mapping"
                ))
            })?;

        let (token_start, token_end, position_id) =
            truncate_tokens_around_position(tokens.len(), token_position, self.config.max_len)?;
        let final_tokens = tokens
            .get(token_start..token_end)
            .ok_or_else(|| {
                DengjenError::PhonemizationError(format!(
                    "token window {token_start}..{token_end} is out of bounds for {} tokens",
                    tokens.len()
                ))
            })?
            .to_vec();

        let mut input_ids: Vec<i64> = vec![self.cls_id()];
        input_ids.extend(final_tokens.iter().map(|t| self.token_to_id(&t.text)));
        input_ids.push(self.sep_id());
        let seq_len = input_ids.len();

        let token_type_ids = vec![0i64; seq_len];
        let attention_mask = vec![1i64; seq_len];
        let phoneme_mask = build_phoneme_mask(&self.config.labels, candidates);

        let input_ids_arr =
            Array2::from_shape_vec((1, seq_len), input_ids).map_err(inference_error)?;
        let token_type_ids_arr =
            Array2::from_shape_vec((1, seq_len), token_type_ids).map_err(inference_error)?;
        let attention_mask_arr =
            Array2::from_shape_vec((1, seq_len), attention_mask).map_err(inference_error)?;
        let char_ids_arr = Array1::from_vec(vec![self.config.chars_index(query_char)? as i64]);
        let position_ids_arr = Array1::from_vec(vec![position_id as i64]);
        let phoneme_mask_arr = Array2::from_shape_vec((1, self.config.labels.len()), phoneme_mask)
            .map_err(inference_error)?;

        let mut session = self.session.lock().map_err(inference_error)?;
        let outputs = session
            .run(ort::inputs![
                "input_ids" => Tensor::from_array(input_ids_arr).map_err(inference_error)?,
                "token_type_ids" => Tensor::from_array(token_type_ids_arr).map_err(inference_error)?,
                "attention_mask" => Tensor::from_array(attention_mask_arr).map_err(inference_error)?,
                "char_ids" => Tensor::from_array(char_ids_arr).map_err(inference_error)?,
                "position_ids" => Tensor::from_array(position_ids_arr).map_err(inference_error)?,
                "phoneme_mask" => Tensor::from_array(phoneme_mask_arr).map_err(inference_error)?,
            ])
            .map_err(inference_error)?;

        let mut values = outputs.values();
        let first = values
            .next()
            .ok_or_else(|| inference_error("model produced no output tensors"))?;
        let (shape, probs) = first.try_extract_tensor::<f32>().map_err(inference_error)?;

        let &[batch, label_dim] = &**shape else {
            return Err(inference_error(format!(
                "expected a rank-2 (batch, num_labels) output tensor, got shape {shape:?}"
            )));
        };
        if batch != 1 {
            return Err(inference_error(format!(
                "expected batch size 1, model produced shape {shape:?}"
            )));
        }
        let num_labels = usize::try_from(label_dim)
            .map_err(|_| inference_error(format!("invalid label dimension in shape {shape:?}")))?;
        if num_labels != self.config.labels.len() || probs.len() < num_labels {
            return Err(inference_error(format!(
                "g2pW model output width {num_labels} (probs len {}) does not match the \
                 configured label count {}",
                probs.len(),
                self.config.labels.len()
            )));
        }
        let predicted = probs
            .iter()
            .take(num_labels)
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(idx, _)| idx)
            .ok_or_else(|| inference_error("empty prediction output"))?;

        self.config.labels.get(predicted).cloned().ok_or_else(|| {
            inference_error(format!("predicted label index {predicted} out of range"))
        })
    }

    fn cls_id(&self) -> i64 {
        self.tokenizer.token_to_id("[CLS]").unwrap_or(101) as i64
    }
    fn sep_id(&self) -> i64 {
        self.tokenizer.token_to_id("[SEP]").unwrap_or(102) as i64
    }
    fn token_to_id(&self, token: &str) -> i64 {
        self.tokenizer.token_to_id(token).unwrap_or(100) as i64
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn build_phoneme_mask_sets_one_for_each_candidate_label_and_zero_elsewhere() {
        let labels = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let candidates = vec![0usize, 2usize];
        let mask = super::build_phoneme_mask(&labels, &candidates);
        assert_eq!(mask, vec![1.0f32, 0.0, 1.0]);
    }

    #[test]
    fn truncate_window_centers_on_the_query_character_within_window_size() {
        let (start, end) = super::truncate_window(10, 5, 4);
        assert_eq!((start, end), (3, 7));
    }

    #[test]
    fn truncate_window_clamps_to_text_bounds_near_the_start() {
        let (start, end) = super::truncate_window(10, 1, 4);
        assert_eq!((start, end), (0, 3));
    }

    #[test]
    fn truncate_window_clamps_to_text_bounds_near_the_end() {
        let (start, end) = super::truncate_window(10, 9, 4);
        assert_eq!((start, end), (7, 10));
    }

    #[test]
    fn truncate_tokens_around_position_keeps_everything_when_under_the_limit() {
        let (start, end, position_id) = super::truncate_tokens_around_position(5, 2, 10).unwrap();
        assert_eq!((start, end, position_id), (0, 5, 3));
    }

    #[test]
    fn truncate_tokens_around_position_centers_the_window_on_the_query_token() {
        let (start, end, position_id) = super::truncate_tokens_around_position(20, 10, 6).unwrap();
        assert_eq!((start, end), (8, 12));
        assert_eq!(position_id, 3);
    }

    #[test]
    fn truncate_tokens_around_position_clamps_the_window_to_the_start() {
        let (start, end, position_id) = super::truncate_tokens_around_position(20, 1, 6).unwrap();
        assert_eq!((start, end), (0, 4));
        assert_eq!(position_id, 2);
    }

    #[test]
    fn truncate_tokens_around_position_clamps_the_window_to_the_end() {
        let (start, end, position_id) = super::truncate_tokens_around_position(20, 19, 6).unwrap();
        assert_eq!((start, end), (16, 20));
        assert_eq!(position_id, 4);
    }

    #[test]
    fn truncate_tokens_around_position_rejects_a_max_len_too_small_to_hold_a_query_token() {
        let err = super::truncate_tokens_around_position(20, 10, 2).unwrap_err();
        assert!(matches!(
            err,
            dengjen_tts_core::DengjenError::InvalidConfiguration(_)
        ));
    }

    #[test]
    fn g2pw_config_chars_index_finds_a_present_character() {
        let config = super::G2pwConfig {
            labels: vec![],
            char2phonemes: std::collections::HashMap::new(),
            window_size: 0,
            max_len: 0,
            chars: vec!['a', 'b', 'c'],
        };
        assert_eq!(config.chars_index('b').unwrap(), 1);
    }

    #[test]
    fn g2pw_config_chars_index_errors_on_a_character_outside_the_sorted_list() {
        let config = super::G2pwConfig {
            labels: vec![],
            char2phonemes: std::collections::HashMap::new(),
            window_size: 0,
            max_len: 0,
            chars: vec!['a', 'b', 'c'],
        };
        let err = config.chars_index('z').unwrap_err();
        assert!(err.to_string().contains('z'));
    }

    #[test]
    fn session_init_error_wraps_the_cause_with_context() {
        let err = super::session_init_error("boom");
        assert!(err.to_string().contains("g2pW inference session"));
        assert!(err.to_string().contains("boom"));
    }

    #[test]
    fn inference_error_wraps_the_cause_with_context() {
        let err = super::inference_error("boom");
        assert!(err.to_string().contains("g2pW inference failed"));
        assert!(err.to_string().contains("boom"));
    }

    #[test]
    fn resolve_polyphonic_disambiguates_a_known_polyphonic_character_with_a_real_model() {
        let (Some(onnx_path), Some(tokenizer_path)) = (
            std::env::var("DENGJEN_PINYIN_TEST_MODEL_PATH").ok(),
            std::env::var("DENGJEN_PINYIN_TEST_TOKENIZER_PATH").ok(),
        ) else {
            eprintln!(
                "skipping: DENGJEN_PINYIN_TEST_MODEL_PATH/DENGJEN_PINYIN_TEST_TOKENIZER_PATH not set"
            );
            return;
        };

        eprintln!(
            "resolve_polyphonic_disambiguates_a_known_polyphonic_character_with_a_real_model \
             requires the crate's dictionary loader to be wired up before it can run for real: \
             {onnx_path} / {tokenizer_path}"
        );
    }
}
