use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tracing_test::traced_test;

fn synthetic_model_config_json() -> &'static str {
    r#"{
        "audio": {"sample_rate": 24000},
        "phonemizer": {"type": "espeak", "voice": "en-us"},
        "phone_id_map": {"^": [1], "$": [2], "_": [3], "t": [4]},
        "tone_id_map": {"_": 0},
        "inference": {"noise_scale": 0.667, "length_scale": 1.0, "noise_scale_w": 0.8},
        "model_path": "model.onnx"
    }"#
}

fn load_synthetic_model(dir_name: &str) -> Arc<dyn dengjen_tts_core::DengjenModel + Send + Sync> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_path = manifest_dir.join("tests/fixtures/synthetic_melotts.onnx");

    let dir = std::env::temp_dir().join(dir_name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(&fixture_path, dir.join("model.onnx")).unwrap();
    let config_path = dir.join("config.json");
    std::fs::write(&config_path, synthetic_model_config_json()).unwrap();

    let model = dengjen_tts_melotts::from_config_path(&config_path)
        .expect("failed to load synthetic MeloTTS model");
    std::fs::remove_dir_all(&dir).ok();
    model
}

#[traced_test]
#[test]
fn synthesizes_against_synthetic_fixture_without_panicking() {
    let model = load_synthetic_model("dengjen_melotts_synthetic_inference_test");

    let audio = model
        .speak_one_sentence("t:_".to_string())
        .expect("synthesis against synthetic fixture failed");

    assert_eq!(audio.info.sample_rate, 24000);
    assert_eq!(audio.samples.into_vec().len(), 16000);
    assert!(logs_contain("speak_one_sentence"));
}

#[test]
fn set_fallback_synthesis_config_changes_inference_output_values() {
    let model = load_synthetic_model("dengjen_melotts_synthetic_inference_fallback_test");

    let default_audio = model
        .speak_one_sentence("t:_".to_string())
        .expect("synthesis against synthetic fixture failed");

    let mut parameters = HashMap::new();
    parameters.insert("noise_scale".to_string(), 1.5f32);
    model
        .set_fallback_synthesis_config(&dengjen_tts_core::SynthesisConfig {
            speaker: None,
            parameters,
        })
        .expect("failed to set fallback synthesis config");

    let overridden_audio = model
        .speak_one_sentence("t:_".to_string())
        .expect("synthesis against synthetic fixture failed");

    assert_ne!(
        default_audio.samples.into_vec(),
        overridden_audio.samples.into_vec(),
        "expected a noise_scale set via set_fallback_synthesis_config to change inference \
         output, but output samples were identical -- this means the live fallback config \
         is being ignored"
    );
}

#[traced_test]
#[test]
fn from_config_path_reports_a_load_error_for_a_missing_onnx_file() {
    let dir = std::env::temp_dir().join("dengjen_melotts_missing_model_path_test");
    std::fs::create_dir_all(&dir).unwrap();
    let config_path = dir.join("config.json");
    std::fs::write(&config_path, synthetic_model_config_json()).unwrap();
    // synthetic_model_config_json() names "model.onnx" but this test never copies the
    // fixture into `dir`, so the ONNX session load itself is what's expected to fail.

    let result = dengjen_tts_melotts::from_config_path(&config_path);
    std::fs::remove_dir_all(&dir).ok();

    let err = result
        .err()
        .expect("loading a missing ONNX file should fail");
    assert!(
        err.to_string()
            .contains("Failed to load MeloTTS ONNX model"),
        "unexpected error message: {err}"
    );
    assert!(logs_contain("from_config_path"));
}

#[test]
fn audio_output_info_reflects_the_configured_sample_rate() {
    let model = load_synthetic_model("dengjen_melotts_synthetic_audio_output_info_test");
    let info = model.audio_output_info().unwrap();
    assert_eq!(info.sample_rate, 24000);
    assert_eq!(info.num_channels, 1);
    assert_eq!(info.sample_width, 2);
}

#[test]
fn get_default_synthesis_config_reflects_the_config_file_inference_settings() {
    let model = load_synthetic_model("dengjen_melotts_synthetic_default_synth_config_test");
    let default = model
        .get_default_synthesis_config()
        .unwrap()
        .expect("MeloTTS models always report a default synthesis config");
    assert_eq!(default.speaker, None);
    assert_eq!(default.parameters.get("noise_scale"), Some(&0.667));
    assert_eq!(default.parameters.get("length_scale"), Some(&1.0));
    assert_eq!(default.parameters.get("noise_scale_w"), Some(&0.8));
}

#[test]
fn fallback_synthesis_config_is_none_until_explicitly_set() {
    let model = load_synthetic_model("dengjen_melotts_synthetic_fallback_config_roundtrip_test");
    assert_eq!(model.get_fallback_synthesis_config().unwrap(), None);

    let mut parameters = HashMap::new();
    parameters.insert("noise_scale".to_string(), 1.5f32);
    let config = dengjen_tts_core::SynthesisConfig {
        speaker: None,
        parameters,
    };
    model.set_fallback_synthesis_config(&config).unwrap();

    assert_eq!(model.get_fallback_synthesis_config().unwrap(), Some(config));
}

#[test]
fn get_speakers_returns_the_empty_map_when_the_config_declares_no_speakers() {
    let model = load_synthetic_model("dengjen_melotts_synthetic_get_speakers_test");
    assert_eq!(model.get_speakers().unwrap(), Some(&HashMap::new()));
}

#[cfg(feature = "espeak")]
#[traced_test]
#[test]
fn phonemize_text_produces_phone_tone_pairs_joined_by_a_colon() {
    let model = load_synthetic_model("dengjen_melotts_synthetic_phonemize_text_test");
    let phonemes = match model.phonemize_text("hi") {
        Ok(phonemes) => phonemes,
        Err(dengjen_tts_core::DengjenError::PhonemizationError(msg))
            if msg.contains(dengjen_espeak_phonemizer::ESPEAKNG_INIT_FAILURE_MARKER) =>
        {
            eprintln!(
                "skipping phonemize_text_produces_phone_tone_pairs_joined_by_a_colon: \
                 espeak-ng data unavailable on this machine"
            );
            return;
        }
        Err(e) => panic!("phonemization against the espeak backend failed: {e}"),
    };
    assert_eq!(phonemes.num_sentences(), 1);
    for token in phonemes.sentences()[0].split('\n') {
        assert!(
            token.contains(':'),
            "expected every token to carry a `phone:tone` pair, got {token:?}"
        );
    }
    assert!(logs_contain("phonemize_text"));
}

#[traced_test]
#[test]
fn speak_batch_synthesizes_each_sentence_independently() {
    let model = load_synthetic_model("dengjen_melotts_synthetic_speak_batch_test");
    let audios = model
        .speak_batch(vec!["t:_".to_string(), "t:_".to_string()])
        .expect("batch synthesis against synthetic fixture failed");
    assert_eq!(audios.len(), 2);
    assert!(logs_contain("speak_batch"));
}
