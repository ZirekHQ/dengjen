const SEPARATING_TAGS: [&str; 3] = ["s", "p", "break"];
const SENTENCE_ENDS: [char; 6] = ['.', '!', '?', '…', ':', ';'];
/// Text with marks but no sentence end, such as a long list, is still cut here so speech
/// starts without waiting for the whole message.
const MAX_UTTERANCE_CHARS: usize = 400;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Piece {
    Text(String),
    Mark(String),
}

/// Text synthesized in one go, with the marks inside it. Each mark sits at a character offset
/// into `text`; a mark at its end follows the audio.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Utterance {
    pub(crate) text: String,
    pub(crate) marks: Vec<(usize, String)>,
}

impl Utterance {
    fn trimmed(self) -> Self {
        let leading = self.text.chars().take_while(|c| c.is_whitespace()).count();
        let text = self.text.trim().to_owned();
        let len = text.chars().count();
        let marks = self
            .marks
            .into_iter()
            .map(|(at, name)| (at.saturating_sub(leading).min(len), name))
            .collect();
        Self { text, marks }
    }

    fn is_complete(&self) -> bool {
        self.text.trim_end().ends_with(SENTENCE_ENDS)
            || self.text.chars().count() >= MAX_UTTERANCE_CHARS
    }
}

/// Groups a message into sentences. Orca puts a mark after every word; synthesizing each word
/// on its own gives every word its own start-up delay and silence, so words in a sentence are
/// synthesized together and the marks are placed in the sentence's audio instead.
pub(crate) fn utterances(text: &str) -> Vec<Utterance> {
    if !is_ssml(text) {
        return vec![Utterance {
            text: text.to_owned(),
            marks: Vec::new(),
        }];
    }
    let mut out = Vec::new();
    let mut current = Utterance::default();
    for piece in split(text) {
        match piece {
            Piece::Mark(name) => {
                let at = current.text.trim_end().chars().count();
                current.marks.push((at, name));
            }
            Piece::Text(text) => {
                if current.is_complete() && !text.trim().is_empty() {
                    out.push(std::mem::take(&mut current).trimmed());
                }
                current.text.push_str(&text);
            }
        }
    }
    out.push(current.trimmed());
    out.retain(|utterance| !utterance.text.is_empty() || !utterance.marks.is_empty());
    out
}

#[cfg(test)]
pub(crate) fn pieces(text: &str) -> Vec<Piece> {
    if !is_ssml(text) {
        return vec![Piece::Text(text.to_owned())];
    }
    split(text)
        .into_iter()
        .filter_map(|piece| match piece {
            Piece::Text(text) => {
                let text = text.trim();
                (!text.is_empty()).then(|| Piece::Text(text.to_owned()))
            }
            mark => Some(mark),
        })
        .collect()
}

fn is_ssml(text: &str) -> bool {
    text.trim_start().starts_with("<speak")
}

/// Splits SSML into decoded text and marks, keeping the whitespace around each mark.
fn split(text: &str) -> Vec<Piece> {
    let mut parts = text.split('<');
    let mut current = parts.next().unwrap_or_default().to_owned();
    let mut out = Vec::new();
    for part in parts {
        let Some(end) = tag_end(part) else { continue };
        let (tag, rest) = part.split_at(end);
        match mark_name(tag) {
            Some(name) => {
                flush(&mut out, &mut current);
                out.push(Piece::Mark(name));
            }
            None if separates(tag) => current.push(' '),
            None => {}
        }
        current.push_str(rest);
    }
    flush(&mut out, &mut current);
    out
}

fn flush(out: &mut Vec<Piece>, current: &mut String) {
    let text = decode_entities(current);
    current.clear();
    if !text.is_empty() {
        out.push(Piece::Text(text));
    }
}

fn mark_name(tag: &str) -> Option<String> {
    let attributes = tag.strip_prefix("mark")?;
    attributes.starts_with(char::is_whitespace).then_some(())?;
    let value = attributes.split_once("name=")?.1;
    let quote = value.chars().next().filter(|c| matches!(c, '"' | '\''))?;
    let (name, _) = value.get(1..)?.split_once(quote)?;
    (!name.is_empty() && !name.contains(char::is_control)).then(|| name.to_owned())
}

fn tag_end(piece: &str) -> Option<usize> {
    let mut quote = None;
    piece
        .char_indices()
        .find_map(|(index, c)| match (quote, c) {
            (None, '>') => Some(index + 1),
            (None, '"' | '\'') => {
                quote = Some(c);
                None
            }
            (Some(open), _) if open == c => {
                quote = None;
                None
            }
            _ => None,
        })
}

fn separates(tag: &str) -> bool {
    let name = tag
        .trim_start_matches('/')
        .split(|c: char| c.is_whitespace() || c == '/' || c == '>')
        .next()
        .unwrap_or_default();
    SEPARATING_TAGS.contains(&name)
}

fn decode_entities(text: &str) -> String {
    let mut pieces = text.split('&');
    let head = pieces.next().unwrap_or_default();
    pieces.fold(head.to_owned(), |mut out, piece| {
        let decoded = piece
            .split_once(';')
            .and_then(|(name, rest)| entity(name).map(|c| (c, rest)));
        match decoded {
            Some((c, rest)) => {
                out.push(c);
                out.push_str(rest);
            }
            None => {
                out.push('&');
                out.push_str(piece);
            }
        }
        out
    })
}

