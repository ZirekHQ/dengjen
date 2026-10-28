use crate::tokens::{Break, Token};

type Step<'a> = (Vec<Token<'a>>, usize);

fn unit_after_number(unit: &str) -> Option<&'static str> {
    match unit {
        "km" => Some("kîlometre"),
        "kg" => Some("kîlogram"),
        "m" => Some("metre"),
        "cm" => Some("santîmetre"),
        "TL" => Some("lîre"),
        _ => None,
    }
}

fn title(word: &str) -> Option<&'static str> {
    match word {
        "Dr" | "dr" => Some("doktor"),
        "Prof" | "prof" => Some("profesor"),
        _ => None,
    }
}

fn step<'a>(rest: &[Token<'a>]) -> Step<'a> {
    match rest {
        [Token::Currency(c), Token::Minus, number @ Token::Number { .. }, ..] => {
            (vec![Token::Minus, number.clone(), Token::Word(c.word())], 3)
        }
        [Token::Currency(c), number @ Token::Number { .. }, ..] => {
            (vec![number.clone(), Token::Word(c.word())], 2)
        }
        [Token::Currency(c), ..] => (vec![Token::Word(c.word())], 1),
        [number @ Token::Number { .. }, Token::Word(unit), ..]
            if unit_after_number(unit).is_some() =>
        {
            let spoken = unit_after_number(unit).unwrap_or_default();
            (vec![number.clone(), Token::Word(spoken)], 2)
        }
        [Token::Word(word), Token::Boundary(Break::FullStop), _, ..] if title(word).is_some() => {
            (vec![Token::Word(title(word).unwrap_or_default())], 2)
        }
        [Token::Word(word), ..] if title(word).is_some() => {
            (vec![Token::Word(title(word).unwrap_or_default())], 1)
        }
        [token, ..] => (vec![token.clone()], 1),
        [] => (Vec::new(), 0),
    }
}

pub(crate) fn expand(tokens: Vec<Token<'_>>) -> Vec<Token<'_>> {
    let mut consumed = 0;
    std::iter::from_fn(|| {
        let (out, used) = step(tokens.get(consumed..)?);
        consumed += used;
        (used > 0).then_some(out)
    })
    .flatten()
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::{tokenize, Currency};

    fn expanded(text: &str) -> Vec<Token<'_>> {
        expand(tokenize(text))
    }

    #[test]
    fn currency_follows_its_amount_wherever_the_symbol_sits() {
        let expected = vec![
            Token::Number {
                integer: "5".into(),
                fraction: None,
            },
            Token::Word(Currency::Euro.word()),
        ];
        assert_eq!(expanded("€5"), expected);
        assert_eq!(expanded("5 €"), expected);
    }

    #[test]
    fn units_after_a_number_are_spelled_out() {
        assert_eq!(expanded("5 km").get(1), Some(&Token::Word("kîlometre")));
        assert_eq!(expanded("5 TL").get(1), Some(&Token::Word("lîre")));
        assert_eq!(expanded("km").first(), Some(&Token::Word("km")));
    }

    #[test]
    fn a_title_absorbs_its_full_stop() {
        assert_eq!(
            expanded("Dr. Ali"),
            vec![Token::Word("doktor"), Token::Word("Ali")]
        );
        assert_eq!(
            expanded("Prof. Ali"),
            vec![Token::Word("profesor"), Token::Word("Ali")]
        );
    }

    #[test]
    fn a_title_at_the_end_keeps_its_full_stop() {
        assert_eq!(
            expanded("Dr."),
            vec![Token::Word("doktor"), Token::Boundary(Break::FullStop)]
        );
    }

    #[test]
    fn abbreviations_espeak_already_expands_are_left_alone() {
        assert_eq!(expanded("hwd").first(), Some(&Token::Word("hwd")));
    }
}
