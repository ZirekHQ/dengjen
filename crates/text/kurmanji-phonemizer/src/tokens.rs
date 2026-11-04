use std::borrow::Cow;

use unicode_normalization::char::is_combining_mark;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Break {
    Comma,
    Colon,
    FullStop,
    Question,
    Exclamation,
    Ellipsis,
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
        second: Option<usize>,
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

fn is_word_joiner(c: char) -> bool {
    matches!(c, '-' | '\'' | '’')
}

fn is_mark_run_char(c: char) -> bool {
    matches!(c, '?' | '!' | '.' | ',')
}

/// Index after the char at `k` when it belongs to the word, or `None` when the word ends there.
/// A joiner needs a letter on both sides; a `?` and the marks after it vanish between letters.
fn word_step(chars: &[(usize, char)], k: usize) -> Option<usize> {
    let letter_at = |i: Option<usize>| {
        i.and_then(|i| chars.get(i))
            .is_some_and(|(_, c)| is_word_char(*c))
    };
    let (_, c) = chars.get(k)?;
    let after_marks = || {
        (k..chars.len())
            .find(|i| chars.get(*i).is_some_and(|(_, m)| !is_mark_run_char(*m)))
            .unwrap_or(chars.len())
    };
    if is_word_char(*c) {
        Some(k + 1)
    } else if is_word_joiner(*c) {
        (letter_at(k.checked_sub(1)) && letter_at(Some(k + 1))).then_some(k + 1)
    } else if *c == '?' && letter_at(k.checked_sub(1)) {
        Some(after_marks()).filter(|end| letter_at(Some(*end)))
    } else {
        None
    }
}

fn word_run(text: &str) -> (&str, &str) {
    let span_len = text
        .find(|c| !is_word_char(c) && !is_word_joiner(c) && !is_mark_run_char(c))
        .unwrap_or(text.len());
    let chars: Vec<(usize, char)> = text
        .get(..span_len)
        .unwrap_or_default()
        .char_indices()
        .collect();
    let end = std::iter::successors(Some(0), |k| word_step(&chars, *k))
        .last()
        .unwrap_or(0);
    text.split_at(chars.get(end).map_or(span_len, |(i, _)| *i))
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

const BRACKETS: &str = "()[]{}<>\"'`«»\u{300a}\u{300b}";

fn is_bracket(c: char) -> bool {
    BRACKETS.contains(c) || ('\u{2014}'..='\u{201f}').contains(&c)
}

fn is_extended_dash(c: char) -> bool {
    matches!(
        c,
        '\u{2013}' | '\u{2014}' | '\u{2e3a}' | '\u{2e3b}' | '\u{fe31}' | '\u{fe32}'
    )
}

fn is_ellipsis(c: char, after: &str) -> bool {
    c == '\u{2026}' || (c == '.' && after.starts_with(".."))
}

fn clause_may_end(after: &str) -> bool {
    after
        .chars()
        .next()
        .is_none_or(|n| n.is_whitespace() || is_bracket(n))
}

fn marks_end_clause(after: &str) -> bool {
    clause_may_end(after.trim_start_matches(['.', '!', '?']))
}

fn ends_sentence(after_period: &str) -> bool {
    clause_may_end(after_period) && !after_period.trim_start().starts_with(char::is_lowercase)
}

fn break_for(c: char, after: &str) -> Option<Break> {
    match c {
        _ if is_ellipsis(c, after) => Some(Break::Ellipsis),
        '?' if marks_end_clause(after) => Some(Break::Question),
        '!' if marks_end_clause(after) => Some(Break::Exclamation),
        ',' | ';' if clause_may_end(after) => Some(Break::Comma),
        _ if is_extended_dash(c) && clause_may_end(after) => Some(Break::Comma),
        ':' if clause_may_end(after) => Some(Break::Colon),
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
        let rest = if kind == Break::Ellipsis {
            after.trim_start_matches('.')
        } else {
            after
        };
        return Some((Token::Boundary(kind), rest));
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
    fn apostrophes_between_letters_stay_inside_the_word() {
        assert_eq!(tokenize("Qur’anê"), vec![Token::Word("Qur’anê")]);
        assert_eq!(tokenize("Sen'an"), vec![Token::Word("Sen'an")]);
        assert_eq!(tokenize("'ez'").len(), 1);
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
                minute: 30,
                second: None
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
    fn a_colon_run_of_four_numbers_is_not_a_time() {
        assert_eq!(
            tokenize("12:10:15:20"),
            vec![number("12"), number("10"), number("15"), number("20")]
        );
        assert_eq!(
            tokenize("at:10:30"),
            vec![
                Token::Word("at"),
                Token::Time {
                    hour: 10,
                    minute: 30,
                    second: None
                }
            ]
        );
    }

    #[test]
    fn question_and_exclamation_break_before_a_space_bracket_or_the_end() {
        assert_eq!(
            tokenize("a? b").get(1),
            Some(&Token::Boundary(Break::Question))
        );
        assert_eq!(
            tokenize("a!").get(1),
            Some(&Token::Boundary(Break::Exclamation))
        );
    }

    #[test]
    fn a_question_mark_between_letters_joins_them_and_an_exclamation_mark_splits_them() {
        assert_eq!(tokenize("a?b"), vec![Token::Word("a?b")]);
        assert_eq!(tokenize("a!b"), vec![Token::Word("a"), Token::Word("b")]);
    }

    #[test]
    fn marks_after_a_question_mark_between_letters_vanish_too() {
        assert_eq!(tokenize("a?!b"), vec![Token::Word("a?!b")]);
        assert_eq!(tokenize("a?.,b"), vec![Token::Word("a?.,b")]);
        assert_eq!(
            tokenize("a?! b").get(..2),
            Some(&[Token::Word("a"), Token::Boundary(Break::Question)][..])
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
        assert_eq!(tokenize("(a) \"b\" c").len(), 3);
    }

    #[test]
    fn spaced_extended_dashes_break_like_a_comma() {
        assert_eq!(
            tokenize("a — b"),
            vec![
                Token::Word("a"),
                Token::Boundary(Break::Comma),
                Token::Word("b")
            ]
        );
        assert_eq!(tokenize("a—b").len(), 2);
    }

    #[test]
    fn three_dots_or_an_ellipsis_break_without_ending_the_sentence() {
        let ellipsis = vec![
            Token::Word("a"),
            Token::Boundary(Break::Ellipsis),
            Token::Word("B"),
        ];
        assert_eq!(tokenize("a... B"), ellipsis);
        assert_eq!(tokenize("a…B"), ellipsis);
        assert_eq!(tokenize("a.... B"), ellipsis);
        assert_eq!(tokenize("a.. b").len(), 2);
    }

    #[test]
    fn a_closing_quote_lets_a_clause_mark_break() {
        assert_eq!(
            tokenize("a,” b").get(1),
            Some(&Token::Boundary(Break::Comma))
        );
        assert_eq!(
            tokenize("a.” b").get(1),
            Some(&Token::Boundary(Break::FullStop))
        );
        assert_eq!(
            tokenize("a. “b").get(1),
            Some(&Token::Boundary(Break::FullStop))
        );
    }
}
