#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)
)]

mod letters;
mod normalize;
mod numbers;
mod stress;
mod suffix;
mod tokens;

use dengjen_tts_core::{DengjenResult, Phonemes};
use tokens::{Break, Token};

const PERCENT: &str = "sɛdˈe";
const DOLLAR: &str = "dʊlˈar";
const SLASH: &str = "slˈaʃ";
const MINUS: &str = "mînus";

type Clause<'a> = (Vec<Token<'a>>, Option<Break>);

fn compound_to_ipa(word: &str, clause_final: bool) -> String {
    let parts = tokens::split_camel_case(word);
    let single_word = parts.len() == 1 && !word.contains('-');
    parts
        .into_iter()
        .map(|part| {
            part.split('-')
                .map(|piece| stress::word_to_ipa(piece, clause_final && single_word))
                .collect::<String>()
        })
        .filter(|ipa| !ipa.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn token_to_ipa(token: &Token, clause_final: bool) -> String {
    match token {
        Token::Boundary(_) => String::new(),
        Token::Word(word) => compound_to_ipa(word, clause_final),
        Token::Number {
            integer,
            fraction: None,
        } => numbers::cardinal(integer),
        Token::Number {
            integer,
            fraction: Some(fraction),
        } => numbers::decimal(integer, fraction),
        Token::Percent => PERCENT.to_string(),
        Token::Dollar => DOLLAR.to_string(),
        Token::Slash => SLASH.to_string(),
        Token::Minus => compound_to_ipa(MINUS, false),
    }
}

fn clause_to_ipa(tokens: &[Token]) -> String {
    let last = tokens.len().saturating_sub(1);
    tokens
        .iter()
        .enumerate()
        .map(|(i, token)| token_to_ipa(token, tokens.len() > 1 && i == last))
        .filter(|ipa| !ipa.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn clauses(tokens: Vec<Token>) -> Vec<Clause> {
    let (mut done, current) = tokens.into_iter().fold(
        (Vec::new(), Vec::new()),
        |(mut done, mut current), token| {
            match token {
                Token::Boundary(kind) => done.push((std::mem::take(&mut current), Some(kind))),
                other => current.push(other),
            }
            (done, current)
        },
    );
    done.push((current, None));
    done
}

fn break_suffix(kind: Option<Break>) -> &'static str {
    match kind {
        Some(Break::Comma) => ",",
        Some(Break::Question) => "?",
        Some(Break::Exclamation) => "!",
        Some(Break::Colon | Break::FullStop) | None => ".",
    }
}

fn ends_sentence(kind: Option<Break>) -> bool {
    matches!(
        kind,
        None | Some(Break::FullStop | Break::Question | Break::Exclamation)
    )
}

fn line_to_sentences(line: &str) -> Vec<String> {
    let (mut sentences, pending) = clauses(tokens::tokenize(line)).iter().fold(
        (Vec::new(), String::new()),
        |(mut sentences, mut pending), (tokens, kind)| {
            let words = clause_to_ipa(tokens);
            if !words.is_empty() {
                pending.push_str(&words);
                pending.push_str(break_suffix(*kind));
            }
            if ends_sentence(*kind) && !pending.is_empty() {
                sentences.push(std::mem::take(&mut pending));
            }
            (sentences, pending)
        },
    );
    if !pending.is_empty() {
        sentences.push(pending);
    }
    sentences
}

/// Returns one phoneme string per spoken sentence, as espeak-ng does: clause
/// breaks keep their `,` `.` `?` `!` suffix, and text with nothing speakable
/// returns no sentences.
pub fn text_to_kurmanji_phonemes(text: &str) -> DengjenResult<Phonemes> {
    Ok(Phonemes::from(
        text.lines().flat_map(line_to_sentences).collect::<Vec<_>>(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sentences(text: &str) -> Vec<String> {
        text_to_kurmanji_phonemes(text).unwrap().to_vec()
    }

    #[test]
    fn empty_input_yields_no_sentences() {
        assert!(sentences("").is_empty());
    }

    #[test]
    fn whitespace_and_punctuation_only_yield_no_sentences() {
        assert!(sentences("  ,.! ").is_empty());
        assert!(sentences("\n\n").is_empty());
    }

    #[test]
    fn arabic_script_yields_no_sentences() {
        assert!(sentences("سلاو").is_empty());
    }

    #[test]
    fn clause_breaks_keep_their_suffix_and_glue_the_next_clause() {
        assert_eq!(
            sentences("Silav, tu çawa yî?"),
            vec!["sɪlˈav,tʊ tʃˈawa jˈi?"]
        );
    }

    #[test]
    fn full_stop_question_and_exclamation_start_new_sentences() {
        assert_eq!(
            sentences("Ez diçim. Tu tê? Erê!"),
            vec!["ɛz dɪtʃˈɪm.", "tʊ tˈe?", "ˈɛre!"]
        );
    }

    #[test]
    fn text_without_a_terminator_ends_with_a_full_stop() {
        assert_eq!(sentences("Ez diçim"), vec!["ɛz dɪtʃˈɪm."]);
    }

    #[test]
    fn colon_continues_the_sentence_with_a_full_stop_suffix() {
        assert_eq!(sentences("Ew hat: min dît"), vec!["eʊ hˈat.mˈɪn dˈit."]);
    }

    #[test]
    fn repeated_breaks_collapse_and_a_trailing_comma_is_kept() {
        assert_eq!(sentences("Ew hat, , min dît"), vec!["eʊ hˈat,mˈɪn dˈit."]);
        assert_eq!(sentences("Ew hat,"), vec!["eʊ hˈat,"]);
    }

    #[test]
    fn a_period_before_lowercase_text_does_not_end_the_sentence() {
        assert_eq!(sentences("Ew hat. min dît"), vec!["eʊ hˈat mˈɪn dˈit."]);
        assert_eq!(sentences("Dr. Ali hat").len(), 2);
    }

    #[test]
    fn each_line_is_its_own_paragraph() {
        assert_eq!(sentences("Ew hat\nTu çûyî"), vec!["eʊ hˈat.", "tʊ tʃujˈi."]);
    }

    #[test]
    fn decomposed_input_stays_one_word() {
        assert_eq!(sentences("hi\u{302}vi\u{302}"), sentences("hîvî"));
    }

    #[test]
    fn apostrophes_split_words_without_panicking() {
        assert_eq!(sentences("n'an").len(), 1);
        assert_eq!(sentences("n’an").len(), 1);
    }

    #[test]
    fn numbers_percent_dollar_and_slash_are_spoken() {
        assert_eq!(sentences("50%"), vec!["pendʒˈi sɛdˈe."]);
        assert_eq!(sentences("$10"), vec!["dʊlˈar dˈɛh."]);
        assert_eq!(sentences("10:30"), vec!["dˈɛh sˈi."]);
        assert_eq!(sentences("3,5"), vec!["sˈe pˈendʒ."]);
        assert_eq!(
            sentences("27/10/2026"),
            vec!["bˈɪstuhˈɛft slˈaʃ dˈɛh slˈaʃ dˈʊ hɛzˈar u bˈɪstuʃˈɛʃ."]
        );
    }

    #[test]
    fn a_minus_sign_before_a_number_is_spoken() {
        assert_eq!(sentences("-5"), vec!["minˈʊs pˈendʒ."]);
        assert_eq!(sentences("−5"), vec!["minˈʊs pˈendʒ."]);
        assert_eq!(
            sentences("Sar -5 derece"),
            vec!["sˈar minˈʊs pˈendʒ dˌɛɾɛdʒˈɛ."]
        );
    }

    #[test]
    fn a_hyphen_in_a_range_is_not_spoken_as_minus() {
        assert_eq!(sentences("2006-2007"), sentences("2006 2007"));
    }

    #[test]
    fn long_text_is_phonemized_without_blowing_up() {
        let text = "Ez diçim. ".repeat(20_000);
        assert_eq!(sentences(&text).len(), 20_000);
        assert_eq!(sentences(&"a".repeat(300_000)).len(), 1);
    }
}
