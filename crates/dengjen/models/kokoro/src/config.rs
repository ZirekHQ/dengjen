use dengjen_tts_core::{DengjenError, DengjenResult};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
struct RawKokoroVoiceConfig {
    model_type: String,
    model_path: String,
    voices_dir: String,
    vocab_path: String,
    sample_rate: u32,
    voices: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct KokoroVoiceConfig {
    pub model_path: PathBuf,
    pub voices_dir: PathBuf,
    pub vocab_path: PathBuf,
    pub sample_rate: u32,
    pub voices: Vec<String>,
}

pub fn load_config(config_path: &Path) -> DengjenResult<KokoroVoiceConfig> {
    let file = std::fs::File::open(config_path).map_err(|e| {
        DengjenError::FailedToLoadResource(format!(
            "Failed to open Kokoro config at `{}`",
            config_path.display()
        ))
        .with_source(e)
    })?;
    let raw: RawKokoroVoiceConfig = serde_json::from_reader(file).map_err(|e| {
        DengjenError::FailedToLoadResource(format!(
            "Failed to parse Kokoro config at `{}`",
            config_path.display()
        ))
        .with_source(e)
    })?;
    if raw.model_type != "kokoro" {
        return Err(DengjenError::FailedToLoadResource(format!(
            "Expected model_type \"kokoro\", got \"{}\"",
            raw.model_type
        )));
    }
    let base_dir = config_path.parent().unwrap_or_else(|| Path::new("."));
    Ok(KokoroVoiceConfig {
        model_path: base_dir.join(raw.model_path),
        voices_dir: base_dir.join(raw.voices_dir),
        vocab_path: base_dir.join(raw.vocab_path),
        sample_rate: raw.sample_rate,
        voices: raw.voices,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp_config(dir: &std::path::Path, contents: &str) -> PathBuf {
        let path = dir.join("config.json");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(contents.as_bytes()).unwrap();
        path
    }

    #[test]
    fn load_config_parses_valid_manifest_with_paths_relative_to_config_dir() {
        let dir = std::env::temp_dir().join("dengjen_kokoro_test_valid");
        std::fs::create_dir_all(&dir).unwrap();
        let config_path = write_temp_config(
            &dir,
            r#"{
                "model_type": "kokoro",
                "model_path": "model.onnx",
                "voices_dir": "voices",
                "vocab_path": "tokenizer.json",
                "sample_rate": 24000,
                "voices": ["af_heart", "am_adam"]
            }"#,
        );
        let config = load_config(&config_path).unwrap();
        assert_eq!(config.model_path, dir.join("model.onnx"));
        assert_eq!(config.voices_dir, dir.join("voices"));
        assert_eq!(config.vocab_path, dir.join("tokenizer.json"));
        assert_eq!(config.sample_rate, 24000);
        assert_eq!(
            config.voices,
            vec!["af_heart".to_string(), "am_adam".to_string()]
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_config_errors_on_malformed_json() {
        let dir = std::env::temp_dir().join("dengjen_kokoro_test_malformed");
        std::fs::create_dir_all(&dir).unwrap();
        let config_path = write_temp_config(&dir, "{ not valid json");
        let result = load_config(&config_path);
        assert!(matches!(
            result.unwrap_err().kind(),
            DengjenError::FailedToLoadResource(_)
        ));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_config_errors_on_missing_file() {
        let result = load_config(Path::new("/nonexistent/path/config.json"));
        assert!(matches!(
            result.unwrap_err().kind(),
            DengjenError::FailedToLoadResource(_)
        ));
    }

    #[test]
    fn load_config_errors_on_wrong_model_type() {
        let dir = std::env::temp_dir().join("dengjen_kokoro_test_wrong_type");
        std::fs::create_dir_all(&dir).unwrap();
        let config_path = write_temp_config(
            &dir,
            r#"{
                "model_type": "piper",
                "model_path": "model.onnx",
                "voices_dir": "voices",
                "vocab_path": "tokenizer.json",
                "sample_rate": 24000,
                "voices": []
            }"#,
        );
        let result = load_config(&config_path);
        assert!(matches!(
            result.unwrap_err().kind(),
            DengjenError::FailedToLoadResource(_)
        ));
        std::fs::remove_dir_all(&dir).ok();
    }
}
