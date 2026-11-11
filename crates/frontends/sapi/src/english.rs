//! English words that screen readers mix into what they send, respelled for a Kurmanji voice.
//!
//! Narrator and other SAPI clients speak their own UI in English (`Misspelled`, `selected`,
//! control types), even to a Kurmanji voice, which then reads them with Kurmanji letter rules.
//! Each word here is rewritten in Kurmanji spelling that sounds like its English pronunciation,
//! so the voice's own phonemizer produces something an English listener recognizes. Only whole
//! words match, ignoring case, and only words that are not also Kurmanji words are listed.
#![forbid(unsafe_code)]

use std::borrow::Cow;

/// English word (lowercase) and its Kurmanji respelling, sorted by the English word.
const RESPELLINGS: &[(&str, &str)] = &[
    ("button", "batin"),
    ("checkbox", "çekboks"),
    ("checked", "çekt"),
    ("clickable", "klîkabil"),
    ("collapsed", "kolapst"),
    ("column", "kolim"),
    ("combobox", "komboboks"),
    ("dialog", "dayalog"),
    ("editable", "edîtabil"),
    ("expanded", "ekspendid"),
    ("graphic", "grafîk"),
    ("heading", "heding"),
    ("level", "levil"),
    ("misspelled", "mîsspeld"),
    ("password", "paswird"),
    ("readonly", "rîdonlî"),
    ("selected", "silektid"),
    ("toolbar", "tûlbar"),
    ("unavailable", "aneveylibil"),
    ("unchecked", "ançekt"),
    ("unselected", "ansilektid"),
    ("window", "windo"),
];

/// Whether a voice for `language` (a code such as `ku` or `ku_TR`) gets English respelled.
pub fn applies_to(language: &str) -> bool {
    let primary = language.split(['_', '-']).next().unwrap_or_default();
    primary.eq_ignore_ascii_case("ku") || primary.eq_ignore_ascii_case("kmr")
}

fn respelling(word: &str) -> Option<&'static str> {
    let lower = word.to_ascii_lowercase();
    RESPELLINGS
        .binary_search_by(|(english, _)| (*english).cmp(lower.as_str()))
        .ok()
        .and_then(|at| RESPELLINGS.get(at))
        .map(|(_, kurmanji)| *kurmanji)
}

/// Text with the listed English words respelled, and where each original character ended up.
#[derive(Debug)]
pub struct Respelled<'a> {
    /// What the voice is given: the caller's text, borrowed unchanged when nothing was respelled.
    pub text: Cow<'a, str>,
    /// For each respelled word, the char index just past it in the original text and how many
    /// chars the respelling added (negative when it is shorter), in order.
    shifts: Vec<(usize, isize)>,
}

impl<'a> Respelled<'a> {
    /// `text` as it is, for a voice that gets no respelling.
    pub fn unchanged(text: &'a str) -> Self {
        Self {
            text: Cow::Borrowed(text),
            shifts: Vec::new(),
        }
    }

    /// Where the char at `original` (an index into the caller's text) is in [`Respelled::text`].
    ///
    /// Word timing uses it so a word's audio is estimated from its position in what was
    /// synthesized, while events keep pointing into the caller's text.
    pub fn char_index(&self, original: usize) -> usize {
        let shift: isize = self
            .shifts
            .iter()
            .take_while(|(end, _)| *end <= original)
            .map(|(_, shift)| shift)
            .sum();
        original.saturating_add_signed(shift)
    }

    /// How many chars [`Respelled::text`] has.
    pub fn char_count(&self) -> usize {
        self.text.chars().count()
    }
}

/// Whether `c` belongs to a word: letters, digits, `_` and the combining diacritical mark blocks,
/// so `button1` or `selected_name` is one token and never matches a listed word.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric()
        || c == '_'
        || matches!(
            c,
            '\u{300}'..='\u{36f}'
                | '\u{1ab0}'..='\u{1aff}'
                | '\u{1dc0}'..='\u{1dff}'
                | '\u{20d0}'..='\u{20ff}'
                | '\u{fe20}'..='\u{fe2f}'
        )
}

