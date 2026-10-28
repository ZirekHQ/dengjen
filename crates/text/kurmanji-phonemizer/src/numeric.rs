use std::borrow::Cow;

use crate::tokens::{is_word_char, Token};

const MAX_ORDINAL_DIGITS: usize = 9;
const ORDINAL_SUFFIXES: [(&str, bool); 4] = [
    ("yemîn", true),
    ("emîn", true),
    ("yem", false),
    ("em", false),
];

type Parsed<'a> = Option<(Token<'a>, &'a str)>;

fn digit_run(text: &str, lengths: std::ops::RangeInclusive<usize>) -> Option<(&str, &str)> {
    let end = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    let (digits, rest) = text.split_at(end);
    lengths.contains(&digits.len()).then_some((digits, rest))
}

fn value(digits: &str) -> Option<usize> {
    digits.parse().ok()
}

fn continues_with_digit_after(rest: &str, separator: char) -> bool {
    rest.strip_prefix(separator)
        .is_some_and(|after| after.starts_with(|c: char| c.is_ascii_digit()))
}

fn time(text: &str) -> Parsed<'_> {
    let (hour, rest) = digit_run(text, 1..=2)?;
    let (minute, rest) = digit_run(rest.strip_prefix(':')?, 2..=2)?;
    let (hour, minute) = (value(hour)?, value(minute)?);
    (hour < 24 && minute < 60 && !continues_with_digit_after(rest, ':'))
        .then_some((Token::Time { hour, minute }, rest))
}

fn three_parts(text: &str) -> Option<([&str; 3], char, &str)> {
    let (first, rest) = digit_run(text, 1..=4)?;
    let separator = rest
        .chars()
        .next()
        .filter(|c| matches!(c, '.' | '/' | '-'))?;
    let (second, rest) = digit_run(rest.strip_prefix(separator)?, 1..=4)?;
    let (third, rest) = digit_run(rest.strip_prefix(separator)?, 1..=4)?;
    Some(([first, second, third], separator, rest))
}

fn day_month_year(parts: [&str; 3], separator: char) -> Option<(usize, usize, usize)> {
    let [first, second, third] = parts;
    let year_first = separator == '-' && first.len() == 4 && second.len() == 2 && third.len() == 2;
    let year_last = separator != '-' && first.len() <= 2 && second.len() <= 2 && third.len() == 4;
    match (year_first, year_last) {
        (true, _) => Some((value(third)?, value(second)?, value(first)?)),
        (_, true) => Some((value(first)?, value(second)?, value(third)?)),
        _ => None,
    }
}

fn date(text: &str) -> Parsed<'_> {
    let (parts, separator, rest) = three_parts(text)?;
    let (day, month, year) = day_month_year(parts, separator)?;
    ((1..=31).contains(&day)
        && (1..=12).contains(&month)
        && !continues_with_digit_after(rest, separator))
    .then_some((Token::Date { day, month, year }, rest))
}

fn ordinal_suffix(rest: &str) -> Option<(bool, &str)> {
    let rest = rest.strip_prefix('-').unwrap_or(rest);
    ORDINAL_SUFFIXES.iter().find_map(|(suffix, formal)| {
        rest.strip_prefix(suffix)
            .filter(|after| !after.starts_with(is_word_char))
            .map(|after| (*formal, after))
    })
}

fn ordinal(text: &str) -> Parsed<'_> {
    let (digits, rest) = digit_run(text, 1..=MAX_ORDINAL_DIGITS)?;
    let (formal, rest) = ordinal_suffix(rest)?;
    Some((
        Token::Ordinal {
            value: value(digits)?,
            formal,
        },
        rest,
    ))
}

pub(crate) fn structured(text: &str) -> Parsed<'_> {
    date(text).or_else(|| time(text)).or_else(|| ordinal(text))
}

fn thousand_groups(text: &str) -> (Vec<&str>, &str) {
    let mut rest = text;
    let groups = std::iter::from_fn(|| {
        let (group, next) = digit_run(rest.strip_prefix('.')?, 3..=3)?;
        rest = next;
        Some(group)
    })
    .collect();
    (groups, rest)
}

