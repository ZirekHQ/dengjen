const SEPARATING_TAGS: [&str; 3] = ["s", "p", "break"];

pub(crate) fn plain_text(text: &str) -> String {
    if !text.trim_start().starts_with("<speak") {
        return text.to_owned();
    }
    decode_entities(&strip_tags(text)).trim().to_owned()
}

fn strip_tags(text: &str) -> String {
    let mut pieces = text.split('<');
    let head = pieces.next().unwrap_or_default();
    pieces.fold(head.to_owned(), |mut out, piece| {
        if let Some(end) = tag_end(piece) {
            let (tag, rest) = piece.split_at(end);
            if separates(tag) {
                out.push(' ');
            }
            out.push_str(rest);
        }
        out
    })
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

    fn words(text: &str) -> String {
        plain_text(text)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn plain_text_passes_through_untouched() {
        assert_eq!(plain_text("a < b and c > d"), "a < b and c > d");
    }

    #[test]
    fn ssml_loses_its_tags_and_marks() {
        let ssml = r#"<speak>Silav.<mark name="__spd_0"/> Navê min Berfîn e.<mark name="__spd_1"/></speak>"#;
        assert_eq!(plain_text(ssml), "Silav. Navê min Berfîn e.");
    }

    #[test]
    fn escaped_markup_becomes_spoken_text_not_tags() {
        let ssml = "<speak>a &amp; b &lt;i&gt; &quot;c&quot; &apos;d&apos; &amp;lt;</speak>";
        assert_eq!(plain_text(ssml), "a & b <i> \"c\" 'd' &lt;");
    }

    #[test]
    fn a_quoted_angle_bracket_does_not_end_the_tag() {
        let ssml = r#"<speak><sub alias="a > b">word</sub> <x y='>'/>end</speak>"#;
        assert_eq!(plain_text(ssml), "word end");
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
        assert_eq!(
            plain_text("<speak>co<emphasis>ld</emphasis></speak>"),
            "cold"
        );
    }

    #[test]
    fn numeric_references_decode_once() {
        let ssml = "<speak>caf&#233; &#xE9; &amp;#233; &#xD800; &#zz; &bogus;</speak>";
        assert_eq!(plain_text(ssml), "café é &#233; &#xD800; &#zz; &bogus;");
    }

    #[test]
    fn an_unclosed_tag_is_dropped() {
        assert_eq!(plain_text("<speak>a<b"), "a");
    }
}
