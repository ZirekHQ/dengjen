use dengjen_tts_core::{DengjenError, DengjenResult};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Deserialize, Clone)]
pub struct AudioConfig {
    pub sample_rate: u32,
}

#[derive(Deserialize, Clone)]
pub struct InferenceConfig {
    pub noise_scale: f32,
    pub length_scale: f32,
    pub noise_scale_w: f32,
}

#[derive(Deserialize, PartialEq, Debug, Clone)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PhonemizerConfig {
    Espeak { voice: String },
    Pinyin { model_dir: PathBuf },
}

#[derive(Deserialize)]
struct RawMeloVoiceConfig {
    audio: AudioConfig,
    phonemizer: PhonemizerConfig,
    phone_id_map: HashMap<String, Vec<i64>>,
    #[serde(default)]
    tone_id_map: HashMap<String, i64>,
    #[serde(default)]
    speaker_id_map: HashMap<String, i64>,
    #[serde(default)]
    default_speaker_id: Option<i64>,
    inference: InferenceConfig,
    model_path: String,
}

#[derive(Clone)]
pub struct MeloVoiceConfig {
    pub audio: AudioConfig,
    pub phonemizer: PhonemizerConfig,
    pub phone_id_map: HashMap<String, Vec<i64>>,
    pub tone_id_map: HashMap<String, i64>,
    pub speaker_id_map: HashMap<String, i64>,
    pub default_speaker_id: Option<i64>,
    pub inference: InferenceConfig,
    pub model_path: PathBuf,
}

pub fn load_config(path: &Path) -> DengjenResult<MeloVoiceConfig> {
    let raw = std::fs::read_to_string(path).map_err(|e| {
        DengjenError::FailedToLoadResource(format!(
            "Failed to read MeloTTS voice config at `{}`",
            path.display()
        ))
        .with_source(e)
    })?;
    let parsed: RawMeloVoiceConfig = serde_json::from_str(&raw).map_err(|e| {
        DengjenError::InvalidConfiguration(format!(
            "Failed to parse MeloTTS voice config at `{}`",
            path.display()
        ))
        .with_source(e)
    })?;
    let model_path = path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(&parsed.model_path);
    Ok(MeloVoiceConfig {
        audio: parsed.audio,
        phonemizer: parsed.phonemizer,
        phone_id_map: parsed.phone_id_map,
        tone_id_map: parsed.tone_id_map,
        speaker_id_map: parsed.speaker_id_map,
        default_speaker_id: parsed.default_speaker_id,
        inference: parsed.inference,
        model_path,
    })
}

