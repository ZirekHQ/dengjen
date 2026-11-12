//! Plain text from the files a person opens to hear read aloud.

use std::fmt;

/// The kinds of file the apps open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    PlainText,
    Markdown,
    Html,
}

impl Format {
    /// The format for a file name extension, without the dot, in any case.
    pub fn from_extension(extension: &str) -> Option<Self> {
        match extension.to_ascii_lowercase().as_str() {
            "txt" | "text" => Some(Self::PlainText),
            "md" | "markdown" => Some(Self::Markdown),
            "htm" | "html" | "xhtml" => Some(Self::Html),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportError {
    /// The bytes are neither UTF-8 nor UTF-16 with a byte-order mark.
    NotText,
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the file is not UTF-8 or UTF-16 text")
    }
}

impl std::error::Error for ImportError {}

/// The text in a file's bytes, with Markdown and HTML markup removed so only
/// what a reader would see is spoken. Lines are kept, so headings and list
/// items stay separate sentences.
pub fn text_from_bytes(bytes: &[u8], format: Format) -> Result<String, ImportError> {
    let text = decode(bytes)?;
    Ok(match format {
        Format::PlainText => text.replace("\r\n", "\n"),
        Format::Markdown => markdown_to_text(&text),
        Format::Html => html_to_text(&text),
    })
}

fn decode(bytes: &[u8]) -> Result<String, ImportError> {
    match bytes {
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8(rest.to_vec()).ok(),
        [0xFF, 0xFE, rest @ ..] => utf16(rest, u16::from_le_bytes),
        [0xFE, 0xFF, rest @ ..] => utf16(rest, u16::from_be_bytes),
        _ => String::from_utf8(bytes.to_vec()).ok(),
    }
    .ok_or(ImportError::NotText)
}

fn utf16(bytes: &[u8], unit: fn([u8; 2]) -> u16) -> Option<String> {
    let (pairs, odd) = bytes.as_chunks::<2>();
    if !odd.is_empty() {
        return None;
    }
    String::from_utf16(&pairs.iter().map(|pair| unit(*pair)).collect::<Vec<_>>()).ok()
}

fn markdown_to_text(text: &str) -> String {
    text.lines()
        .filter(|line| !is_fence(line))
        .map(|line| inline_markdown(strip_block_marker(line.trim())))
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_fence(line: &str) -> bool {
    let line = line.trim_start();
    line.starts_with("```") || line.starts_with("~~~")
}

fn strip_block_marker(line: &str) -> &str {
    let line = line.trim_start_matches(['>', ' ']);
    let heading = line.trim_start_matches('#');
    if heading.len() < line.len() && heading.starts_with(' ') {
        return heading.trim_start();
    }
    for marker in ["- ", "* ", "+ "] {
        if let Some(rest) = line.strip_prefix(marker) {
            return rest;
        }
    }
    let digits = line.trim_start_matches(|c: char| c.is_ascii_digit());
    if digits.len() < line.len() {
        if let Some(rest) = digits
            .strip_prefix(". ")
            .or_else(|| digits.strip_prefix(") "))
        {
            return rest;
        }
    }
    line
}

// Keeps a link's or image's words and drops its address, and drops the
// emphasis and code markers.
fn inline_markdown(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(c) = rest.chars().next() {
        let after = rest.get(c.len_utf8()..).unwrap_or_default();
        if let Some((words, remainder)) = link(rest) {
            out.push_str(words);
            rest = remainder;
            continue;
        }
        if !matches!(c, '*' | '`') && !(c == '_' && after.starts_with('_')) {
            out.push(c);
        }
        rest = if c == '_' && after.starts_with('_') {
            after.get(1..).unwrap_or_default()
        } else {
            after
        };
    }
    out
}

fn link(text: &str) -> Option<(&str, &str)> {
    let open = text.strip_prefix("![").or_else(|| text.strip_prefix('['))?;
    let (words, after) = open.split_once("](")?;
    let (_, remainder) = after.split_once(')')?;
    (!words.contains(['[', ']'])).then_some((words, remainder))
}

const HIDDEN: [&str; 5] = ["script", "style", "head", "template", "noscript"];
const BLOCKS: [&str; 22] = [
    "p",
    "br",
    "div",
    "li",
    "ul",
    "ol",
    "tr",
    "td",
    "th",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "section",
    "article",
    "header",
    "footer",
    "blockquote",
    "pre",
    "hr",
];

fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(c) = rest.chars().next() {
        rest = if c == '<' {
            skip_tag(rest, &mut out)
        } else {
            out.push(if c.is_whitespace() { ' ' } else { c });
            rest.get(c.len_utf8()..).unwrap_or_default()
        };
    }
    tidy_lines(&decode_entities(&out))
}

fn skip_tag<'a>(rest: &'a str, out: &mut String) -> &'a str {
    if let Some(comment) = rest.strip_prefix("<!--") {
        return comment.split_once("-->").map_or("", |(_, after)| after);
    }
    let Some((tag, after)) = rest.get(1..).and_then(|tag| tag.split_once('>')) else {
        return "";
    };
    let closing = tag.starts_with('/');
    let name = tag
        .trim_start_matches('/')
        .split(|c: char| !c.is_ascii_alphanumeric())
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if BLOCKS.contains(&name.as_str()) {
        out.push('\n');
    }
    if !closing && !tag.ends_with('/') && HIDDEN.contains(&name.as_str()) {
        return skip_past_close(after, &name);
    }
    after
}

