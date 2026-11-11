//! A pronunciation lexicon: whole-word overrides that volunteers can edit as a
//! data file, without changing the phonemizer's rules.
//!
//! The built-in lexicon is `lexicon/kmr.tsv`, compiled into the crate so every
//! offline frontend gets it. Its format is in
//! `docs/modules/ROOT/pages/pronunciation-lexicon.adoc` at the repository root.

use std::collections::HashMap;
use std::fmt;
use std::sync::OnceLock;

use crate::normalize::normalize;
use crate::{letters, tokens};

const BUILTIN: &str = include_str!("../lexicon/kmr.tsv");
const HEADER: &str = "written\tsay\tnote";

/// How an entry says its word.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Say {
    /// Kurmanji words the phonemizer reads in place of the written form, with
    /// its usual rules and stress.
    Respelling(String),
    /// IPA used as is, written between slashes in the file.
    Phonemes(String),
}

/// One row of a lexicon file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The word as it is written in text.
    pub written: String,
    /// How to say it.
    pub say: Say,
    /// The 1-based line number in the file.
    pub line: usize,
}

/// A problem with one line of a lexicon file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LexiconError {
    /// The 1-based line number in the file.
    pub line: usize,
    /// What is wrong, in words a volunteer can act on.
    pub message: String,
}

impl fmt::Display for LexiconError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

/// Whole-word pronunciation overrides, looked up before the letter rules.
///
/// A written form with a capital letter matches only that exact spelling, so
/// an abbreviation such as `AB` does not change the word `ab`. An all-lowercase
/// written form matches the word in any case.
#[derive(Clone, Debug, Default)]
pub struct Lexicon {
    exact: HashMap<String, Say>,
    folded: HashMap<String, Say>,
}

fn has_capital(word: &str) -> bool {
    word.chars().any(char::is_uppercase)
}

fn is_respelling_char(c: char) -> bool {
    matches!(c, ' ' | '-' | '\'')
        || c.to_lowercase().all(|lower| {
            lower.is_ascii_lowercase() || matches!(lower, 'î' | 'ê' | 'û' | 'ç' | 'ş' | 'ö' | 'ü')
        })
}

fn is_phoneme_char(c: char) -> bool {
    matches!(c, ' ' | 'ˈ' | 'ˌ' | 'ʰ') || letters::phoneme_symbols().any(|symbol| symbol == c)
}

fn parse_say(raw: &str) -> Result<Say, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("the say column is empty".to_string());
    }
    match raw
        .strip_prefix('/')
        .and_then(|rest| rest.strip_suffix('/'))
    {
        Some(ipa) => {
            let ipa = ipa.trim();
            let unknown: String = ipa.chars().filter(|&c| !is_phoneme_char(c)).collect();
            if ipa.is_empty() {
                Err("the phonemes between the slashes are empty".to_string())
            } else if !unknown.is_empty() {
                Err(format!(
                    "these phonemes are not in the voice's phoneme set: {unknown}"
                ))
            } else {
                Ok(Say::Phonemes(ipa.to_string()))
            }
        }
        None if raw.starts_with('/') || raw.ends_with('/') => {
            Err("phonemes need a slash at both ends, like /sɪlˈav/".to_string())
        }
        None => {
            let unknown: String = raw.chars().filter(|&c| !is_respelling_char(c)).collect();
            if unknown.is_empty() {
                Ok(Say::Respelling(
                    raw.split_whitespace().collect::<Vec<_>>().join(" "),
                ))
            } else {
                Err(format!(
                    "a respelling uses only Kurmanji letters, spaces, hyphens and apostrophes, not: {unknown}"
                ))
            }
        }
    }
}

fn parse_written(raw: &str) -> Result<String, String> {
    let written = raw.trim();
    if written.is_empty() {
        Err("the written column is empty".to_string())
    } else if !written.chars().all(tokens::is_word_char) {
        Err(format!(
            "`{written}` must be a single word of letters, with no spaces, digits or punctuation"
        ))
    } else {
        Ok(written.to_string())
    }
}

fn parse_line(line: usize, text: &str) -> Result<Entry, LexiconError> {
    let error = |message: String| LexiconError { line, message };
    let mut columns = text.split('\t');
    let written = parse_written(columns.next().unwrap_or_default()).map_err(error)?;
    let say = parse_say(columns.next().unwrap_or_default()).map_err(error)?;
    // A third column is a free-text note; more than three is a mistake.
    if columns.nth(1).is_some() {
        return Err(error(
            "a line has at most three columns: written, say, note".to_string(),
        ));
    }
    Ok(Entry { written, say, line })
}

fn key(written: &str) -> (bool, String) {
    if has_capital(written) {
        (true, written.to_string())
    } else {
        (false, normalize(written))
    }
}

