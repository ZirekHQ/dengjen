use crate::chars::{
    char_to_id_map, dagesh_classes, is_niqqud, niqqud_classes, normalize, sin_classes, RAFE,
};
use dengjen_tts_core::{DengjenError, DengjenResult};
use ndarray::Array2;
use ort::session::Session;
use ort::value::{Shape, Tensor};
use std::path::Path;
use std::sync::Mutex;

fn can_dagesh(letter: char) -> bool {
    "\u{05D1}\u{05D2}\u{05D3}\u{05D4}\u{05D5}\u{05D6}\u{05D8}\u{05D9}\u{05DB}\u{05DC}\u{05DE}\u{05E0}\u{05E1}\u{05E4}\u{05E6}\u{05E7}\u{05E9}\u{05EA}\u{05DA}\u{05E3}"
        .contains(letter)
}

fn can_sin(letter: char) -> bool {
    letter == '\u{05E9}'
}

fn can_niqqud(letter: char) -> bool {
    "\u{05D0}\u{05D1}\u{05D2}\u{05D3}\u{05D4}\u{05D5}\u{05D6}\u{05D7}\u{05D8}\u{05D9}\u{05DB}\u{05DC}\u{05DE}\u{05E0}\u{05E1}\u{05E2}\u{05E4}\u{05E6}\u{05E7}\u{05E8}\u{05E9}\u{05EA}\u{05DA}\u{05DF}"
        .contains(letter)
}

fn merge_diacritics(
    letters: &[char],
    niqqud_ids: &[usize],
    dagesh_ids: &[usize],
    sin_ids: &[usize],
) -> String {
    let niqqud_chars = niqqud_classes();
    let dagesh_chars = dagesh_classes();
    let sin_chars = sin_classes();

    let class_char = |ids: &[usize], classes: &[char], i: usize| -> Option<char> {
        let id = *ids.get(i)?;
        classes.get(id.checked_sub(1)?).copied()
    };

    let mut out = String::new();
    for (i, &letter) in letters.iter().enumerate() {
        out.push(letter);
        if can_dagesh(letter) {
            out.extend(class_char(dagesh_ids, &dagesh_chars, i));
        }
        if can_sin(letter) {
            out.extend(class_char(sin_ids, &sin_chars, i));
        }
        if can_niqqud(letter) {
            out.extend(class_char(niqqud_ids, &niqqud_chars, i));
        }
    }
    out.replace(RAFE, "")
}

pub struct NakdimonEngine(Mutex<Session>);

pub fn create_nakdimon_engine(model_path: &Path) -> DengjenResult<NakdimonEngine> {
    let session = dengjen_tts_core::session_builder()
        .map_err(session_init_error)?
        .commit_from_file(model_path)
        .map_err(session_init_error)?;
    Ok(NakdimonEngine(Mutex::new(session)))
}

fn session_init_error(cause: ort::Error) -> DengjenError {
    DengjenError::InferenceError(format!(
        "Failed to initialize the Nakdimon inference session: {cause}"
    ))
}

fn inference_error(cause: impl std::fmt::Display) -> DengjenError {
    DengjenError::InferenceError(format!("Nakdimon inference failed: {cause}"))
}

pub fn num_classes(shape: &Shape, seq_len: usize, expected_classes: usize) -> DengjenResult<usize> {
    if shape.len() != 3 {
        return Err(inference_error(format!(
            "expected a rank-3 output tensor, got shape {shape:?}"
        )));
    }
    let Some(&num_classes) = shape.get(2) else {
        return Err(inference_error(format!(
            "expected a rank-3 output tensor, got shape {shape:?}"
        )));
    };
    if shape.first() != Some(&1) {
        return Err(inference_error(format!(
            "expected batch size 1, model produced shape {shape:?}"
        )));
    }
    if shape.get(1) != Some(&(seq_len as i64)) {
        return Err(inference_error(format!(
            "expected output sequence length {seq_len}, model produced shape {shape:?}"
        )));
    }

    let num_classes = usize::try_from(num_classes).map_err(|_| {
        inference_error(format!(
            "expected a non-negative class dimension, model produced shape {shape:?}"
        ))
    })?;
    if num_classes != expected_classes {
        return Err(inference_error(format!(
            "expected {expected_classes} classes, model produced shape {shape:?}"
        )));
    }
    Ok(num_classes)
}

fn argmax_per_position(data: &[f32], seq_len: usize, num_classes: usize) -> Vec<usize> {
    (0..seq_len)
        .map(|pos| {
            data.get(pos * num_classes..(pos + 1) * num_classes)
                .and_then(|row| {
                    row.iter()
                        .enumerate()
                        .max_by(|a, b| a.1.total_cmp(b.1))
                        .map(|(idx, _)| idx)
                })
                .unwrap_or(0)
        })
        .collect()
}

fn remove_niqqud(text: &str) -> String {
    text.chars().filter(|&c| !is_niqqud(c)).collect()
}

