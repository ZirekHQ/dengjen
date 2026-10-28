use dengjen_kurmanji_phonemizer::text_to_kurmanji_phonemes;
use std::process::Command;

const MAX_REPORTED: usize = 20;

fn espeak_ipa(line: &str, data_path: Option<&str>) -> Option<String> {
    let mut command = Command::new("espeak-ng");
    command.args(["-v", "ku", "--ipa", "-q"]);
    if let Some(path) = data_path {
        command.arg(format!("--path={path}"));
    }
    let output = command.arg(line).output().ok()?;
    output.status.success().then(|| {
        String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    })
}

fn ours(line: &str) -> String {
    text_to_kurmanji_phonemes(line)
        .unwrap()
        .to_vec()
        .join(" ")
        .replace([',', '.', '?', '!'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

const SENTINEL: &str = "qq";
const SWEEP_CHUNK: usize = 50;

fn chunk_diffs(numbers: &[String], data_path: Option<&str>) -> Vec<String> {
    let text = numbers.join(&format!(" {SENTINEL} "));
    let oracle = espeak_ipa(&text, data_path).unwrap_or_else(|| panic!("espeak-ng failed"));
    let marker = ours(SENTINEL);
    let wants: Vec<String> = oracle
        .split(marker.as_str())
        .map(|w| w.trim().to_string())
        .collect();
    let wants = if wants.len() == numbers.len() {
        wants
    } else {
        numbers
            .iter()
            .map(|n| espeak_ipa(n, data_path).unwrap_or_default())
            .collect()
    };
    numbers
        .iter()
        .zip(wants)
        .filter_map(|(number, batch_want)| {
            let got = ours(number);
            (got != batch_want).then_some(())?;
            let want = espeak_ipa(number, data_path).unwrap_or_default();
            (got != want).then(|| format!("{number}: got {got} want {want}"))
        })
        .collect()
}

#[test]
fn parity_with_espeak_on_integer_sweep() {
    let Some(max) = std::env::var("DENGJEN_KU_SWEEP_MAX")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
    else {
        eprintln!("skipping: DENGJEN_KU_SWEEP_MAX not set");
        return;
    };
    let data_path = std::env::var("DENGJEN_KU_ESPEAK_PATH").ok();
    let numbers: Vec<String> = (1..=max).map(|n| n.to_string()).collect();
    let diffs: Vec<String> = numbers
        .chunks(SWEEP_CHUNK)
        .flat_map(|chunk| chunk_diffs(chunk, data_path.as_deref()))
        .collect();
    let shown: Vec<_> = diffs.iter().take(MAX_REPORTED).collect();
    assert!(
        diffs.is_empty(),
        "{} mismatching numbers:\n{shown:#?}",
        diffs.len()
    );
}

#[test]
fn parity_with_espeak_on_corpus() {
    let Ok(corpus_path) = std::env::var("DENGJEN_KU_CORPUS") else {
        eprintln!("skipping: DENGJEN_KU_CORPUS not set");
        return;
    };
    let data_path = std::env::var("DENGJEN_KU_ESPEAK_PATH").ok();
    let corpus = std::fs::read_to_string(corpus_path).unwrap();
    let diffs: Vec<String> = corpus
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let want = espeak_ipa(line, data_path.as_deref())
                .unwrap_or_else(|| panic!("espeak-ng failed on {line:?}"));
            let got = ours(line);
            (got != want).then(|| format!("{line}\n  got  {got}\n  want {want}"))
        })
        .collect();
    let allowed: usize = std::env::var("DENGJEN_KU_MAX_MISMATCH")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let shown: Vec<_> = diffs.iter().take(MAX_REPORTED).collect();
    assert!(
        diffs.len() <= allowed,
        "{} mismatching lines (allowed {allowed}):\n{shown:#?}",
        diffs.len()
    );
}
