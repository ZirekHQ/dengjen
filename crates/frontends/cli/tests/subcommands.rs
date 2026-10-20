use std::process::Command;

fn dengjen() -> Command {
    Command::new(env!("CARGO_BIN_EXE_dengjen"))
}

#[test]
fn bare_config_path_is_rejected_with_a_speak_hint() {
    let output = dengjen().arg("model.onnx.json").output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("tip:") && stderr.contains("speak"),
        "{stderr}"
    );
}

#[test]
fn help_lists_speak_and_voices() {
    let output = dengjen().arg("--help").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("speak") && stdout.contains("voices"),
        "{stdout}"
    );
}

#[test]
fn a_mistyped_subcommand_gets_no_synthesis_tip() {
    let output = dengjen().arg("voice").output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("tip: synthesis moved"), "{stderr}");
}
