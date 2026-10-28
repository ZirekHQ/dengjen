use std::borrow::Cow;

use unicode_normalization::char::is_combining_mark;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Break {
    Comma,
    Colon,
    FullStop,
    Question,
    Exclamation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Token<'a> {
    Word(&'a str),
    Number {
        integer: Cow<'a, str>,
        fraction: Option<&'a str>,
    },
    Time {
        hour: usize,
        minute: usize,
    },
    Date {
        day: usize,
        month: usize,
        year: usize,
    },
    Ordinal {
        value: usize,
        formal: bool,
    },
    Currency(Currency),
    Percent,
    Dollar,
    Slash,
    Minus,
    Boundary(Break),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Currency {
    Euro,
    Pound,
    Lira,
}

impl Currency {
    pub(crate) fn word(self) -> &'static str {
        match self {
            Currency::Euro => "euro",
            Currency::Pound => "sterlîn",
            Currency::Lira => "lîre",
        }
    }
}

pub(crate) fn is_word_char(c: char) -> bool {
    c.is_alphabetic() || is_combining_mark(c)
}

fn currency_symbol(c: char) -> Option<Currency> {
    match c {
        '€' => Some(Currency::Euro),
        '£' => Some(Currency::Pound),
        '₺' => Some(Currency::Lira),
        _ => None,
    }
}

fn is_token_start(c: char) -> bool {
    is_word_char(c)
        || c.is_ascii_digit()
        || matches!(c, '%' | '$' | '/')
        || currency_symbol(c).is_some()
}

fn word_run(text: &str) -> (&str, &str) {
    let span_len = text
        .find(|c| !is_word_char(c) && c != '-')
        .unwrap_or(text.len());
    let chars: Vec<(usize, char)> = text
        .get(..span_len)
        .unwrap_or_default()
        .char_indices()
        .collect();
    let is_word_at = |k: Option<usize>| {
        k.and_then(|k| chars.get(k))
            .is_some_and(|(_, c)| is_word_char(*c))
    };
    let belongs = |k: usize| match chars.get(k) {
        Some((_, '-')) => is_word_at(k.checked_sub(1)) && is_word_at(Some(k + 1)),
        _ => is_word_at(Some(k)),
    };
    let kept = (0..chars.len()).take_while(|k| belongs(*k)).count();
    text.split_at(chars.get(kept).map_or(span_len, |(i, _)| *i))
}

pub(crate) fn split_camel_case(word: &str) -> Vec<&str> {
    let cuts: Vec<usize> = word
        .char_indices()
        .zip(word.char_indices().skip(1))
        .filter(|((_, before), (_, after))| before.is_lowercase() && after.is_uppercase())
        .map(|(_, (cut, _))| cut)
        .collect();
    std::iter::once(0)
        .chain(cuts.iter().copied())
        .zip(cuts.iter().copied().chain(std::iter::once(word.len())))
        .filter_map(|(start, end)| word.get(start..end))
        .collect()
}

fn ends_sentence(after_period: &str) -> bool {
    let trimmed = after_period.trim_start();
    trimmed.is_empty()
        || (trimmed.len() < after_period.len() && trimmed.starts_with(char::is_uppercase))
}

fn break_for(c: char, after: &str) -> Option<Break> {
    let before_space_or_end = !after.starts_with(|n: char| !n.is_whitespace());
    match c {
        '?' => Some(Break::Question),
        '!' => Some(Break::Exclamation),
        ',' | ';' if before_space_or_end => Some(Break::Comma),
        ':' if before_space_or_end => Some(Break::Colon),
        '.' if ends_sentence(after) => Some(Break::FullStop),
        _ => None,
    }
}

fn after_first_char(text: &str) -> &str {
    text.chars()
        .next()
        .and_then(|c| text.get(c.len_utf8()..))
        .unwrap_or_default()
}

fn is_minus(c: char, before: Option<char>, after: &str) -> bool {
    let starts_with_digit = |s: &str| s.starts_with(|n: char| n.is_ascii_digit());
    let digit_follows = starts_with_digit(after)
        || after.chars().next().is_some_and(|n| {
            currency_symbol(n).is_some() && starts_with_digit(after_first_char(after))
        });
    let opens = before.is_none_or(|p| {
        p.is_whitespace()
            || currency_symbol(p).is_some()
            || matches!(p, '(' | '[' | ',' | ';' | ':' | '"' | '“')
    });
    digit_follows && (c == '−' || (c == '-' && opens))
}

fn skip_ignorable(text: &str, before: Option<char>) -> (&str, Option<char>) {
    std::iter::successors(Some((text, before)), |&(rest, prev)| {
        let c = rest.chars().next()?;
        let after = after_first_char(rest);
        let stop = is_token_start(c) || break_for(c, after).is_some() || is_minus(c, prev, after);
        (!stop).then_some((after, Some(c)))
    })
    .last()
    .unwrap_or((text, before))
}

fn next_token(
    text: &str,
    before: Option<char>,
    after_digit_colon: bool,
) -> Option<(Token<'_>, &str)> {
    let (text, before) = skip_ignorable(text, before);
    let first = text.chars().next()?;
    let after = after_first_char(text);
    if let Some(kind) = break_for(first, after) {
        return Some((Token::Boundary(kind), after));
    }
    if let Some(currency) = currency_symbol(first) {
        return Some((Token::Currency(currency), after));
    }
    Some(match first {
        '-' | '−' if is_minus(first, before, after) => (Token::Minus, after),
        '%' => (Token::Percent, after),
        '$' => (Token::Dollar, after),
        '/' => (Token::Slash, after),
        c if c.is_ascii_digit() => crate::numeric::structured(text)
            .filter(|_| !after_digit_colon)
            .unwrap_or_else(|| crate::numeric::number(text)),
        _ => {
            let (word, rest) = word_run(text);
            (Token::Word(word), rest)
        }
    })
}

pub(crate) fn tokenize(text: &str) -> Vec<Token<'_>> {
    let mut rest = text;
    let before = |rest: &str| {
        text.get(..text.len() - rest.len())
            .and_then(|read| read.chars().next_back())
    };
    let after_digit_colon =
        |rest: &str| rest.starts_with(':') && before(rest).is_some_and(|c| c.is_ascii_digit());
    std::iter::from_fn(|| {
        let (token, after) = next_token(rest, before(rest), after_digit_colon(rest))?;
        rest = after;
        Some(token)
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn number(integer: &str) -> Token<'_> {
        Token::Number {
            integer: integer.into(),
            fraction: None,
        }
    }

    #[test]
    fn splits_words_numbers_and_symbols() {
        assert_eq!(
            tokenize("Ez $10 û 50% 3.14 1/2"),
            vec![
                Token::Word("Ez"),
                Token::Dollar,
                number("10"),
                Token::Word("û"),
                number("50"),
                Token::Percent,
                Token::Number {
                    integer: "3".into(),
                    fraction: Some("14")
                },
                number("1"),
                Token::Slash,
                number("2"),
            ]
        );
    }

    #[test]
    fn trailing_dot_ends_the_sentence() {
        assert_eq!(
            tokenize("1."),
            vec![number("1"), Token::Boundary(Break::FullStop)]
        );
    }

    #[test]
    fn dot_only_ends_a_sentence_before_a_capital_or_the_end() {
        assert_eq!(tokenize("hat. min").len(), 2);
        assert_eq!(tokenize("hat.Min").len(), 2);
        assert_eq!(tokenize("hat. Min").len(), 3);
        assert_eq!(tokenize("hwd. zêdetir").len(), 2);
    }

    #[test]
    fn comma_colon_and_semicolon_break_only_before_whitespace() {
        assert_eq!(
            tokenize("3,5"),
            vec![Token::Number {
                integer: "3".into(),
                fraction: Some("5")
            }]
        );
        assert_eq!(
            tokenize("10:30"),
            vec![Token::Time {
                hour: 10,
                minute: 30
            }]
        );
        assert_eq!(
            tokenize("a, b"),
            vec![
                Token::Word("a"),
                Token::Boundary(Break::Comma),
                Token::Word("b")
            ]
        );
        assert_eq!(
            tokenize("a: b").get(1),
            Some(&Token::Boundary(Break::Colon))
        );
    }

    #[test]
    fn a_colon_run_of_three_numbers_is_not_a_time() {
        assert_eq!(
            tokenize("12:10:15"),
            vec![number("12"), number("10"), number("15")]
        );
        assert_eq!(
            tokenize("at:10:30"),
            vec![
                Token::Word("at"),
                Token::Time {
                    hour: 10,
                    minute: 30
                }
            ]
        );
    }

    #[test]
    fn question_and_exclamation_always_break() {
        assert_eq!(
            tokenize("a?b").get(1),
            Some(&Token::Boundary(Break::Question))
        );
        assert_eq!(
            tokenize("a!").get(1),
            Some(&Token::Boundary(Break::Exclamation))
        );
    }

    #[test]
    fn hyphen_between_letters_keeps_one_word() {
        assert_eq!(tokenize("Hip-Hopê"), vec![Token::Word("Hip-Hopê")]);
        assert_eq!(tokenize("a - b").len(), 2);
        assert_eq!(tokenize("a-"), vec![Token::Word("a")]);
        assert_eq!(tokenize("a--b").len(), 2);
    }

    #[test]
    fn camel_case_splits_at_lower_to_upper_only() {
        assert_eq!(split_camel_case("iPhone"), vec!["i", "Phone"]);
        assert_eq!(split_camel_case("FIFA"), vec!["FIFA"]);
        assert_eq!(split_camel_case("Silav"), vec!["Silav"]);
    }

    #[test]
    fn a_minus_before_a_number_is_a_token() {
        assert_eq!(tokenize("-5"), vec![Token::Minus, number("5")]);
        assert_eq!(tokenize("−5"), vec![Token::Minus, number("5")]);
        assert_eq!(
            tokenize("Sar -5 derece"),
            vec![
                Token::Word("Sar"),
                Token::Minus,
                number("5"),
                Token::Word("derece")
            ]
        );
        assert_eq!(tokenize("(-5)").first(), Some(&Token::Minus));
    }

    #[test]
    fn a_hyphen_between_numbers_or_after_a_word_is_not_a_minus() {
        assert_eq!(tokenize("2006-2007"), vec![number("2006"), number("2007")]);
        assert_eq!(tokenize("a-5"), vec![Token::Word("a"), number("5")]);
        assert_eq!(tokenize("5 - 3"), vec![number("5"), number("3")]);
        assert_eq!(tokenize("-"), Vec::<Token>::new());
    }

    #[test]
    fn brackets_quotes_and_dashes_are_ignored() {
        assert_eq!(tokenize("(a) \"b\" — c").len(), 3);
    }
}
