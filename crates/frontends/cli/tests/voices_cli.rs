use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;

const KEY: &str = "xx_XX-test-low";
const MODEL: &[u8] = b"onnx-bytes";
const CONFIG: &[u8] = b"{}";

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn catalogue_json() -> String {
    let file = |bytes: &[u8]| {
        format!(
            r#"{{"size_bytes": {}, "sha256_digest": "{}"}}"#,
            bytes.len(),
            sha256_hex(bytes)
        )
    };
    format!(
        r#"{{"{KEY}": {{
            "key": "{KEY}", "name": "test", "quality": "low", "num_speakers": 1,
            "language": {{"code": "xx_XX", "family": "xx"}},
            "files": {{"xx/{KEY}.onnx": {}, "xx/{KEY}.onnx.json": {}}}
        }}}}"#,
        file(MODEL),
        file(CONFIG)
    )
}

fn routes() -> HashMap<String, Vec<u8>> {
    HashMap::from([
        ("/voices.json".to_owned(), catalogue_json().into_bytes()),
        (format!("/files/xx/{KEY}.onnx"), MODEL.to_vec()),
        (format!("/files/xx/{KEY}.onnx.json"), CONFIG.to_vec()),
    ])
}

fn request_path(stream: &mut TcpStream) -> String {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap_or(0) == 1 {
        head.push(byte[0]);
    }
    String::from_utf8_lossy(&head)
        .split_whitespace()
        .nth(1)
        .unwrap_or("/")
        .to_owned()
}

fn answer(mut stream: TcpStream, routes: &HashMap<String, Vec<u8>>) {
    let path = request_path(&mut stream);
    answer_known(stream, &path, routes);
}

fn answer_known(mut stream: TcpStream, path: &str, routes: &HashMap<String, Vec<u8>>) {
    let (status, body) = routes
        .get(path)
        .map_or(("404 Not Found", &[][..]), |body| ("200 OK", body));
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
}

fn answer_stalling(mut stream: TcpStream, routes: &HashMap<String, Vec<u8>>) {
    let path = request_path(&mut stream);
    if !path.ends_with(".onnx") {
        return answer_known(stream, &path, routes);
    }
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        MODEL.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&MODEL[..1]);
    let _ = stream.flush();
    std::thread::sleep(std::time::Duration::from_secs(60));
}

fn serve_stalling_fixture() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let routes = routes();
    std::thread::spawn(move || {
        listener.incoming().flatten().for_each(|stream| {
            let routes = routes.clone();
            std::thread::spawn(move || answer_stalling(stream, &routes));
        })
    });
    base
}

fn serve_fixture() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let routes = routes();
    std::thread::spawn(move || {
        listener
            .incoming()
            .flatten()
            .for_each(|stream| answer(stream, &routes))
    });
    base
}

fn dengjen(dir: &std::path::Path, base: &str) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_dengjen"));
    cmd.arg("voices")
        .env("DENGJEN_VOICES_DIR", dir)
        .env("DENGJEN_CATALOGUE_URL", format!("{base}/voices.json"))
        .env("DENGJEN_FILES_BASE_URL", format!("{base}/files"));
    cmd
}

#[test]
fn install_list_json_and_remove_round_trip() {
    let base = serve_fixture();
    let tmp = tempfile::tempdir().unwrap();

    let install = dengjen(tmp.path(), &base)
        .args(["install", KEY])
        .output()
        .unwrap();
    assert!(
        install.status.success(),
        "{}",
        String::from_utf8_lossy(&install.stderr)
    );
    assert!(install.stdout.is_empty());
    assert!(tmp
        .path()
        .join("piper")
        .join(KEY)
        .join(format!("{KEY}.onnx"))
        .is_file());

    let list = dengjen(tmp.path(), &base)
        .args(["list", "--installed", "--json"])
        .output()
        .unwrap();
    let rows: serde_json::Value = serde_json::from_slice(&list.stdout).unwrap();
    assert_eq!(rows[0]["key"], KEY);

    let remove = dengjen(tmp.path(), &base)
        .args(["remove", KEY])
        .output()
        .unwrap();
    assert!(remove.status.success());
    let again = dengjen(tmp.path(), &base)
        .args(["remove", KEY])
        .output()
        .unwrap();
    assert_eq!(again.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&again.stderr).contains("not installed"));
}

#[test]
fn unreachable_catalogue_fails_on_stderr_only() {
    let tmp = tempfile::tempdir().unwrap();
    let out = dengjen(tmp.path(), "http://127.0.0.1:9")
        .arg("list")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    assert!(!out.stderr.is_empty());
}

#[test]
fn bare_voices_lists_like_list() {
    let base = serve_fixture();
    let tmp = tempfile::tempdir().unwrap();
    let bare = dengjen(tmp.path(), &base).output().unwrap();
    let list = dengjen(tmp.path(), &base).arg("list").output().unwrap();
    assert!(bare.status.success() && list.status.success());
    assert!(String::from_utf8_lossy(&bare.stdout).contains(KEY));
    assert_eq!(bare.stdout, list.stdout);
}