fn entity(name: &str) -> Option<char> {
    match name {
        "lt" => Some('<'),
        "gt" => Some('>'),
        "amp" => Some('&'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => numeric(name.strip_prefix('#')?),
    }
}

fn numeric(digits: &str) -> Option<char> {
    let code = match digits.strip_prefix(['x', 'X']) {
        Some(hex) => u32::from_str_radix(hex, 16),
        None => digits.parse(),
    };
    char::from_u32(code.ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> Piece {
        Piece::Text(value.to_owned())
    }

    fn mark(value: &str) -> Piece {
        Piece::Mark(value.to_owned())
    }

    fn spoken(ssml: &str) -> String {
        pieces(ssml)
            .into_iter()
            .filter_map(|piece| match piece {
                Piece::Text(text) => Some(text),
                Piece::Mark(_) => None,
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn words(ssml: &str) -> String {
        spoken(ssml)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn plain_text_passes_through_untouched() {
        assert_eq!(pieces("a < b and c > d"), vec![text("a < b and c > d")]);
    }

    #[test]
    fn marks_split_the_text_in_order() {
        let ssml = r#"<speak>Silav.<mark name="__spd_0"/> Navê min Berfîn e.<mark name='__spd_1'/></speak>"#;
        assert_eq!(
            pieces(ssml),
            vec![
                text("Silav."),
                mark("__spd_0"),
                text("Navê min Berfîn e."),
                mark("__spd_1")
            ]
        );
    }

    #[test]
    fn malformed_marks_are_dropped() {
        let ssml = "<speak>a<mark/><mark name=\"\"/><mark name=x/><marker name=\"m\"/>b</speak>";
        assert_eq!(pieces(ssml), vec![text("ab")]);
    }

    #[test]
    fn escaped_markup_becomes_spoken_text_not_tags() {
        let ssml = "<speak>a &amp; b &lt;i&gt; &quot;c&quot; &apos;d&apos; &amp;lt;</speak>";
        assert_eq!(spoken(ssml), "a & b <i> \"c\" 'd' &lt;");
    }

    #[test]
    fn a_quoted_angle_bracket_does_not_end_the_tag() {
        let ssml = r#"<speak><sub alias="a > b">word</sub> <x y='>'/>end</speak>"#;
        assert_eq!(spoken(ssml), "word end");
    }

    #[test]
    fn sentence_paragraph_and_break_tags_keep_words_apart() {
        assert_eq!(
            words("<speak><s>Turn left</s><s>Now</s></speak>"),
            "Turn left Now"
        );
        assert_eq!(
            words("<speak><p>One</p><p>Two</p>Three<break/>Four</speak>"),
            "One Two Three Four"
        );
        assert_eq!(spoken("<speak>co<emphasis>ld</emphasis></speak>"), "cold");
    }

    #[test]
    fn numeric_references_decode_once() {
        let ssml = "<speak>caf&#233; &#xE9; &amp;#233; &#xD800; &#zz; &bogus;</speak>";
        assert_eq!(spoken(ssml), "café é &#233; &#xD800; &#zz; &bogus;");
    }

    fn utterance(text: &str, marks: &[(usize, &str)]) -> Utterance {
        Utterance {
            text: text.to_owned(),
            marks: marks
                .iter()
                .map(|(at, name)| (*at, (*name).to_owned()))
                .collect(),
        }
    }

    #[test]
    fn word_marks_stay_inside_one_sentence() {
        let ssml = r#"<speak><mark name="0:4"/>Navê <mark name="5:3"/>min <mark name="9:6"/>Berfîn e. <mark name="18:5"/>Silav<mark name="end"/></speak>"#;
        assert_eq!(
            utterances(ssml),
            vec![
                utterance(
                    "Navê min Berfîn e.",
                    &[(0, "0:4"), (4, "5:3"), (8, "9:6"), (18, "18:5")]
                ),
                utterance("Silav", &[(5, "end")]),
            ]
        );
    }

    #[test]
    fn a_mark_after_a_sentence_end_follows_that_sentence() {
        let ssml = r#"<speak>Silav.<mark name="a"/> Navê min Berfîn e.<mark name="b"/></speak>"#;
        assert_eq!(
            utterances(ssml),
            vec![
                utterance("Silav.", &[(6, "a")]),
                utterance("Navê min Berfîn e.", &[(18, "b")]),
            ]
        );
    }

    #[test]
    fn a_mark_inside_a_word_does_not_split_it() {
        assert_eq!(
            utterances(r#"<speak>co<mark name="m"/>ld</speak>"#),
            vec![utterance("cold", &[(2, "m")])]
        );
    }

    #[test]
    fn plain_text_is_one_utterance_without_marks() {
        assert_eq!(utterances(" a. b "), vec![utterance(" a. b ", &[])]);
    }

    #[test]
    fn long_text_without_a_sentence_end_is_cut() {
        let word = "peyv ";
        let ssml = format!(
            "<speak>{}</speak>",
            format!("{word}<mark name=\"m\"/>").repeat(MAX_UTTERANCE_CHARS / word.len() + 2)
        );
        assert!(utterances(&ssml).len() > 1);
    }

    #[test]
    fn an_unclosed_tag_is_dropped() {
        assert_eq!(spoken("<speak>a<b"), "a");
    }
}
