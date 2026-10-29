pub(crate) fn plain_text(text: &str) -> String {
    if !text.trim_start().starts_with("<speak") {
        return text.to_owned();
    }
    decode_entities(&strip_tags(text))
}

fn strip_tags(text: &str) -> String {
    let mut in_tag = false;
    text.chars()
        .filter(|&c| match c {
            '<' => {
                in_tag = true;
                false
            }
            '>' if in_tag => {
                in_tag = false;
                false
            }
            _ => !in_tag,
        })
        .collect()
}

fn decode_entities(text: &str) -> String {
    [
        ("&lt;", "<"),
        ("&gt;", ">"),
        ("&quot;", "\""),
        ("&apos;", "'"),
    ]
    .iter()
    .fold(text.to_owned(), |decoded, (entity, plain)| {
        decoded.replace(entity, plain)
    })
    .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