/// Parses a lexicon file: a header row `written<TAB>say<TAB>note`, then one
/// entry per line. Blank lines are skipped. Every problem is reported, not
/// only the first.
pub fn parse_entries(tsv: &str) -> Result<Vec<Entry>, Vec<LexiconError>> {
    let mut lines = tsv.lines().enumerate().map(|(i, text)| (i + 1, text));
    let header_ok = lines
        .next()
        .is_some_and(|(_, header)| header.trim_end() == HEADER);
    let mut errors = Vec::new();
    if !header_ok {
        errors.push(LexiconError {
            line: 1,
            message: "the first line must be the header: written, say, note (tab-separated)"
                .to_string(),
        });
    }
    let mut seen: HashMap<(bool, String), usize> = HashMap::new();
    let entries = lines.filter(|(_, text)| !text.trim().is_empty()).fold(
        Vec::new(),
        |mut entries, (line, text)| {
            match parse_line(line, text) {
                Err(error) => errors.push(error),
                Ok(entry) => match seen.insert(key(&entry.written), entry.line) {
                    Some(first) => errors.push(LexiconError {
                        line,
                        message: format!("`{}` is already on line {first}", entry.written),
                    }),
                    None => entries.push(entry),
                },
            }
            entries
        },
    );
    if errors.is_empty() {
        Ok(entries)
    } else {
        errors.sort_by_key(|error| error.line);
        Err(errors)
    }
}

impl Lexicon {
    /// Builds a lexicon from the text of a lexicon file.
    pub fn parse(tsv: &str) -> Result<Self, Vec<LexiconError>> {
        Ok(parse_entries(tsv)?.into_iter().collect())
    }

    /// The lexicon compiled into this crate from `lexicon/kmr.tsv`. CI checks
    /// that file, so it always parses; if it ever did not, this is empty.
    pub fn builtin() -> &'static Self {
        static BUILT: OnceLock<Lexicon> = OnceLock::new();
        BUILT.get_or_init(|| Self::parse(BUILTIN).unwrap_or_default())
    }

    /// The text of the built-in lexicon file.
    pub fn builtin_source() -> &'static str {
        BUILTIN
    }

    /// The number of entries.
    pub fn len(&self) -> usize {
        self.exact.len() + self.folded.len()
    }

    /// Whether the lexicon has no entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub(crate) fn lookup(&self, word: &str) -> Option<&Say> {
        if self.is_empty() {
            return None;
        }
        self.exact
            .get(word)
            .or_else(|| self.folded.get(&normalize(word)))
    }
}

impl FromIterator<Entry> for Lexicon {
    fn from_iter<I: IntoIterator<Item = Entry>>(entries: I) -> Self {
        entries
            .into_iter()
            .fold(Self::default(), |mut lexicon, entry| {
                match key(&entry.written) {
                    (true, exact) => lexicon.exact.insert(exact, entry.say),
                    (false, folded) => lexicon.folded.insert(folded, entry.say),
                };
                lexicon
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lexicon(rows: &str) -> Lexicon {
        Lexicon::parse(&format!("{HEADER}\n{rows}")).unwrap()
    }

    fn errors(rows: &str) -> Vec<String> {
        parse_entries(&format!("{HEADER}\n{rows}"))
            .unwrap_err()
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    #[test]
    fn a_header_alone_is_an_empty_lexicon() {
        assert!(lexicon("").is_empty());
    }

    #[test]
    fn respellings_phonemes_and_notes_parse() {
        let entries = parse_entries(&format!(
            "{HEADER}\nAB\tYekîtiya Ewropayê\tthe EU\n\nsilav\t/sɪlˈav/\n"
        ))
        .unwrap();
        assert_eq!(
            entries,
            vec![
                Entry {
                    written: "AB".into(),
                    say: Say::Respelling("Yekîtiya Ewropayê".into()),
                    line: 2,
                },
                Entry {
                    written: "silav".into(),
                    say: Say::Phonemes("sɪlˈav".into()),
                    line: 4,
                },
            ]
        );
    }

    #[test]
    fn capitalised_entries_match_exactly_and_lowercase_ones_in_any_case() {
        let lexicon = lexicon("AB\tYekîtiya Ewropayê\nkurmancî\t/kʊrmandʒˈi/");
        assert!(lexicon.lookup("AB").is_some());
        assert!(lexicon.lookup("ab").is_none());
        assert!(lexicon.lookup("Ab").is_none());
        assert!(lexicon.lookup("Kurmancî").is_some());
        assert!(lexicon.lookup("KURMANCÎ").is_some());
    }

    #[test]
    fn a_missing_header_is_reported() {
        let result = parse_entries("silav\tsilav\n");
        assert_eq!(result.unwrap_err()[0].line, 1);
    }

    #[test]
    fn duplicates_are_reported_with_the_first_line() {
        assert_eq!(
            errors("silav\tsilaw\nSilav\t/sɪlˈaw/\nsilav\tselav"),
            vec!["line 4: `silav` is already on line 2"]
        );
    }

    #[test]
    fn unknown_phonemes_are_reported() {
        assert_eq!(
            errors("silav\t/sɪlˈaθ/"),
            vec!["line 2: these phonemes are not in the voice's phoneme set: θ"]
        );
    }

    #[test]
    fn malformed_rows_are_all_reported() {
        let found =
            errors("\tsilav\ntwo words\tx\nsilav\t\nsilav\t/sɪl\nab\t3\nab\tav\tnote\textra");
        assert_eq!(found.len(), 6, "{found:#?}");
    }
}
