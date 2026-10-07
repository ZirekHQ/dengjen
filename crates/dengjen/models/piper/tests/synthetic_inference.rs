use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tracing_test::traced_test;

fn phoneme_id_map_json() -> &'static str {
    r#"{"^": [1], "$": [2], "_": [3], "t": [4], "ɛ": [5], "s": [6]}"#
}

fn synthetic_model_config_json(speaker_id_map: &str) -> String {
    format!(
        r#"{{
            "key": null,
            "language": {{"code": "en-US"}},
            "audio": {{"sample_rate": 22050, "quality": null}},
            "num_speakers": 1,
            "speaker_id_map": {speaker_id_map},
            "streaming": false,
            "espeak": {{"voice": "en-us"}},
            "inference": {{"noise_scale": 0.667, "length_scale": 1.0, "noise_w": 0.8}},
            "num_symbols": 8,
            "phoneme_map": {{}},
            "phoneme_id_map": {phoneme_map},
            "phoneme_type": "text",
            "hop_length": 256
        }}"#,
        phoneme_map = phoneme_id_map_json(),
        speaker_id_map = speaker_id_map,
    )
}

/// Loads a real (synthetic-fixture) `VitsModel` -- exercises the same config/session
/// wiring as `dengjen_tts_piper::from_config_path` without needing a real trained voice.
fn load_synthetic_model(dir_name: &str) -> Arc<dyn dengjen_tts_core::DengjenModel + Send + Sync> {
    load_synthetic_model_with_speaker_map(dir_name, r#"{"default": 0}"#)
}

fn load_synthetic_model_with_speaker_map(
    dir_name: &str,
    speaker_id_map: &str,
) -> Arc<dyn dengjen_tts_core::DengjenModel + Send + Sync> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_path = manifest_dir.join("tests/fixtures/synthetic_piper_batch.onnx");

    let dir = std::env::temp_dir().join(dir_name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(&fixture_path, dir.join("model.onnx")).unwrap();
    let config_path = dir.join("model.onnx.json");
    std::fs::write(&config_path, synthetic_model_config_json(speaker_id_map)).unwrap();

    let model = dengjen_tts_piper::from_config_path(&config_path)
        .expect("failed to load synthetic Piper model");
    std::fs::remove_dir_all(&dir).ok();
    model
}

#[traced_test]
#[test]
fn speak_one_sentence_synthesizes_against_the_synthetic_fixture() {
    let model = load_synthetic_model("dengjen_piper_synthetic_speak_one_sentence_test");
    let audio = model
        .speak_one_sentence("t".to_string())
        .expect("synthesis against synthetic fixture failed");
    assert_eq!(audio.info.sample_rate, 22050);
    assert!(!audio.samples.into_vec().is_empty());
    assert!(logs_contain("speak_one_sentence"));
}

#[traced_test]
#[test]
fn speak_batch_synthesizes_each_sentence_independently() {
    let model = load_synthetic_model("dengjen_piper_synthetic_speak_batch_test");
    let audios = model
        .speak_batch(vec!["t".to_string(), "s".to_string()])
        .expect("batch synthesis against synthetic fixture failed");
    assert_eq!(audios.len(), 2);
    assert!(logs_contain("speak_batch"));
}

#[traced_test]
#[test]
fn phonemize_text_passes_through_unchanged_for_the_text_phoneme_type() {
    let model = load_synthetic_model("dengjen_piper_synthetic_phonemize_text_test");
    let phonemes = model.phonemize_text("ts").unwrap();
    assert_eq!(phonemes.num_sentences(), 1);
    assert_eq!(phonemes.sentences()[0], "ts");
    assert!(logs_contain("phonemize_text"));
}

#[test]
fn audio_output_info_reflects_the_configured_sample_rate() {
    let model = load_synthetic_model("dengjen_piper_synthetic_audio_output_info_test");
    let info = model.audio_output_info().unwrap();
    assert_eq!(info.sample_rate, 22050);
}

#[test]
fn a_single_speaker_voice_reports_no_default_speaker() {
    let model = load_synthetic_model("dengjen_piper_synthetic_default_synth_config_test");
    let default = model
        .get_default_synthesis_config()
        .unwrap()
        .expect("Piper models always report a default synthesis config");
    assert_eq!(default.speaker, None);
}

#[test]
fn a_voice_with_an_empty_speaker_map_accepts_its_own_default_config_and_speaks() {
    let model = load_synthetic_model_with_speaker_map(
        "dengjen_piper_synthetic_empty_speaker_map_test",
        "{}",
    );
    let default = model.get_default_synthesis_config().unwrap().unwrap();
    model
        .set_fallback_synthesis_config(&default)
        .expect("a single-speaker voice must accept its own default config");
    assert!(model.speak_one_sentence("t".to_string()).is_ok());
}

#[test]
fn fallback_synthesis_config_starts_at_the_factory_default_then_updates_on_set() {
    let model = load_synthetic_model("dengjen_piper_synthetic_fallback_config_roundtrip_test");
    let initial = model.get_fallback_synthesis_config().unwrap().unwrap();
    assert_eq!(initial.speaker, None);

    let mut parameters = HashMap::new();
    parameters.insert("noise_scale".to_string(), 1.5f32);
    parameters.insert("length_scale".to_string(), 1.0f32);
    parameters.insert("noise_w".to_string(), 0.8f32);
    model
        .set_fallback_synthesis_config(&dengjen_tts_core::SynthesisConfig {
            speaker: Some(0),
            parameters,
        })
        .expect("failed to set fallback synthesis config");

    let updated = model.get_fallback_synthesis_config().unwrap().unwrap();
    assert_eq!(updated.parameters.get("noise_scale"), Some(&1.5));
}

#[test]
fn set_fallback_synthesis_config_rejects_an_unknown_speaker_id() {
    let model = load_synthetic_model("dengjen_piper_synthetic_unknown_speaker_test");
    let result = model.set_fallback_synthesis_config(&dengjen_tts_core::SynthesisConfig {
        speaker: Some(99),
        parameters: HashMap::new(),
    });
    assert!(result.is_err());
}

#[test]
fn get_speakers_and_speaker_name_to_id_reflect_the_configured_speaker_map() {
    let model = load_synthetic_model("dengjen_piper_synthetic_get_speakers_test");
    let speakers = model.get_speakers().unwrap().unwrap();
    assert_eq!(speakers.get(&0), Some(&"default".to_string()));
    assert_eq!(model.speaker_name_to_id("default").unwrap(), Some(0));
    assert_eq!(model.speaker_name_to_id("nobody").unwrap(), None);
}

#[test]
fn get_language_reads_the_configured_language_code() {
    let model = load_synthetic_model("dengjen_piper_synthetic_get_language_test");
    assert_eq!(model.get_language().unwrap(), Some("en-US".to_string()));
}

#[test]
fn properties_reports_an_unknown_quality_when_the_config_omits_it() {
    let model = load_synthetic_model("dengjen_piper_synthetic_properties_test");
    let properties = model.properties().unwrap();
    assert_eq!(properties.get("quality"), Some(&"unknown".to_string()));
}

#[traced_test]
#[test]
fn from_config_path_logs_the_load_error_via_tracing() {
    let dir = std::env::temp_dir().join("dengjen_piper_missing_model_path_test");
    std::fs::create_dir_all(&dir).unwrap();
    let config_path = dir.join("does_not_exist.json");

    let result = dengjen_tts_piper::from_config_path(&config_path);
    std::fs::remove_dir_all(&dir).ok();

    assert!(result.is_err(), "loading a missing config file should fail");
    assert!(logs_contain("from_config_path"));
}