impl NakdimonEngine {
    pub fn diacritize(&self, text: &str) -> DengjenResult<String> {
        let text = remove_niqqud(text);
        let letters: Vec<char> = text.chars().collect();
        if letters.is_empty() {
            return Ok(text);
        }
        let char_ids = char_to_id_map();
        let input_ids: Vec<f32> = letters
            .iter()
            .map(|&c| {
                let normalized = normalize(c);
                *char_ids.get(&normalized).unwrap_or(&0) as f32
            })
            .collect();
        let seq_len = letters.len();
        let input =
            Array2::<f32>::from_shape_vec((1, seq_len), input_ids).map_err(inference_error)?;

        let mut session = self.0.lock().map_err(inference_error)?;
        let outputs = session
            .run(ort::inputs![
                Tensor::from_array(input).map_err(inference_error)?
            ])
            .map_err(inference_error)?;
        let mut values = outputs.values();
        let (Some(niqqud), Some(dagesh), Some(sin)) = (values.next(), values.next(), values.next())
        else {
            return Err(inference_error(format!(
                "expected 3 output tensors (niqqud, dagesh, sin), model produced {}",
                outputs.len()
            )));
        };

        let (n_shape, n_data) = niqqud
            .try_extract_tensor::<f32>()
            .map_err(inference_error)?;
        let (d_shape, d_data) = dagesh
            .try_extract_tensor::<f32>()
            .map_err(inference_error)?;
        let (s_shape, s_data) = sin.try_extract_tensor::<f32>().map_err(inference_error)?;

        let niqqud_ids = argmax_per_position(
            n_data,
            seq_len,
            num_classes(n_shape, seq_len, niqqud_classes().len() + 1)?,
        );
        let dagesh_ids = argmax_per_position(
            d_data,
            seq_len,
            num_classes(d_shape, seq_len, dagesh_classes().len() + 1)?,
        );
        let sin_ids = argmax_per_position(
            s_data,
            seq_len,
            num_classes(s_shape, seq_len, sin_classes().len() + 1)?,
        );

        Ok(merge_diacritics(
            &letters,
            &niqqud_ids,
            &dagesh_ids,
            &sin_ids,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_diacritics_appends_dagesh_sin_niqqud_in_that_order() {
        let letters = vec!['\u{05D1}'];
        let niqqud_ids = vec![find_niqqud_class_id('\u{05B7}')];
        let dagesh_ids = dagesh_id_for_dagesh_letter();
        let sin_ids = vec![2usize];
        let merged = merge_diacritics(&letters, &niqqud_ids, &dagesh_ids, &sin_ids);
        assert_eq!(merged, "\u{05D1}\u{05BC}\u{05B7}");
    }

    #[test]
    fn merge_diacritics_skips_niqqud_for_letters_that_cannot_take_it() {
        let letters = vec![' '];
        let merged = merge_diacritics(&letters, &[0], &[0], &[0]);
        assert_eq!(merged, " ");
    }

    fn find_niqqud_class_id(target: char) -> usize {
        crate::chars::niqqud_classes()
            .iter()
            .position(|&c| c == target)
            .expect("target must be a real niqqud class")
            + 1
    }

    fn dagesh_id_for_dagesh_letter() -> Vec<usize> {
        vec![2]
    }

    #[test]
    fn inference_error_wraps_the_cause_with_context() {
        let err = super::inference_error("boom");
        assert!(err.to_string().contains("Nakdimon inference failed"));
        assert!(err.to_string().contains("boom"));
    }

    #[test]
    fn argmax_per_position_picks_the_highest_scoring_class_at_each_position() {
        // seq_len=2, num_classes=3: position 0 favors class 2, position 1 favors class 0.
        let data = [0.1f32, 0.2, 0.9, 0.7, 0.05, 0.1];
        let ids = argmax_per_position(&data, 2, 3);
        assert_eq!(ids, vec![2, 0]);
    }

    #[test]
    fn remove_niqqud_strips_points_and_leaves_base_letters_untouched() {
        let pointed = "\u{05E9}\u{05C1}\u{05B7}\u{05DC}\u{05B9}\u{05D5}\u{05DD}\u{05B8}";
        assert_eq!(remove_niqqud(pointed), "\u{05E9}\u{05DC}\u{05D5}\u{05DD}");
    }

    #[test]
    fn remove_niqqud_keeps_maqaf_and_sof_pasuq() {
        let text = "\u{05E2}\u{05B7}\u{05DC}\u{05BE}\u{05D9}\u{05D3}\u{05D9}\u{05C3}";
        assert_eq!(
            remove_niqqud(text),
            "\u{05E2}\u{05DC}\u{05BE}\u{05D9}\u{05D3}\u{05D9}\u{05C3}"
        );
    }

    #[test]
    fn num_classes_accepts_the_expected_model_output_shape() {
        let shape = Shape::new(vec![1, 4, 7]);

        assert_eq!(num_classes(&shape, 4, 7).unwrap(), 7);
    }

    #[test]
    fn num_classes_rejects_wrong_rank_batch_sequence_and_class_dimensions() {
        let cases = [
            (Shape::new(vec![4, 7]), 4, 7),
            (Shape::new(vec![2, 4, 7]), 4, 7),
            (Shape::new(vec![1, 3, 7]), 4, 7),
            (Shape::new(vec![1, 4, 6]), 4, 7),
            (Shape::new(vec![1, 4, 7, 1]), 4, 7),
        ];

        for (shape, seq_len, expected_classes) in cases {
            assert!(num_classes(&shape, seq_len, expected_classes).is_err());
        }
    }

    #[test]
    fn num_classes_rejects_a_negative_class_dimension_instead_of_wrapping_to_usize_max() {
        let shape = Shape::new(vec![1, 4, -1]);
        assert!(num_classes(&shape, 4, usize::MAX).is_err());
    }

    #[test]
    fn diacritize_restores_niqqud_with_a_real_model() {
        let Ok(model_path) = std::env::var("DENGJEN_NAKDIMON_TEST_MODEL_PATH") else {
            eprintln!("skipping: DENGJEN_NAKDIMON_TEST_MODEL_PATH not set");
            return;
        };
        let engine = create_nakdimon_engine(std::path::Path::new(&model_path)).unwrap();
        let result = engine.diacritize("שלום").unwrap();
        assert_ne!(result, "שלום", "expected niqqud marks to be added");
    }
}
