use dengjen_kurmanji_phonemizer::text_to_kurmanji_phonemes;

const WORDS: &str = include_str!("golden_words.tsv");
const STRESS: &str = include_str!("golden_stress.tsv");

fn plain(phonemes: &str) -> String {
    phonemes
        .replace([',', '.', '?', '!'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn mismatches(fixture: &str) -> Vec<String> {
    fixture
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .filter_map(|(word, want)| {
            let got = plain(&text_to_kurmanji_phonemes(word).unwrap().to_vec().join(" "));
            (got != want).then(|| format!("{word}: got {got} want {want}"))
        })
        .collect()
}

#[test]
fn words_match_espeak_exactly() {
    let diffs = mismatches(WORDS);
    assert!(diffs.is_empty(), "{diffs:#?}");
}

const NUMBERS: &str = include_str!("golden_numbers.tsv");

#[test]
fn numbers_and_symbols_match_espeak_exactly() {
    let diffs = mismatches(NUMBERS);
    assert!(diffs.is_empty(), "{diffs:#?}");
}

#[test]
fn stress_matches_espeak_exactly() {
    let diffs = mismatches(STRESS);
    assert!(diffs.is_empty(), "{diffs:#?}");
}
