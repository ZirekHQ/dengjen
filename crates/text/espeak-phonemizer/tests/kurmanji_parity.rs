//! Diffs `dengjen-kurmanji-phonemizer` against the eSpeak-ng `ku` voice through the same runtime
//! call Piper uses. The eSpeak-ng comparisons are `#[ignore]` until run with `--include-ignored`
//! against data from `scripts/stage-espeak-data.sh` and `DENGJEN_ESPEAKNG_DATA_DIRECTORY`.
use dengjen_espeak_phonemizer::text_to_phonemes;
use dengjen_kurmanji_phonemizer::{text_to_kurmanji_phonemes_with_lexicon, Lexicon};

const CORPUS: &str = include_str!("kurmanji_corpus.txt");
const MAX_REPORTED: usize = 100;
const SWEEP_MAX: u64 = 3000;

/// Inputs where the crate intentionally differs from eSpeak-ng: `Dr.` is read as *doktor*
/// (`kurmanji-phonemizer/tests/divergence.tsv`), and `s` plus a combining cedilla is read as `ş`, not `s`.
const INTENDED_DIVERGENCE: [&str; 2] = ["Dr.", "\u{327}"];

fn parse_corpus_column(raw: &str) -> usize {
    match raw.parse::<usize>() {
        Ok(column) if column >= 1 => column,
        _ => panic!("DENGJEN_KU_CORPUS_COLUMN is a 1-based column number, got {raw:?}"),
    }
}

fn corpus_column() -> Option<usize> {
    match std::env::var("DENGJEN_KU_CORPUS_COLUMN") {
        Ok(raw) => Some(parse_corpus_column(&raw)),
        Err(std::env::VarError::NotPresent) => None,
        Err(error) => panic!("DENGJEN_KU_CORPUS_COLUMN is unreadable: {error}"),
    }
}

fn corpus_lines(raw: &str, column: Option<usize>) -> Vec<&str> {
    let mut lines: Vec<&str> = match column {
        Some(index) => raw
            .lines()
            .skip(1)
            .filter_map(|row| row.split('\t').nth(index - 1))
            .collect(),
        None => raw.lines().collect(),
    };
    lines.retain(|line| !line.trim().is_empty());
    lines.sort_unstable();
    lines.dedup();
    lines
}

fn is_intended_divergence(line: &str) -> bool {
    INTENDED_DIVERGENCE
        .iter()
        .any(|marker| line.contains(marker))
}

fn is_speakable(sentence: &str) -> bool {
    sentence
        .chars()
        .any(|c| !matches!(c, ',' | '.' | '?' | '!' | ' '))
}

fn espeak(line: &str) -> String {
    text_to_phonemes(line, "ku", None, false, false)
        .unwrap_or_else(|error| panic!("espeak-ng failed on {line:?}: {error}"))
        .into_iter()
        // eSpeak-ng returns punctuation-only sentences for quote-only text; the crate returns none.
        .filter(|sentence| is_speakable(sentence))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The letter rules alone: lexicon entries are deliberate departures from eSpeak-ng.
fn ours(line: &str) -> String {
    text_to_kurmanji_phonemes_with_lexicon(line, &Lexicon::default())
        .unwrap_or_else(|error| panic!("crate failed on {line:?}: {error:?}"))
        .to_vec()
        .join(" ")
}

fn mismatches<'a>(lines: impl Iterator<Item = &'a str>) -> Vec<String> {
    lines
        .filter(|line| !is_intended_divergence(line))
        .filter_map(|line| {
            let (got, want) = (ours(line), espeak(line));
            (got != want).then(|| format!("{line}\n  got  {got}\n  want {want}"))
        })
        .collect()
}

fn assert_no_mismatches(diffs: &[String], allowed: usize) {
    let shown: Vec<_> = diffs.iter().take(MAX_REPORTED).collect();
    assert!(
        diffs.len() <= allowed,
        "{} mismatching lines (allowed {allowed}):\n{shown:#?}",
        diffs.len()
    );
}

#[test]
#[should_panic(expected = "1-based")]
fn column_zero_is_rejected() {
    parse_corpus_column("0");
}

#[test]
#[should_panic(expected = "1-based")]
fn a_non_numeric_column_is_rejected() {
    parse_corpus_column("2x");
}

#[test]
fn column_mode_skips_the_header_and_picks_the_column() {
    let raw = "id\ttext\n1\tb\n2\ta\n3\ta\n";
    assert_eq!(corpus_lines(raw, Some(2)), vec!["a", "b"]);
}

#[test]
#[ignore = "needs the data built from source: scripts/stage-espeak-data.sh, then --include-ignored"]
fn committed_corpus_matches_espeak() {
    let lines = corpus_lines(CORPUS, None);
    assert!(!lines.is_empty(), "the committed corpus is empty");
    assert_no_mismatches(&mismatches(lines.into_iter()), 0);
}

#[test]
#[ignore = "needs the data built from source: scripts/stage-espeak-data.sh, then --include-ignored"]
fn integers_match_espeak() {
    let numbers: Vec<String> = (0..=SWEEP_MAX).map(|n| n.to_string()).collect();
    assert_no_mismatches(&mismatches(numbers.iter().map(String::as_str)), 0);
}

#[test]
#[ignore = "needs DENGJEN_KU_CORPUS and the data built from source; a local check, not run in CI"]
fn external_corpus_matches_espeak() {
    let Ok(path) = std::env::var("DENGJEN_KU_CORPUS") else {
        eprintln!("skipping: DENGJEN_KU_CORPUS not set");
        return;
    };
    let raw = std::fs::read_to_string(path).unwrap();
    let lines = corpus_lines(&raw, corpus_column());
    assert!(
        !lines.is_empty(),
        "no corpus lines selected; check DENGJEN_KU_CORPUS_COLUMN"
    );
    let allowed: usize = std::env::var("DENGJEN_KU_MAX_MISMATCH")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    assert_no_mismatches(&mismatches(lines.into_iter()), allowed);
}
