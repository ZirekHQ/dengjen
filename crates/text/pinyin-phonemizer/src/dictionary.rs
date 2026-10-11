use crate::g2pw::G2pwEngine;
use dengjen_tts_core::{DengjenError, DengjenResult};
use std::collections::HashMap;
use std::path::Path;

pub(crate) struct Dictionaries {
    pub monophonic: HashMap<char, String>,
    pub char_bopomofo: HashMap<char, String>,
    pub polyphonic_chars: Vec<(char, String)>,
}

fn read_error(path: &Path, cause: impl std::fmt::Display) -> DengjenError {
    DengjenError::PhonemizationError(format!(
        "Failed to read pinyin dictionary file {}: {cause}",
        path.display()
    ))
}

fn read_tab_separated_char_string_pairs(path: &Path) -> DengjenResult<Vec<(char, String)>> {
    let content = std::fs::read_to_string(path).map_err(|e| read_error(path, e))?;
    content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let mut parts = line.splitn(2, '\t');
            let char_part = parts.next().unwrap_or_default();
            let value_part = parts.next().unwrap_or_default();
            let c = char_part.chars().next().ok_or_else(|| {
                DengjenError::PhonemizationError(format!(
                    "Malformed line in {}: {line:?}",
                    path.display()
                ))
            })?;
            Ok((c, value_part.to_string()))
        })
        .collect()
}

pub(crate) fn load_dictionaries(
    monophonic_path: &Path,
    polyphonic_path: &Path,
    char_bopomofo_path: &Path,
) -> DengjenResult<Dictionaries> {
    let monophonic = read_tab_separated_char_string_pairs(monophonic_path)?
        .into_iter()
        .collect();
    let polyphonic_chars = read_tab_separated_char_string_pairs(polyphonic_path)?;

    let raw = std::fs::read_to_string(char_bopomofo_path)
        .map_err(|e| read_error(char_bopomofo_path, e))?;
    let parsed: HashMap<String, Vec<String>> =
        serde_json::from_str(&raw).map_err(|e| read_error(char_bopomofo_path, e))?;
    let char_bopomofo = parsed
        .into_iter()
        .filter_map(|(k, v)| {
            let c = k.chars().next()?;
            let first = v.into_iter().next()?;
            Some((c, first))
        })
        .collect();

    Ok(Dictionaries {
        monophonic,
        char_bopomofo,
        polyphonic_chars,
    })
}

