#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)
)]

mod expand;
mod letters;
mod lexicon;
mod normalize;
mod numbers;
mod numeric;
mod phrases;
mod spell;
mod stress;
mod suffix;
mod tokens;

use dengjen_tts_core::{DengjenResult, Phonemes};
pub use lexicon::{parse_entries, Entry, Lexicon, LexiconError, Say};
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

fn phrase_to_ipa(words: &[String], clause_final: bool) -> String {
    let last = words.len().saturating_sub(1);
    words
        .iter()
        .enumerate()
        .map(|(i, word)| compound_to_ipa(word, clause_final && i == last))
        .filter(|ipa| !ipa.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_short_acronym(word: &str) -> bool {
    (2..=3).contains(&word.chars().count()) && word.chars().all(char::is_uppercase)
}

/// eSpeak-ng spells a short all-capitals word only inside running lowercase
/// text: more than three lowercase letters in the clause, and no more capitals
/// than lowercase letters.
fn spells_acronyms(tokens: &[Token]) -> bool {
    let (upper, lower) = tokens
        .iter()
        .filter_map(|token| match token {
            Token::Word(word) => Some(word.chars()),
            _ => None,
        })
        .flatten()
        .filter(|c| c.is_alphabetic())
        .fold((0_usize, 0_usize), |(upper, lower), c| {
            if c.is_uppercase() {
                (upper + 1, lower)
            } else if c.is_lowercase() {
                (upper, lower + 1)
            } else {
                (upper, lower)
            }
        });
    lower > 3 && upper <= lower
}

fn say_to_ipa(say: &Say, clause_final: bool) -> String {
    match say {
        Say::Phonemes(ipa) => ipa.clone(),
        Say::Respelling(words) => phrase_to_ipa(
            &words
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>(),
            clause_final,
        ),
    }
}

fn word_to_ipa(word: &str, clause_final: bool, spell_acronyms: bool, lexicon: &Lexicon) -> String {
    if let Some(say) = lexicon.lookup(word) {
        return say_to_ipa(say, clause_final);
    }
    (spell_acronyms && is_short_acronym(word))
        .then(|| stress::acronym_to_ipa(word))
        .flatten()
        .unwrap_or_else(|| compound_to_ipa(word, clause_final))
}

fn token_to_ipa(
    token: &Token,
    clause_final: bool,
    spell_acronyms: bool,
    lexicon: &Lexicon,
) -> String {
    match token {
        Token::Boundary(_) => String::new(),
        Token::Word(word) => word_to_ipa(word, clause_final, spell_acronyms, lexicon),
        Token::Time {
            hour,
            minute,
            second,
        } => phrase_to_ipa(&phrases::clock_words(*hour, *minute, *second), clause_final),
        Token::Date { day, month, year } => phrase_to_ipa(
            &phrases::date_words(*day, *month, *year).unwrap_or_default(),
            clause_final,
        ),
        Token::Ordinal { value, formal } => phrase_to_ipa(
            &spell::ordinal_words(*value, *formal).unwrap_or_default(),
            clause_final,
        ),
        Token::Currency(currency) => compound_to_ipa(currency.word(), clause_final),
        Token::Number {
            integer,
            fraction: None,
        } => numbers::cardinal(integer),
        Token::Number {
            integer,
            fraction: Some(fraction),
        } => phrases::fraction_words(integer, fraction).map_or_else(
            || numbers::decimal(integer, fraction),
            |words| phrase_to_ipa(&words, clause_final),
        ),
        Token::Percent => PERCENT.to_string(),
        Token::Dollar => DOLLAR.to_string(),
        Token::Slash => SLASH.to_string(),
        Token::Minus => compound_to_ipa(MINUS, false),
    }
}

fn clause_to_ipa(tokens: &[Token], lexicon: &Lexicon) -> String {
    let last = tokens.len().saturating_sub(1);
    let spell_acronyms = spells_acronyms(tokens);
    tokens
        .iter()
        .enumerate()
        .map(|(i, token)| token_to_ipa(token, i == last, spell_acronyms, lexicon))
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
        Some(Break::Ellipsis) => "",
        Some(Break::Colon | Break::FullStop) | None => ".",
    }
}

fn ends_sentence(kind: Option<Break>) -> bool {
    matches!(
        kind,
        None | Some(Break::FullStop | Break::Question | Break::Exclamation)
    )
}

fn line_to_sentences(line: &str, lexicon: &Lexicon) -> Vec<String> {
    let (mut sentences, pending) = clauses(expand::expand(tokens::tokenize(line))).iter().fold(
        (Vec::new(), String::new()),
        |(mut sentences, mut pending), (tokens, kind)| {
            let words = clause_to_ipa(tokens, lexicon);
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
/// returns no sentences. Words in the built-in pronunciation lexicon
/// (`lexicon/kmr.tsv`) are said as the lexicon says.
pub fn text_to_kurmanji_phonemes(text: &str) -> DengjenResult<Phonemes> {
    text_to_kurmanji_phonemes_with_lexicon(text, Lexicon::builtin())
}

/// [`text_to_kurmanji_phonemes`] with another lexicon in place of the
/// built-in one; [`Lexicon::default`] applies the letter rules alone.
pub fn text_to_kurmanji_phonemes_with_lexicon(
    text: &str,
    lexicon: &Lexicon,
) -> DengjenResult<Phonemes> {
    Ok(Phonemes::from(
        text.lines()
            .flat_map(|line| line_to_sentences(line, lexicon))
            .collect::<Vec<_>>(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sentences(text: &str) -> Vec<String> {
        text_to_kurmanji_phonemes(text).unwrap().to_vec()
    }

    #[test]
    fn a_pronoun_alone_before_a_comma_keeps_its_stress() {
        assert_eq!(sentences("Ez, bi nivîsa"), vec!["ˈɛz,bɪ nɪvˈisa."]);
        assert_eq!(sentences("Ez bi nivîsa"), vec!["ɛz bɪ nɪvˈisa."]);
    }

    #[test]
    fn uncased_letters_do_not_count_as_lowercase() {
        assert_eq!(sentences("ISO سلاو")[0], sentences("ISO")[0]);
    }

    #[test]
    fn short_capitals_in_running_lowercase_text_are_spelled() {
        assert_eq!(
            sentences("ISO sed kes hatin")[0],
            "ˌɪsˌeˈo sˈɛd kˈɛs hatˈɪn."
        );
        assert_eq!(sentences("ISO sed")[0], "ɪsˈo sˈɛd.");
        assert_eq!(
            sentences("UEFA sed kes hatin")[0],
            "ʊˈɛfa sˈɛd kˈɛs hatˈɪn."
        );
    }

    #[test]
    fn an_ellipsis_joins_the_clauses_without_a_suffix() {
        assert_eq!(sentences("min dît… te dît"), vec!["mˈɪn dˈittɛ dˈit."]);
        assert_eq!(sentences("min dît..."), vec!["mˈɪn dˈit"]);
    }

    #[test]
    fn a_period_before_a_quote_ends_the_sentence() {
        assert_eq!(
            sentences("min dît. “te dît"),
            vec!["mˈɪn dˈit.", "tɛ dˈit."]
        );
    }

    #[test]
    fn a_question_mark_inside_a_word_is_dropped() {
        assert_eq!(sentences("min a?b dît"), vec!["mˈɪn ˈab dˈit."]);
        assert_eq!(sentences("min dît!te dît"), vec!["mˈɪn dˈit tɛ dˈit."]);
    }

    #[test]
    fn a_spaced_dash_is_a_comma_break() {
        assert_eq!(sentences("min dît — te dît"), vec!["mˈɪn dˈit,tɛ dˈit."]);
    }

    #[test]
    fn capitals_are_counted_per_clause() {
        assert_eq!(
            sentences("ISO sed kes hatin, DNA"),
            vec!["ˌɪsˌeˈo sˈɛd kˈɛs hatˈɪn,dnˈa."]
        );
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
        assert_eq!(sentences("Ew hat. Ali çû").len(), 2);
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
        assert_eq!(sentences("3,5"), sentences("sê û nîv"));
        assert_eq!(sentences("3,15"), sentences("sê bêhnok yek pênc"));
        assert_eq!(sentences("12.000"), sentences("12000"));
        assert_eq!(sentences("1.502"), sentences("1502"));
        assert_eq!(sentences("3/4"), vec!["sˈe slˈaʃ dʒˈar."]);
    }

    #[test]
    fn times_dates_and_ordinals_are_spelled_as_words() {
        assert_eq!(sentences("10:30"), sentences("saet deh û nîv"));
        assert_eq!(
            sentences("28.10.2026"),
            sentences("bîst û heştê Çirîyê du hezar û bîst û şeş")
        );
        assert_eq!(sentences("27/10/2026"), sentences("27.10.2026"));
        assert_eq!(sentences("3em"), sentences("sêyem"));
        assert_eq!(sentences("3-emîn"), sentences("sêyemîn"));
    }

    #[test]
    fn currency_units_and_titles_are_spelled_as_words() {
        assert_eq!(sentences("€5"), sentences("pênc euro"));
        assert_eq!(sentences("5 €"), sentences("pênc euro"));
        assert_eq!(sentences("5 km"), sentences("pênc kîlometre"));
        assert_eq!(sentences("Dr. Ali"), sentences("doktor Ali"));
        assert_eq!(sentences("Dr. Ali hat").len(), 1);
    }

    #[test]
    fn an_ordinal_needs_its_suffix() {
        assert_eq!(sentences("3."), sentences("3"));
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

    fn with(rows: &str, text: &str) -> Vec<String> {
        let lexicon = Lexicon::parse(&format!("written\tsay\tnote\n{rows}")).unwrap();
        text_to_kurmanji_phonemes_with_lexicon(text, &lexicon)
            .unwrap()
            .to_vec()
    }

    #[test]
    fn a_respelling_is_read_with_the_usual_rules_and_stress() {
        assert_eq!(
            with("AB\tYekîtiya Ewropayê", "AB hat"),
            sentences("Yekîtiya Ewropayê hat")
        );
        assert_eq!(with("AB\tYekîtiya Ewropayê", "ab hat"), sentences("ab hat"));
    }

    #[test]
    fn phonemes_are_used_as_written() {
        assert_eq!(
            with("silav\t/sɪlˈaw/", "Silav, heval"),
            vec!["sɪlˈaw,hɛvˈal."]
        );
    }

    #[test]
    fn a_lexicon_entry_wins_over_acronym_spelling() {
        assert_eq!(
            with("ISO\tîso", "ISO sed kes hatin"),
            sentences("îso sed kes hatin")
        );
    }

    #[test]
    fn the_lexicon_matches_whole_words_only() {
        assert_eq!(with("silav\t/sɪlˈaw/", "silavên"), sentences("silavên"));
    }

    #[test]
    fn long_text_is_phonemized_without_blowing_up() {
        let text = "Ez diçim. ".repeat(20_000);
        assert_eq!(sentences(&text).len(), 20_000);
        assert_eq!(sentences(&"a".repeat(300_000)).len(), 1);
    }
}
