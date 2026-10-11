use std::path::{Path, PathBuf};
use std::process::Command;

const STYLE_DIM: usize = 256;
const MAX_TOKEN_LEN: usize = 510;
const WAV_HEADER_BYTES: usize = 44;

fn write_synthetic_voice_file(voices_dir: &Path, voice_name: &str) {
    let mut bytes = Vec::with_capacity(MAX_TOKEN_LEN * STYLE_DIM * 4);
    for row in 0..MAX_TOKEN_LEN {
        for _ in 0..STYLE_DIM {
            bytes.extend_from_slice(&(row as f32).to_le_bytes());
        }
    }
    std::fs::write(voices_dir.join(format!("{voice_name}.bin")), &bytes).unwrap();
}

fn kokoro_fixture_model() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../dengjen/models/kokoro/tests/fixtures/synthetic_kokoro.onnx")
}

#[test]
fn cli_synthesizes_a_wav_from_a_synthetic_kokoro_voice_when_espeak_data_is_available() {
    let dir = std::env::temp_dir().join(format!(
        "dengjen_cli_kokoro_synthetic_test_{}",
        std::process::id()
    ));
    std::fs::remove_dir_all(&dir).ok();
    let voices_dir = dir.join("voices");
    std::fs::create_dir_all(&voices_dir).unwrap();
    write_synthetic_voice_file(&voices_dir, "af_test");
    std::fs::write(
        dir.join("tokenizer.json"),
        r#"{"model": {"vocab": {"$": 0, "t": 1, "ɛ": 2, "s": 3}}}"#,
    )
    .unwrap();
    let model_path = kokoro_fixture_model();
    assert!(
        model_path.exists(),
        "missing fixture at {}",
        model_path.display()
    );
    std::fs::write(
        dir.join("config.json"),
        format!(
            r#"{{
                "model_type": "kokoro",
                "model_path": {model_path:?},
                "voices_dir": "voices",
                "vocab_path": "tokenizer.json",
                "sample_rate": 24000,
                "voices": ["af_test"]
            }}"#
        ),
    )
    .unwrap();
    let input_path = dir.join("input.txt");
    std::fs::write(&input_path, "Test.").unwrap();
    let output_path = dir.join("output.wav");

    let output = Command::new(env!("CARGO_BIN_EXE_dengjen"))
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
        "CLI panicked on a Kokoro voice: {stderr}"
    );

    if !output.status.success() {
        assert!(
            stderr.contains(dengjen_espeak_phonemizer::ESPEAKNG_INIT_FAILURE_MARKER),
            "CLI failed for an unexpected reason: {stderr}"
        );
        eprintln!(
            "Loaded the Kokoro voice without panicking, but skipping the audio assertions: no espeak-ng data available. Set DENGJEN_ESPEAKNG_DATA_DIRECTORY to the directory containing `espeak-ng-data`."
        );
        std::fs::remove_dir_all(&dir).ok();
        return;
    }

    let wav_bytes = std::fs::read(&output_path).expect("expected the CLI to write an output WAV");
    assert!(
        wav_bytes.len() > WAV_HEADER_BYTES,
        "expected the WAV to carry samples beyond its {WAV_HEADER_BYTES}-byte header, got {} bytes",
        wav_bytes.len()
    );
    assert_eq!(&wav_bytes[..4], b"RIFF");
    assert_eq!(&wav_bytes[8..12], b"WAVE");

    std::fs::remove_dir_all(&dir).ok();
}
