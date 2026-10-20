use std::process::Command;

fn real_voice_config_path() -> Option<String> {
    std::env::var("DENGJEN_KOKORO_TEST_VOICE_CONFIG").ok()
}

#[test]
fn cli_synthesizes_from_a_real_kokoro_voice() {
    let Some(config_path) = real_voice_config_path() else {
        eprintln!(
            "Skipping: set DENGJEN_KOKORO_TEST_VOICE_CONFIG to a real Kokoro voice config to run this test"
        );
        return;
    };

    let dir = std::env::temp_dir().join("dengjen_cli_kokoro_e2e_test");
    std::fs::create_dir_all(&dir).expect("failed to create temp dir");
    let input_path = dir.join("input.txt");
    std::fs::write(&input_path, "Hello, world!").expect("failed to write input file");
    let output_path = dir.join("output.wav");

    let output = Command::new(env!("CARGO_BIN_EXE_dengjen"))
        .arg("speak")
        .arg(&config_path)
        .arg("-f")
        .arg(&input_path)
        .arg("-o")
        .arg(&output_path)
        .output()
        .expect("failed to spawn dengjen-cli");

    assert!(
        output.status.success(),
        "CLI exited with failure: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let wav_bytes = std::fs::read(&output_path).expect("expected CLI to write an output WAV file");
    assert!(
        !wav_bytes.is_empty(),
        "expected non-empty WAV bytes in output file"
    );

    std::fs::remove_dir_all(&dir).ok();
}