/// `text` with every listed English word respelled.
pub fn respell(text: &str) -> Respelled<'_> {
    let mut out = String::new();
    let mut copied = 0;
    let mut shifts = Vec::new();
    let mut rest = text.char_indices().enumerate().peekable();
    while let Some((start_char, (start, c))) = rest.next() {
        if !is_word_char(c) {
            continue;
        }
        let mut end = start + c.len_utf8();
        let mut end_char = start_char + 1;
        while let Some(&(at_char, (at, next))) = rest.peek() {
            if !is_word_char(next) {
                break;
            }
            end = at + next.len_utf8();
            end_char = at_char + 1;
            rest.next();
        }
        let word = text.get(start..end).unwrap_or_default();
        if let Some(kurmanji) = respelling(word) {
            out.push_str(text.get(copied..start).unwrap_or_default());
            out.push_str(kurmanji);
            copied = end;
            let added = kurmanji.chars().count() as isize - (end_char - start_char) as isize;
            shifts.push((end_char, added));
        }
    }
    let text = if shifts.is_empty() {
        Cow::Borrowed(text)
    } else {
        out.push_str(text.get(copied..).unwrap_or_default());
        Cow::Owned(out)
    };
    Respelled { text, shifts }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_sorted_lowercase_and_unique_so_lookups_find_every_word() {
        assert!(RESPELLINGS.windows(2).all(|pair| pair[0].0 < pair[1].0));
        assert!(RESPELLINGS
            .iter()
            .all(|(english, _)| english.chars().all(|c| c.is_ascii_lowercase())));
        assert!(RESPELLINGS
            .iter()
            .all(|(english, _)| respelling(english).is_some()));
    }

    #[test]
    fn narrators_markers_are_respelled_and_the_rest_is_kept() {
        assert_eq!(respell(",Misspelled , Rojbaş").text, ",mîsspeld , Rojbaş");
        assert_eq!(respell("Bişkok selected").text, "Bişkok silektid");
        assert_eq!(respell("SELECTED.").text, "silektid.");
    }

    #[test]
    fn only_whole_words_match() {
        assert_eq!(respell("buttons").text, "buttons");
        assert_eq!(respell("preselected").text, "preselected");
        assert_eq!(respell("button1").text, "button1");
        assert_eq!(respell("selected_name").text, "selected_name");
        assert_eq!(respell("checke\u{301}d").text, "checke\u{301}d");
        assert_eq!(respell("button\u{1ab0}").text, "button\u{1ab0}");
        assert_eq!(respell("button\u{20d7}").text, "button\u{20d7}");
    }

    #[test]
    fn text_without_english_is_borrowed_unchanged() {
        assert!(matches!(
            respell("Ez baş im, tu çawa yî?").text,
            Cow::Borrowed(_)
        ));
        assert!(matches!(respell("").text, Cow::Borrowed(_)));
    }

    #[test]
    fn char_indexes_move_by_what_the_respellings_before_them_added() {
        // "unavailable" (11 chars) becomes "aneveylibil" (11); "checked" (7) becomes "çekt" (4).
        let respelled = respell("checked a unavailable b");
        assert_eq!(respelled.text, "çekt a aneveylibil b");
        assert_eq!(respelled.char_index(0), 0);
        assert_eq!(respelled.char_index(8), 5);
        assert_eq!(respelled.char_index(10), 7);
        assert_eq!(respelled.char_index(22), 19);
        assert_eq!(respelled.char_count(), 20);
    }

    #[test]
    fn only_kurmanji_voices_respell() {
        assert!(applies_to("ku"));
        assert!(applies_to("ku_TR"));
        assert!(applies_to("kmr"));
        assert!(!applies_to("ckb"));
        assert!(!applies_to("en_US"));
        assert!(!applies_to(""));
    }
}
