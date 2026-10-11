use dengjen_tts_core::{DengjenError, DengjenResult};
use ndarray::{Array2, Axis, Slice};
use std::collections::HashMap;
use std::path::{Component, Path};

const STYLE_DIM: usize = 256;
pub const MAX_TOKEN_LEN: usize = 510;
const EXPECTED_FILE_BYTES: usize = MAX_TOKEN_LEN * STYLE_DIM * 4;

fn is_plain_file_stem(voice_name: &str) -> bool {
    let mut components = Path::new(voice_name).components();
    matches!(
        (components.next(), components.next()),
        (Some(Component::Normal(name)), None) if name == voice_name
    )
}

pub struct VoiceStyles {
    per_voice: HashMap<String, Array2<f32>>,
}

impl VoiceStyles {
    pub fn load(voices_dir: &Path, voices: &[String]) -> DengjenResult<Self> {
        let mut per_voice = HashMap::with_capacity(voices.len());
        for voice_name in voices {
            if !is_plain_file_stem(voice_name) {
                return Err(DengjenError::FailedToLoadResource(format!(
                    "Kokoro voice name `{voice_name}` must be a plain file name without path separators"
                )));
            }
            let path = voices_dir.join(format!("{voice_name}.bin"));
            let bytes = std::fs::read(&path).map_err(|e| {
                DengjenError::FailedToLoadResource(format!(
                    "Failed to read Kokoro voice style file `{}`: {}",
                    path.display(),
                    e
                ))
            })?;
            if bytes.len() != EXPECTED_FILE_BYTES {
                return Err(DengjenError::FailedToLoadResource(format!(
                    "Kokoro voice style file `{}` is {} bytes, expected {} ({} rows x {} dims x 4 bytes)",
                    path.display(),
                    bytes.len(),
                    EXPECTED_FILE_BYTES,
                    MAX_TOKEN_LEN,
                    STYLE_DIM
                )));
            }
            let (chunks, _) = bytes.as_chunks::<4>();
            let floats: Vec<f32> = chunks.iter().copied().map(f32::from_le_bytes).collect();
            let table = Array2::from_shape_vec((MAX_TOKEN_LEN, STYLE_DIM), floats)
                .map_err(|e| DengjenError::with_message(e.to_string()))?;
            per_voice.insert(voice_name.clone(), table);
        }
        Ok(Self { per_voice })
    }

    pub fn style_for(&self, voice_name: &str, token_len: usize) -> DengjenResult<Array2<f32>> {
        let table = self.per_voice.get(voice_name).ok_or_else(|| {
            DengjenError::OperationError(format!("Unknown Kokoro voice: `{}`", voice_name))
        })?;
        let row_index = token_len.saturating_sub(1).min(MAX_TOKEN_LEN - 1);

        Ok(table
            .slice_axis(Axis(0), Slice::from(row_index..row_index + 1))
            .to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_synthetic_voice_file(dir: &Path, voice_name: &str) -> std::path::PathBuf {
        let path = dir.join(format!("{voice_name}.bin"));
        let mut bytes = Vec::with_capacity(EXPECTED_FILE_BYTES);
        for row in 0..MAX_TOKEN_LEN {
            for _ in 0..STYLE_DIM {
                bytes.extend_from_slice(&(row as f32).to_le_bytes());
            }
        }
        std::fs::write(&path, &bytes).unwrap();
        path
    }

    #[test]
    fn load_reads_a_correctly_shaped_voice_file() {
        let dir = std::env::temp_dir().join("dengjen_kokoro_voice_style_test_valid");
        std::fs::create_dir_all(&dir).unwrap();
        write_synthetic_voice_file(&dir, "test_voice");
        let styles = VoiceStyles::load(&dir, &["test_voice".to_string()]).unwrap();
        let row0 = styles.style_for("test_voice", 1).unwrap();
        assert_eq!(row0.shape(), &[1, 256]);
        assert_eq!(row0[[0, 0]], 0.0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_rejects_voice_names_that_escape_the_voices_dir() {
        let dir = std::env::temp_dir().join("dengjen_kokoro_voice_style_test_escape");
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["../outside", "/etc/passwd", "sub/voice", "..", ""] {
            let result = VoiceStyles::load(&dir, &[name.to_string()]);
            assert!(
                matches!(&result, Err(DengjenError::FailedToLoadResource(msg)) if msg.contains("plain file name")),
                "voice name `{name}` must be rejected"
            );
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_errors_when_voice_file_missing() {
        let dir = std::env::temp_dir().join("dengjen_kokoro_voice_style_test_missing");
        std::fs::create_dir_all(&dir).unwrap();
        let result = VoiceStyles::load(&dir, &["nonexistent_voice".to_string()]);
        assert!(matches!(result, Err(DengjenError::FailedToLoadResource(_))));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_errors_when_voice_file_is_wrong_size() {
        let dir = std::env::temp_dir().join("dengjen_kokoro_voice_style_test_wrong_size");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("bad_voice.bin"), vec![0u8; 100]).unwrap();
        let result = VoiceStyles::load(&dir, &["bad_voice".to_string()]);
        assert!(matches!(result, Err(DengjenError::FailedToLoadResource(_))));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn style_for_unknown_voice_returns_operation_error() {
        let dir = std::env::temp_dir().join("dengjen_kokoro_voice_style_test_unknown");
        std::fs::create_dir_all(&dir).unwrap();
        write_synthetic_voice_file(&dir, "known_voice");
        let styles = VoiceStyles::load(&dir, &["known_voice".to_string()]).unwrap();
        let result = styles.style_for("nonexistent_voice", 5);
        assert!(matches!(result, Err(DengjenError::OperationError(_))));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn style_for_returns_the_row_matching_token_length() {
        let dir = std::env::temp_dir().join("dengjen_kokoro_voice_style_test_row_select");
        std::fs::create_dir_all(&dir).unwrap();
        write_synthetic_voice_file(&dir, "test_voice");
        let styles = VoiceStyles::load(&dir, &["test_voice".to_string()]).unwrap();
        let result = styles.style_for("test_voice", 42).unwrap();
        assert_eq!(result[[0, 0]], 41.0);
        assert_eq!(result[[0, 255]], 41.0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn style_for_clamps_token_len_to_available_rows() {
        let dir = std::env::temp_dir().join("dengjen_kokoro_voice_style_test_clamp");
        std::fs::create_dir_all(&dir).unwrap();
        write_synthetic_voice_file(&dir, "test_voice");
        let styles = VoiceStyles::load(&dir, &["test_voice".to_string()]).unwrap();
        let result = styles.style_for("test_voice", 10000).unwrap();
        assert_eq!(result.shape(), &[1, 256]);
        assert_eq!(result[[0, 0]], 509.0);
        std::fs::remove_dir_all(&dir).ok();
    }
}