fn grouped_integer(text: &str) -> Option<(String, &str)> {
    let (head, after_head) = digit_run(text, 1..=3)?;
    let (groups, rest) = thousand_groups(after_head);
    let ambiguous = continues_with_digit_after(rest, '.');
    (!groups.is_empty() && !head.starts_with('0') && !ambiguous)
        .then(|| (format!("{head}{}", groups.concat()), rest))
}

fn fraction_digits<'a>(rest: &'a str, separators: &[char]) -> Option<(&'a str, &'a str)> {
    separators
        .iter()
        .find_map(|separator| digit_run(rest.strip_prefix(*separator)?, 1..=usize::MAX))
}

/// Reads a number with `.` grouping thousands and `,` as the decimal separator, as written in
/// Turkey; a dot before fewer or more than three digits stays a decimal point.
pub(crate) fn number(text: &str) -> (Token<'_>, &str) {
    let (integer, rest, separators): (Cow<str>, &str, &[char]) = match grouped_integer(text) {
        Some((digits, rest)) => (Cow::Owned(digits), rest, &[',']),
        None => {
            let (digits, rest) = digit_run(text, 1..=usize::MAX).unwrap_or((text, ""));
            (Cow::Borrowed(digits), rest, &['.', ','])
        }
    };
    match fraction_digits(rest, separators) {
        Some((fraction, rest)) => (
            Token::Number {
                integer,
                fraction: Some(fraction),
            },
            rest,
        ),
        None => (
            Token::Number {
                integer,
                fraction: None,
            },
            rest,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(text: &str) -> Option<Token<'_>> {
        structured(text).map(|(token, _)| token)
    }

    fn parts(text: &str) -> (String, Option<String>) {
        match number(text).0 {
            Token::Number { integer, fraction } => {
                (integer.into_owned(), fraction.map(str::to_string))
            }
            other => panic!("not a number: {other:?}"),
        }
    }

    fn split(integer: &str, fraction: Option<&str>) -> (String, Option<String>) {
        (integer.to_string(), fraction.map(str::to_string))
    }

    #[test]
    fn a_dot_before_three_digits_groups_thousands() {
        assert_eq!(parts("1.502"), split("1502", None));
        assert_eq!(parts("1.000.000"), split("1000000", None));
        assert_eq!(parts("12.000"), split("12000", None));
    }

    #[test]
    fn a_comma_between_digits_is_the_decimal_separator() {
        assert_eq!(parts("3,5"), split("3", Some("5")));
        assert_eq!(parts("1.234,56"), split("1234", Some("56")));
    }

    #[test]
    fn other_dots_stay_decimal_points() {
        assert_eq!(parts("3.14"), split("3", Some("14")));
        assert_eq!(parts("0.500"), split("0", Some("500")));
        assert_eq!(parts("1.2345"), split("1", Some("2345")));
    }

    #[test]
    fn times_need_valid_hours_and_minutes() {
        assert_eq!(
            parsed("12:30"),
            Some(Token::Time {
                hour: 12,
                minute: 30
            })
        );
        assert_eq!(parsed("9:05"), Some(Token::Time { hour: 9, minute: 5 }));
        assert_eq!(parsed("24:00"), None);
        assert_eq!(parsed("12:60"), None);
        assert_eq!(parsed("12:30:15"), None);
    }

    #[test]
    fn dates_accept_dotted_slashed_and_iso_forms() {
        let expected = Some(Token::Date {
            day: 28,
            month: 10,
            year: 2026,
        });
        assert_eq!(parsed("28.10.2026"), expected);
        assert_eq!(parsed("28/10/2026"), expected);
        assert_eq!(parsed("2026-10-28"), expected);
        assert_eq!(parsed("28.13.2026"), None);
        assert_eq!(parsed("2006-2007"), None);
    }

    #[test]
    fn ordinals_need_an_explicit_suffix() {
        assert_eq!(
            parsed("3em"),
            Some(Token::Ordinal {
                value: 3,
                formal: false
            })
        );
        assert_eq!(
            parsed("3-yem"),
            Some(Token::Ordinal {
                value: 3,
                formal: false
            })
        );
        assert_eq!(
            parsed("12-emîn"),
            Some(Token::Ordinal {
                value: 12,
                formal: true
            })
        );
        assert_eq!(parsed("3."), None);
        assert_eq!(parsed("3emxx"), None);
        assert_eq!(parsed("2026an"), None);
    }
}
