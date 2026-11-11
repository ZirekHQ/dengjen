//! Checks the built-in lexicon file, `lexicon/kmr.tsv`, the way a reviewer
//! would: it parses, and every entry makes an audible difference.

use dengjen_kurmanji_phonemizer::{
    parse_entries, text_to_kurmanji_phonemes, text_to_kurmanji_phonemes_with_lexicon, Lexicon,
};

#[test]
fn the_builtin_lexicon_has_no_errors() {
    let errors = parse_entries(Lexicon::builtin_source())
        .err()
        .unwrap_or_default()
        .iter()
        .map(|error| format!("lexicon/kmr.tsv {error}"))
        .collect::<Vec<_>>();
    assert!(errors.is_empty(), "{errors:#?}");
}

#[test]
fn every_entry_changes_how_its_word_is_said() {
    let rules_only = Lexicon::default();
    let unchanged = parse_entries(Lexicon::builtin_source())
        .unwrap()
        .into_iter()
        .filter(|entry| {
            text_to_kurmanji_phonemes(&entry.written).unwrap().to_vec()
                == text_to_kurmanji_phonemes_with_lexicon(&entry.written, &rules_only)
                    .unwrap()
                    .to_vec()
        })
        .map(|entry| {
            format!(
                "lexicon/kmr.tsv line {}: `{}` already sounds like that without the entry",
                entry.line, entry.written
            )
        })
        .collect::<Vec<_>>();
    assert!(unchanged.is_empty(), "{unchanged:#?}");
}
