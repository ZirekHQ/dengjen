use std::ops::Range;

/// One sentence of the text, located both ways UI toolkits count: UTF-8 bytes
/// (Rust, GTK) and UTF-16 code units (WinUI, Android, AppKit).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sentence {
    pub bytes: Range<usize>,
    pub utf16: Range<usize>,
}

impl Sentence {
    /// The sentence's text, or `None` when `text` is not the string it was
    /// split from.
    pub fn text<'a>(&self, text: &'a str) -> Option<&'a str> {
        text.get(self.bytes.clone())
    }
}

/// Splits `text` into the sentences a read-aloud session speaks one at a
/// time. A line break always ends a sentence. `?` and `!` end one before a
/// space or the end of the line; `.` does too unless the next word starts
/// lowercase (an abbreviation such as "hwd. ji"). An ellipsis does not end a
/// sentence. Ranges leave out surrounding whitespace, and text with no letter
/// or digit (a line of dashes) is skipped.
pub fn sentences(text: &str) -> Vec<Sentence> {
    let byte_ranges: Vec<Range<usize>> = lines(text)
        .flat_map(|(offset, line)| {
            split_line(line)
                .into_iter()
                .map(move |range| range.start + offset..range.end + offset)
        })
        .filter_map(|range| trim(text, range))
        .collect();
    let mut cursor = Utf16Cursor::default();
    byte_ranges
        .into_iter()
        .map(|bytes| {
            let start = cursor.advance_to(text, bytes.start);
            let end = cursor.advance_to(text, bytes.end);
            Sentence {
                bytes,
                utf16: start..end,
            }
        })
        .collect()
}

/// The index of the sentence holding the UTF-16 offset `caret`, or of the
/// next sentence when the caret sits between two; `None` past the last one.
pub fn sentence_at(sentences: &[Sentence], caret: usize) -> Option<usize> {
    sentences
        .iter()
        .position(|sentence| caret < sentence.utf16.end)
}

fn lines(text: &str) -> impl Iterator<Item = (usize, &str)> {
    text.split('\n').scan(0, |offset, line| {
        let start = *offset;
        *offset += line.len() + 1;
        Some((start, line))
    })
}

fn split_line(line: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let (mut start, mut pos) = (0, 0);
    while let Some(c) = line.get(pos..).and_then(|rest| rest.chars().next()) {
        let after = line.get(pos + c.len_utf8()..).unwrap_or_default();
        pos = if c == '.' && after.starts_with("..") {
            pos + 3
        } else if ends_sentence(c, after) {
            let end = line.len() - after.trim_start_matches(is_trailer).len();
            ranges.push(start..end);
            start = end;
            end
        } else {
            pos + c.len_utf8()
        };
    }
    ranges.push(start..line.len());
    ranges
}

fn ends_sentence(c: char, after: &str) -> bool {
    match c {
        '?' | '!' => may_end(after.trim_start_matches(['.', '!', '?'])),
        '.' => may_end(after) && !next_word(after).starts_with(char::is_lowercase),
        _ => false,
    }
}

fn may_end(after: &str) -> bool {
    after
        .chars()
        .next()
        .is_none_or(|next| next.is_whitespace() || is_closer(next))
}

fn next_word(after: &str) -> &str {
    after.trim_start_matches(|c: char| c.is_whitespace() || is_closer(c))
}

fn is_trailer(c: char) -> bool {
    matches!(c, '.' | '!' | '?') || is_closer(c)
}

fn is_closer(c: char) -> bool {
    matches!(
        c,
        ')' | ']' | '}' | '"' | '\'' | '\u{201D}' | '\u{2019}' | '\u{BB}' | '\u{203A}'
    )
}

fn trim(text: &str, range: Range<usize>) -> Option<Range<usize>> {
    let slice = text.get(range.clone())?;
    let trimmed = slice.trim();
    if !trimmed.chars().any(char::is_alphanumeric) {
        return None;
    }
    let start = range.start + (slice.len() - slice.trim_start().len());
    Some(start..start + trimmed.len())
}

#[derive(Default)]
struct Utf16Cursor {
    byte: usize,
    units: usize,
}

impl Utf16Cursor {
    // Callers pass non-decreasing byte offsets, so the whole text is walked once.
    fn advance_to(&mut self, text: &str, byte: usize) -> usize {
        self.units += text
            .get(self.byte..byte)
            .map_or(0, |gap| gap.encode_utf16().count());
        self.byte = byte;
        self.units
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(text: &str) -> Vec<&str> {
        sentences(text)
            .iter()
            .map(|sentence| sentence.text(text).unwrap())
            .collect()
    }

    #[test]
    fn splits_on_full_stops_questions_and_exclamations() {
        assert_eq!(
            texts("Ez baş im. Tu çawa yî? Pir xweş e!"),
            ["Ez baş im.", "Tu çawa yî?", "Pir xweş e!"]
        );
    }

    #[test]
    fn a_line_break_ends_a_sentence_without_punctuation() {
        assert_eq!(
            texts("Sernav\r\nNivîs li vir e."),
            ["Sernav", "Nivîs li vir e."]
        );
    }

    #[test]
    fn a_full_stop_before_a_lowercase_word_does_not_end_the_sentence() {
        assert_eq!(
            texts("Ev hwd. tişt in. Paşê."),
            ["Ev hwd. tişt in.", "Paşê."]
        );
    }

    #[test]
    fn a_full_stop_inside_a_number_or_address_does_not_end_the_sentence() {
        assert_eq!(
            texts("Biha 3.50 e. Zirek.org baş e."),
            ["Biha 3.50 e.", "Zirek.org baş e."]
        );
    }

    #[test]
    fn an_ellipsis_does_not_end_the_sentence() {
        assert_eq!(
            texts("Belê... Ez dizanim. Erê… Baş e."),
            ["Belê... Ez dizanim.", "Erê… Baş e."]
        );
    }

    #[test]
    fn closing_quotes_and_repeated_marks_stay_with_their_sentence() {
        assert_eq!(
            texts("Wî got: \u{201C}Were!\u{201D} Çi?! (Na.) Baş."),
            ["Wî got: \u{201C}Were!\u{201D}", "Çi?!", "(Na.)", "Baş."]
        );
    }

    #[test]
    fn text_without_letters_or_digits_is_skipped() {
        assert_eq!(texts("  \n----\n...\nErê.  "), ["Erê."]);
        assert!(sentences("").is_empty());
    }

    #[test]
    fn utf16_ranges_count_surrogate_pairs_as_two_units() {
        let text = "Silav 👋. Baş e.";
        let found = sentences(text);
        assert_eq!(found[0].utf16, 0..9);
        assert_eq!(found[1].utf16, 10..16);
        let units: Vec<u16> = text.encode_utf16().collect();
        assert_eq!(String::from_utf16(&units[10..16]).unwrap(), "Baş e.");
    }

    #[test]
    fn sentence_at_finds_the_sentence_under_or_after_the_caret() {
        let found = sentences("Yek. Du. Sê.");
        assert_eq!(sentence_at(&found, 0), Some(0));
        assert_eq!(sentence_at(&found, 4), Some(1));
        assert_eq!(sentence_at(&found, 6), Some(1));
        assert_eq!(sentence_at(&found, 12), None);
    }
}