fn skip_past_close<'a>(text: &'a str, name: &str) -> &'a str {
    let needle = format!("</{name}");
    text.to_ascii_lowercase()
        .find(&needle)
        .and_then(|at| text.get(at..))
        .and_then(|close| close.split_once('>'))
        .map_or("", |(_, after)| after)
}

fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some((before, after)) = rest.split_once('&') {
        out.push_str(before);
        let decoded = after
            .split_once(';')
            .filter(|(name, _)| name.len() <= 8)
            .and_then(|(name, tail)| entity(name).map(|c| (c, tail)));
        rest = match decoded {
            Some((c, tail)) => {
                out.push(c);
                tail
            }
            None => {
                out.push('&');
                after
            }
        };
    }
    out.push_str(rest);
    out
}

fn entity(name: &str) -> Option<char> {
    match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some(' '),
        _ => {
            let number = name.strip_prefix('#')?;
            let code = match number.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => number.parse().ok()?,
            };
            char::from_u32(code)
        }
    }
}

fn tidy_lines(text: &str) -> String {
    text.lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_map_to_formats_in_any_case() {
        assert_eq!(Format::from_extension("TXT"), Some(Format::PlainText));
        assert_eq!(Format::from_extension("md"), Some(Format::Markdown));
        assert_eq!(Format::from_extension("Html"), Some(Format::Html));
        assert_eq!(Format::from_extension("pdf"), None);
    }

    #[test]
    fn plain_text_drops_a_utf8_bom_and_carriage_returns() {
        let bytes = b"\xEF\xBB\xBFRoj ba\xC5\x9F.\r\nSilav.";
        assert_eq!(
            text_from_bytes(bytes, Format::PlainText).unwrap(),
            "Roj baş.\nSilav."
        );
    }

    #[test]
    fn utf16_with_a_bom_is_decoded_in_either_byte_order() {
        let text = "Şev baş.";
        let le: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain(text.encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        let be: Vec<u8> = [0xFE, 0xFF]
            .into_iter()
            .chain(text.encode_utf16().flat_map(u16::to_be_bytes))
            .collect();
        assert_eq!(text_from_bytes(&le, Format::PlainText).unwrap(), text);
        assert_eq!(text_from_bytes(&be, Format::PlainText).unwrap(), text);
    }

    #[test]
    fn bytes_that_are_not_text_are_refused() {
        assert_eq!(
            text_from_bytes(b"\xFFnot utf8\xFE", Format::PlainText),
            Err(ImportError::NotText)
        );
        assert_eq!(
            text_from_bytes(b"\xFF\xFE\x41", Format::PlainText),
            Err(ImportError::NotText)
        );
    }

    #[test]
    fn markdown_keeps_the_words_and_drops_the_markup() {
        let md = "# Sernav\n\n> Gotin **giring** e.\n- Yek\n2. Du\n```\nkod\n```\n[Malper](https://zirek.org) û ![wêne](a.png) `kod`.";
        assert_eq!(
            text_from_bytes(md.as_bytes(), Format::Markdown).unwrap(),
            "Sernav\n\nGotin giring e.\nYek\nDu\nkod\nMalper û wêne kod."
        );
    }

    #[test]
    fn markdown_keeps_single_underscores_and_hash_words() {
        assert_eq!(
            markdown_to_text("#hashtag and snake_case __bold__"),
            "#hashtag and snake_case bold"
        );
    }

    #[test]
    fn html_keeps_visible_text_with_block_breaks() {
        let html = "<html><head><title>T</title><style>p{}</style></head><body>\n<h1>Sernav</h1><p>Roj\n  baş &amp; <b>xweş</b>&nbsp;e.</p><!-- nîşe --><script>x()</SCRIPT><p>&#350;ev &#x15F;ad e.</p><br/>Dawî</body></html>";
        assert_eq!(
            text_from_bytes(html.as_bytes(), Format::Html).unwrap(),
            "Sernav\nRoj baş & xweş e.\nŞev şad e.\nDawî"
        );
    }

    #[test]
    fn unknown_entities_and_unclosed_tags_are_left_alone_or_dropped() {
        assert_eq!(html_to_text("A &unknown; B &amp"), "A &unknown; B &amp");
        assert_eq!(html_to_text("Baş <b"), "Baş");
    }
}
