//! Estimates where each word starts in an utterance's audio.
//!
//! Piper voices do not report alignments, so a word's start is placed in proportion to its
//! character position in the sentence. Screen readers use word events to highlight or to resume
//! reading; an estimate within a sentence is enough for both, and the sentence start is exact.
#![forbid(unsafe_code)]

use crate::fragments::{Utterance, Word};

/// Each word of `utterance` with the sample, from the utterance's first, where it starts.
pub fn word_starts(utterance: &Utterance, total_samples: usize) -> Vec<(Word, usize)> {
    let chars = utterance.char_count().max(1);
    utterance
        .words
        .iter()
        .map(|word| {
            let start = word.char_index.min(chars) as u128 * total_samples as u128 / chars as u128;
            // The quotient never exceeds `total_samples`, so it always fits back in `usize`.
            (*word, usize::try_from(start).unwrap_or(total_samples))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fragments::{plan, Action, Fragment, FragmentProsody, Step};

    fn utterance(text: &str) -> Utterance {
        let steps = plan(&[Fragment {
            action: Action::Speak,
            text: text.encode_utf16().collect(),
            offset: 0,
            prosody: FragmentProsody::default(),
            silence_ms: 0,
        }]);
        match steps.into_iter().next() {
            Some(Step::Speak(utterance)) => utterance,
            other => panic!("expected an utterance, got {other:?}"),
        }
    }

    #[test]
    fn words_start_in_proportion_to_their_characters() {
        let starts: Vec<usize> = word_starts(&utterance("ab cd efgh"), 1000)
            .into_iter()
            .map(|(_, sample)| sample)
            .collect();
        assert_eq!(starts, [0, 300, 600]);
    }

    #[test]
    fn starts_never_run_past_the_audio() {
        let starts = word_starts(&utterance("a b c d e"), 3);
        assert!(starts.iter().all(|(_, sample)| *sample <= 3));
        assert!(starts.windows(2).all(|pair| match pair {
            [(_, a), (_, b)] => a <= b,
            _ => true,
        }));
    }

    #[test]
    fn silent_audio_puts_every_word_at_the_start() {
        assert!(word_starts(&utterance("ez tu"), 0)
            .iter()
            .all(|(_, sample)| *sample == 0));
    }
}