#[test]
fn list_json_of_an_empty_filter_is_an_empty_array() {
    let base = serve_fixture();
    let tmp = tempfile::tempdir().unwrap();
    let out = dengjen(tmp.path(), &base)
        .args(["list", "--lang", "zz", "--json"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "[]");
}

#[test]
fn location_flags_are_accepted_after_the_subcommand() {
    let base = serve_fixture();
    let tmp = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_dengjen"))
        .args(["voices", "install", KEY, "--voices-dir"])
        .arg(tmp.path())
        .args(["--catalogue-url", &format!("{base}/voices.json")])
        .args(["--files-base-url", &format!("{base}/files")])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(tmp.path().join("piper").join(KEY).is_dir());
}

#[test]
fn list_installed_works_without_the_catalogue() {
    let base = serve_fixture();
    let tmp = tempfile::tempdir().unwrap();
    let install = dengjen(tmp.path(), &base)
        .args(["install", KEY])
        .output()
        .unwrap();
    assert!(install.status.success());

    let offline = dengjen(tmp.path(), "http://127.0.0.1:9")
        .args(["list", "--installed", "--json"])
        .output()
        .unwrap();
    assert!(
        offline.status.success(),
        "{}",
        String::from_utf8_lossy(&offline.stderr)
    );
    let rows: serde_json::Value = serde_json::from_slice(&offline.stdout).unwrap();
    assert_eq!(rows[0]["key"], KEY);
}

#[test]
fn speak_finds_a_voice_installed_under_an_explicit_voices_dir() {
    let base = serve_fixture();
    let tmp = tempfile::tempdir().unwrap();
    let install = dengjen(tmp.path(), &base)
        .args(["install", KEY])
        .output()
        .unwrap();
    assert!(install.status.success());

    let out = Command::new(env!("CARGO_BIN_EXE_dengjen"))
        .args(["speak", KEY, "--voices-dir"])
        .arg(tmp.path())
        .env_remove("DENGJEN_VOICES_DIR")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("is not installed") && !stderr.contains("unexpected argument"),
        "{stderr}"
    );
}

#[cfg(unix)]
#[test]
fn a_second_interrupt_force_exits_a_stalled_install() {
    let base = serve_stalling_fixture();
    let tmp = tempfile::tempdir().unwrap();
    let mut child = dengjen(tmp.path(), &base)
        .args(["install", KEY])
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let staging = tmp.path().join(".staging").join("piper").join(KEY);
    let started = std::time::Instant::now();
    while !staging.exists() && started.elapsed().as_secs() < 20 {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(staging.exists(), "install never reached the download");
    std::thread::sleep(std::time::Duration::from_millis(300));
    let interrupt = |pid: u32| {
        Command::new("kill")
            .args(["-INT", &pid.to_string()])
            .status()
            .unwrap();
    };
    interrupt(child.id());
    std::thread::sleep(std::time::Duration::from_millis(300));
    if child.try_wait().unwrap().is_none() {
        interrupt(child.id());
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "install still running after two interrupts"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    assert!(matches!(status.code(), Some(1 | 130)), "{status:?}");
}

#[test]
fn an_empty_listing_prints_nothing() {
    let base = serve_fixture();
    let tmp = tempfile::tempdir().unwrap();
    let out = dengjen(tmp.path(), &base)
        .args(["list", "--lang", "zz"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(out.stdout.is_empty(), "{:?}", out.stdout);
}

#[test]
fn list_marks_installed_voices_in_table_and_json() {
    let base = serve_fixture();
    let tmp = tempfile::tempdir().unwrap();
    let before = dengjen(tmp.path(), &base).arg("list").output().unwrap();
    assert!(!String::from_utf8_lossy(&before.stdout).contains("installed"));

    let install = dengjen(tmp.path(), &base)
        .args(["install", KEY])
        .output()
        .unwrap();
    assert!(install.status.success());

    let table = dengjen(tmp.path(), &base).arg("list").output().unwrap();
    assert!(String::from_utf8_lossy(&table.stdout).contains("installed"));
    let json = dengjen(tmp.path(), &base)
        .args(["list", "--json"])
        .output()
        .unwrap();
    let rows: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(rows[0]["installed"], true);
}

#[test]
fn small_voices_do_not_show_as_zero_mib() {
    let base = serve_fixture();
    let tmp = tempfile::tempdir().unwrap();
    let out = dengjen(tmp.path(), &base).arg("list").output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("KiB") && !stdout.contains("0 MiB"),
        "{stdout}"
    );
}

#[test]
fn redirected_install_progress_has_no_carriage_returns() {
    let base = serve_fixture();
    let tmp = tempfile::tempdir().unwrap();
    let out = dengjen(tmp.path(), &base)
        .args(["install", KEY])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(!out.stderr.contains(&b'\r'), "{:?}", out.stderr);
}

#[test]
fn refresh_reports_its_count_on_stdout() {
    let base = serve_fixture();
    let tmp = tempfile::tempdir().unwrap();
    let out = dengjen(tmp.path(), &base).arg("refresh").output().unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "1 voices");
    assert!(out.stderr.is_empty());
}

#[test]
fn speak_with_an_installed_voice_missing_its_model_names_the_file() {
    let base = serve_fixture();
    let tmp = tempfile::tempdir().unwrap();
    let install = dengjen(tmp.path(), &base)
        .args(["install", KEY])
        .output()
        .unwrap();
    assert!(install.status.success());
    std::fs::remove_file(
        tmp.path()
            .join("piper")
            .join(KEY)
            .join(format!("{KEY}.onnx")),
    )
    .unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_dengjen"))
        .args(["speak", KEY, "--voices-dir"])
        .arg(tmp.path())
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr.contains("missing") && stderr.contains(&format!("{KEY}.onnx")),
        "{stderr}"
    );
}

#[test]
fn an_unknown_key_is_reported_before_the_runtime_starts() {
    let tmp = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_dengjen"))
        .args(["speak", "typo", "--voices-dir"])
        .arg(tmp.path())
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("is not installed"), "{stderr}");
    assert!(
        !stderr.contains("AVX2") && !stderr.contains("onnxruntime"),
        "{stderr}"
    );
}