pub(crate) fn get_phoneme_labels(
    polyphonic_chars: &[(char, String)],
) -> (Vec<String>, HashMap<char, Vec<usize>>) {
    let mut labels: Vec<String> = polyphonic_chars
        .iter()
        .map(|(_, phoneme)| phoneme.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    labels.sort();

    let mut char2phonemes: HashMap<char, Vec<usize>> = HashMap::new();
    for (c, phoneme) in polyphonic_chars {
        if let Some(idx) = labels.iter().position(|l| l == phoneme) {
            char2phonemes.entry(*c).or_default().push(idx);
        }
    }
    (labels, char2phonemes)
}

pub(crate) fn resolve_char_dictionary_only(dictionaries: &Dictionaries, c: char) -> Option<String> {
    dictionaries
        .monophonic
        .get(&c)
        .or_else(|| dictionaries.char_bopomofo.get(&c))
        .cloned()
}

pub(crate) fn resolve_char(
    dictionaries: &Dictionaries,
    g2pw: &G2pwEngine,
    chars: &[char],
    char_index: usize,
) -> DengjenResult<Option<String>> {
    let c = *chars.get(char_index).ok_or_else(|| {
        DengjenError::PhonemizationError(format!(
            "char_index {char_index} is out of bounds for a sentence of {} characters",
            chars.len()
        ))
    })?;

    if g2pw.is_polyphonic(c) {
        return g2pw.resolve_polyphonic(chars, char_index).map(Some);
    }

    Ok(resolve_char_dictionary_only(dictionaries, c))
}

#[cfg(test)]
mod tests {
    #[test]
    fn read_error_wraps_the_path_and_cause_with_context() {
        let err = super::read_error(std::path::Path::new("/tmp/does-not-exist.txt"), "boom");
        assert!(err.to_string().contains("does-not-exist.txt"));
        assert!(err.to_string().contains("boom"));
    }

    #[test]
    fn read_tab_separated_char_string_pairs_parses_lines_and_skips_blanks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pairs.txt");
        std::fs::write(&path, "行\tㄒㄧㄥˊ\n\n长\tㄔㄤˊ\n").unwrap();

        let pairs = super::read_tab_separated_char_string_pairs(&path).unwrap();

        assert_eq!(
            pairs,
            vec![('行', "ㄒㄧㄥˊ".to_string()), ('长', "ㄔㄤˊ".to_string()),]
        );
    }

    #[test]
    fn read_tab_separated_char_string_pairs_errors_on_a_missing_file() {
        let path = std::path::Path::new("/tmp/dengjen-nonexistent-dictionary.txt");
        let err = super::read_tab_separated_char_string_pairs(path).unwrap_err();
        assert!(err
            .to_string()
            .contains("dengjen-nonexistent-dictionary.txt"));
    }

    #[test]
    fn read_tab_separated_char_string_pairs_errors_on_a_line_with_no_leading_character() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pairs.txt");
        std::fs::write(&path, "\tㄒㄧㄥˊ\n").unwrap();

        let err = super::read_tab_separated_char_string_pairs(&path).unwrap_err();
        assert!(err.to_string().contains("Malformed line"));
    }

    #[test]
    fn load_dictionaries_reads_and_merges_all_three_source_files() {
        let dir = tempfile::tempdir().unwrap();
        let monophonic_path = dir.path().join("monophonic.txt");
        let polyphonic_path = dir.path().join("polyphonic.txt");
        let char_bopomofo_path = dir.path().join("char_bopomofo.json");

        std::fs::write(&monophonic_path, "你\tㄋㄧˇ\n").unwrap();
        std::fs::write(&polyphonic_path, "行\tㄒㄧㄥˊ\n行\tㄏㄤˊ\n").unwrap();
        std::fs::write(&char_bopomofo_path, r#"{"好": ["ㄏㄠˇ", "ㄏㄠˋ"]}"#).unwrap();

        let dictionaries =
            super::load_dictionaries(&monophonic_path, &polyphonic_path, &char_bopomofo_path)
                .unwrap();

        assert_eq!(dictionaries.monophonic.get(&'你').unwrap(), "ㄋㄧˇ");
        assert_eq!(dictionaries.char_bopomofo.get(&'好').unwrap(), "ㄏㄠˇ");
        assert_eq!(
            dictionaries.polyphonic_chars,
            vec![('行', "ㄒㄧㄥˊ".to_string()), ('行', "ㄏㄤˊ".to_string()),]
        );
    }

    #[test]
    fn load_dictionaries_errors_when_the_bopomofo_json_is_malformed() {
        let dir = tempfile::tempdir().unwrap();
        let monophonic_path = dir.path().join("monophonic.txt");
        let polyphonic_path = dir.path().join("polyphonic.txt");
        let char_bopomofo_path = dir.path().join("char_bopomofo.json");

        std::fs::write(&monophonic_path, "").unwrap();
        std::fs::write(&polyphonic_path, "").unwrap();
        std::fs::write(&char_bopomofo_path, "not json").unwrap();

        let err = super::load_dictionaries(&monophonic_path, &polyphonic_path, &char_bopomofo_path)
            .err()
            .unwrap();
        assert!(err.to_string().contains("char_bopomofo.json"));
    }

    #[test]
    fn get_phoneme_labels_sorts_labels_and_indexes_char_to_phonemes() {
        let polyphonic_chars = vec![
            ('行', "ㄒㄧㄥˊ".to_string()),
            ('行', "ㄏㄤˊ".to_string()),
            ('长', "ㄔㄤˊ".to_string()),
        ];
        let (labels, char2phonemes) = super::get_phoneme_labels(&polyphonic_chars);
        assert_eq!(labels.len(), 3);
        let xing_idx = labels.iter().position(|l| l == "ㄒㄧㄥˊ").unwrap();
        let hang_idx = labels.iter().position(|l| l == "ㄏㄤˊ").unwrap();
        let mut got = char2phonemes.get(&'行').cloned().unwrap();
        got.sort();
        let mut want = vec![xing_idx, hang_idx];
        want.sort();
        assert_eq!(got, want);
    }

    #[test]
    fn resolve_char_prefers_monophonic_over_char_bopomofo_when_both_present() {
        let dictionaries = super::Dictionaries {
            monophonic: [('好', "ㄏㄠˇ".to_string())].into_iter().collect(),
            char_bopomofo: [('好', "ㄏㄠˋ".to_string())].into_iter().collect(),
            polyphonic_chars: vec![],
        };
        let result = super::resolve_char_dictionary_only(&dictionaries, '好');
        assert_eq!(result, Some("ㄏㄠˇ".to_string()));
    }

    #[test]
    fn resolve_char_falls_back_to_char_bopomofo_when_not_monophonic() {
        let dictionaries = super::Dictionaries {
            monophonic: std::collections::HashMap::new(),
            char_bopomofo: [('你', "ㄋㄧˇ".to_string())].into_iter().collect(),
            polyphonic_chars: vec![],
        };
        let result = super::resolve_char_dictionary_only(&dictionaries, '你');
        assert_eq!(result, Some("ㄋㄧˇ".to_string()));
    }

    #[test]
    fn resolve_char_dictionary_only_returns_none_for_an_unresolved_character() {
        let dictionaries = super::Dictionaries {
            monophonic: std::collections::HashMap::new(),
            char_bopomofo: std::collections::HashMap::new(),
            polyphonic_chars: vec![],
        };
        assert_eq!(
            super::resolve_char_dictionary_only(&dictionaries, '。'),
            None
        );
    }
}