pub(crate) fn map_phone_tone_pairs_to_ids(
    phone_id_map: &HashMap<String, Vec<i64>>,
    tone_id_map: &HashMap<String, i64>,
    pairs: &[(String, String)],
) -> (Vec<i64>, Vec<i64>) {
    let bos_id = phone_id_map
        .get("^")
        .and_then(|v| v.first())
        .copied()
        .unwrap_or(0);
    let eos_id = phone_id_map
        .get("$")
        .and_then(|v| v.first())
        .copied()
        .unwrap_or(0);
    let blank_id = phone_id_map
        .get("_")
        .and_then(|v| v.first())
        .copied()
        .unwrap_or(0);
    let blank_tone = tone_id_map.get("_").copied().unwrap_or(0);

    let longest_entry = phone_id_map
        .keys()
        .map(|entry| entry.chars().count())
        .max()
        .unwrap_or(1)
        .max(1);

    let mut phone_ids = vec![bos_id];
    let mut tone_ids = vec![blank_tone];

    for (chunk, tone_symbol) in pairs {
        let tone_id = tone_id_map.get(tone_symbol).copied().unwrap_or(blank_tone);
        let chars: Vec<char> = chunk.chars().collect();
        let mut cursor = 0;
        while cursor < chars.len() {
            let widest = longest_entry.min(chars.len() - cursor);
            let matched = (1..=widest).rev().find_map(|width| {
                let candidate: String = chars[cursor..cursor + width].iter().collect();
                phone_id_map
                    .get(&candidate)
                    .and_then(|ids| ids.first().map(|&id| (width, id)))
            });
            match matched {
                Some((width, id)) => {
                    phone_ids.push(id);
                    phone_ids.push(blank_id);
                    tone_ids.push(tone_id);
                    tone_ids.push(blank_tone);
                    cursor += width;
                }
                None => cursor += 1,
            }
        }
    }

    phone_ids.push(eos_id);
    tone_ids.push(blank_tone);
    (phone_ids, tone_ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE_CONFIG_JSON: &str = r#"{
        "audio": {"sample_rate": 44100},
        "phonemizer": {"type": "espeak", "voice": "en-us"},
        "phone_id_map": {"^": [1], "$": [2], "_": [3], "a": [4]},
        "inference": {"noise_scale": 0.667, "length_scale": 1.0, "noise_scale_w": 0.8},
        "model_path": "model.onnx"
    }"#;

    #[test]
    fn parses_a_minimal_espeak_voice_config() {
        let config: RawMeloVoiceConfig = serde_json::from_str(BASE_CONFIG_JSON).unwrap();
        assert_eq!(config.audio.sample_rate, 44100);
        assert_eq!(
            config.phonemizer,
            PhonemizerConfig::Espeak {
                voice: "en-us".to_string()
            }
        );
        assert_eq!(config.phone_id_map.get("a"), Some(&vec![4]));
        assert!(config.tone_id_map.is_empty());
        assert!(config.speaker_id_map.is_empty());
        assert_eq!(config.default_speaker_id, None);
    }

    #[test]
    fn parses_a_pinyin_voice_config_with_tone_id_map_and_speakers() {
        let json = r#"{
            "audio": {"sample_rate": 44100},
            "phonemizer": {"type": "pinyin", "model_dir": "/models/g2pw"},
            "phone_id_map": {"^": [1], "$": [2], "_": [3], "zh": [4], "ang": [5]},
            "tone_id_map": {"_": 0, "1": 1, "2": 2, "3": 3, "4": 4},
            "speaker_id_map": {"default": 0},
            "default_speaker_id": 0,
            "inference": {"noise_scale": 0.667, "length_scale": 1.0, "noise_scale_w": 0.8},
            "model_path": "model.onnx"
        }"#;
        let config: RawMeloVoiceConfig = serde_json::from_str(json).unwrap();
        assert_eq!(
            config.phonemizer,
            PhonemizerConfig::Pinyin {
                model_dir: PathBuf::from("/models/g2pw")
            }
        );
        assert_eq!(config.tone_id_map.get("2"), Some(&2));
        assert_eq!(config.speaker_id_map.get("default"), Some(&0));
        assert_eq!(config.default_speaker_id, Some(0));
    }

    #[test]
    fn rejects_a_config_with_an_unknown_phonemizer_type() {
        let json = r#"{
            "audio": {"sample_rate": 44100},
            "phonemizer": {"type": "festival", "voice": "en"},
            "phone_id_map": {},
            "inference": {"noise_scale": 0.667, "length_scale": 1.0, "noise_scale_w": 0.8},
            "model_path": "model.onnx"
        }"#;
        assert!(serde_json::from_str::<RawMeloVoiceConfig>(json).is_err());
    }

    #[test]
    fn load_config_errors_when_the_file_does_not_exist() {
        let result = load_config(Path::new("/nonexistent/config.json"));
        assert!(matches!(
            result.err().as_ref().map(DengjenError::kind),
            Some(DengjenError::FailedToLoadResource(_))
        ));
    }

    #[test]
    fn load_config_resolves_model_path_relative_to_the_config_file() {
        let dir = std::env::temp_dir().join("dengjen_melotts_config_test");
        std::fs::create_dir_all(&dir).unwrap();
        let config_path = dir.join("config.json");
        let json = r#"{
            "audio": {"sample_rate": 44100},
            "phonemizer": {"type": "espeak", "voice": "en-us"},
            "phone_id_map": {"^": [1], "$": [2], "_": [3], "a": [4]},
            "inference": {"noise_scale": 0.667, "length_scale": 1.0, "noise_scale_w": 0.8},
            "model_path": "model.onnx"
        }"#;
        std::fs::write(&config_path, json).unwrap();
        let config = load_config(&config_path).unwrap();
        assert_eq!(config.model_path, dir.join("model.onnx"));
        std::fs::remove_dir_all(&dir).ok();
    }

    fn test_phone_id_map() -> HashMap<String, Vec<i64>> {
        HashMap::from([
            ("^".to_string(), vec![1]),
            ("$".to_string(), vec![2]),
            ("_".to_string(), vec![3]),
            ("zh".to_string(), vec![4]),
            ("ang".to_string(), vec![5]),
            ("a".to_string(), vec![6]),
        ])
    }

    fn test_tone_id_map() -> HashMap<String, i64> {
        HashMap::from([
            ("_".to_string(), 0),
            ("1".to_string(), 1),
            ("2".to_string(), 2),
        ])
    }

    #[test]
    fn map_phone_tone_pairs_to_ids_wraps_output_in_bos_eos_with_blank_interleaving() {
        let pairs = vec![("a".to_string(), "_".to_string())];
        let (phone_ids, tone_ids) =
            map_phone_tone_pairs_to_ids(&test_phone_id_map(), &test_tone_id_map(), &pairs);
        assert_eq!(phone_ids, vec![1, 6, 3, 2]);
        assert_eq!(tone_ids, vec![0, 0, 0, 0]);
    }

    #[test]
    fn map_phone_tone_pairs_to_ids_assigns_a_syllables_tone_to_both_its_initial_and_finale() {
        let pairs = vec![("zhang".to_string(), "2".to_string())];
        let (phone_ids, tone_ids) =
            map_phone_tone_pairs_to_ids(&test_phone_id_map(), &test_tone_id_map(), &pairs);
        assert_eq!(phone_ids, vec![1, 4, 3, 5, 3, 2]);
        assert_eq!(tone_ids, vec![0, 2, 0, 2, 0, 0]);
    }

    #[test]
    fn map_phone_tone_pairs_to_ids_drops_characters_with_no_matching_phone_id() {
        let pairs = vec![("a!a".to_string(), "_".to_string())];
        let (phone_ids, _) =
            map_phone_tone_pairs_to_ids(&test_phone_id_map(), &test_tone_id_map(), &pairs);
        assert_eq!(phone_ids, vec![1, 6, 3, 6, 3, 2]);
    }
}
