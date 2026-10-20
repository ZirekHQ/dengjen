use std::path::{Path, PathBuf};
use std::process::Command;

const WAV_HEADER_LEN: usize = 44;

fn melotts_fixture_model() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../dengjen/models/melotts/tests/fixtures/synthetic_melotts.onnx")
}

fn write_config(dir: &Path) {
    std::fs::write(
        dir.join("config.json"),
        r#"{
            "model_type": "melotts",
            "audio": {"sample_rate": 24000},
            "phonemizer": {"type": "espeak", "voice": "en-us"},
            "phone_id_map": {"^": [1], "$": [2], "_": [3], "t": [4]},
            "tone_id_map": {"_": 0},
            "inference": {"noise_scale": 0.667, "length_scale": 1.0, "noise_scale_w": 0.8},
            "model_path": "model.onnx"
        }"#,
    )
    .unwrap();
}

#[test]
fn cli_loads_a_melotts_voice_without_panicking() {
    let dir = std::env::temp_dir().join(format!(
        "dengjen_cli_melotts_synthetic_test_{}",
        std::process::id()
    ));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    let model_path = melotts_fixture_model();
    assert!(
        model_path.exists(),
        "missing fixture at {}",
        model_path.display()
    );
    std::fs::copy(&model_path, dir.join("model.onnx")).unwrap();
    write_config(&dir);
    let input_path = dir.join("input.txt");
    std::fs::write(&input_path, "Test.").unwrap();
    let output_path = dir.join("output.wav");

    let output = Command::new(env!("CARGO_BIN_EXE_dengjen"))
        .arg("speak")
        .arg(dir.join("config.json"))
        .arg("-f")
        .arg(&input_path)
        .arg("-o")
        .arg(&output_path)
        .output()
        .expect("failed to spawn the dengjen binary");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    assert!(
        !stderr.contains("panicked"),
        "CLI panicked on a MeloTTS voice: {stderr}"
    );

    if !output.status.success() {
        assert!(
            stderr.contains(dengjen_espeak_phonemizer::ESPEAKNG_INIT_FAILURE_MARKER),
            "CLI failed for an unexpected reason: {stderr}"
        );
        eprintln!(
            "Loaded the MeloTTS voice without panicking, but skipping the audio assertions: no espeak-ng data available. Set DENGJEN_ESPEAKNG_DATA_DIRECTORY to the directory containing `espeak-ng-data`."
        );
        std::fs::remove_dir_all(&dir).ok();
        return;
    }

    let wav_bytes = std::fs::read(&output_path).expect("expected the CLI to write an output WAV");
    assert!(
        wav_bytes.len() > WAV_HEADER_LEN,
        "expected WAV bytes beyond the header, got {}",
        wav_bytes.len()
    );
    assert_eq!(&wav_bytes[..4], b"RIFF");
    assert_eq!(&wav_bytes[8..12], b"WAVE");

    std::fs::remove_dir_all(&dir).ok();
}
