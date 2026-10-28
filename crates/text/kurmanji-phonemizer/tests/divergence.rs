//! Each row of divergence.tsv pairs an input with the spelling it must phonemize identically to.
use dengjen_kurmanji_phonemizer::text_to_kurmanji_phonemes;

const SAME_AS_SPELLING: &str = include_str!("divergence.tsv");

fn phonemes(text: &str) -> Vec<String> {
    text_to_kurmanji_phonemes(text).unwrap().to_vec()
}

#[test]
fn each_input_sounds_like_its_spelling() {
    let diffs: Vec<String> = SAME_AS_SPELLING
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .filter(|(input, spelled)| phonemes(input) != phonemes(spelled))
        .map(|(input, spelled)| format!("{input}: expected to sound like {spelled}"))
        .collect();
    assert!(diffs.is_empty(), "{diffs:#?}");
}
