//! SAPI's text fragments, and the plan of steps the voice speaks them in.
//!
//! SAPI parses the caller's text and XML into a list of fragments, each with an action (speak,
//! insert silence, report a bookmark, ...) and the prosody in force for it. Offsets and lengths
//! count UTF-16 code units in the caller's original text, which is what SAPI's word and sentence
//! events must report back, so the plan keeps them in those units.
#![forbid(unsafe_code)]

/// What SAPI asks the voice to do with one fragment (`SPVACTIONS`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Speak,
    Silence,
    Bookmark,
    SpellOut,
    /// Phoneme input, section markers, unknown tags: nothing dengjen can voice.
    Ignore,
}

/// One fragment, copied out of SAPI's `SPVTEXTFRAG` list.
#[derive(Clone, Debug, PartialEq)]
pub struct Fragment {
    pub action: Action,
    pub text: Vec<u16>,
    /// Offset of `text` in the caller's original text, in UTF-16 code units.
    pub offset: u32,
    pub prosody: FragmentProsody,
    pub silence_ms: u32,
}

/// The prosody SAPI's XML put in force for a fragment, before the caller's global settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FragmentProsody {
    /// `-10..=10`, added to the voice's rate.
    pub rate_adj: i32,
    /// `0..=100`, a percentage of the voice's volume.
    pub volume: u32,
    /// `-10..=10`, the middle pitch adjustment.
    pub pitch_adj: i32,
}

/// A word inside an [`Utterance`]: its place in the caller's text and in the utterance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Word {
    /// Offset in the caller's original text, in UTF-16 code units.
    pub offset: u32,
    /// Length in UTF-16 code units.
    pub len: u32,
    /// Index of the word's first `char` in [`Utterance::text`].
    pub char_index: usize,
}

/// One sentence, synthesized in one call so an abort never waits for more than a sentence.
#[derive(Clone, Debug, PartialEq)]
pub struct Utterance {
    pub text: String,
    pub offset: u32,
    pub len: u32,
    pub words: Vec<Word>,
    pub prosody: FragmentProsody,
}

impl Utterance {
    pub fn char_count(&self) -> usize {
        self.text.chars().count()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Speak(Utterance),
    Silence { ms: u32 },
    Bookmark { name: String },
}

/// Turns SAPI's fragments into the steps the voice performs, in order.
pub fn plan(fragments: &[Fragment]) -> Vec<Step> {
    fragments.iter().flat_map(plan_fragment).collect()
}

fn plan_fragment(fragment: &Fragment) -> Vec<Step> {
    match fragment.action {
        Action::Speak => sentences(fragment).into_iter().map(Step::Speak).collect(),
        Action::SpellOut => spelled(fragment).into_iter().map(Step::Speak).collect(),
        Action::Silence => vec![Step::Silence {
            ms: fragment.silence_ms,
        }],
        Action::Bookmark => vec![Step::Bookmark {
            name: String::from_utf16_lossy(&fragment.text),
        }],
        Action::Ignore => Vec::new(),
    }
}

/// A decoded `char` with its offset and length in the fragment's UTF-16 text.
#[derive(Clone, Copy)]
struct Unit {
    ch: char,
    offset: u32,
    len: u32,
}

fn units(text: &[u16]) -> Vec<Unit> {
    let mut offset = 0u32;
    char::decode_utf16(text.iter().copied())
        .map(|decoded| {
            let (ch, len) = match decoded {
                Ok(ch) => (ch, ch.len_utf16() as u32),
                Err(_) => (char::REPLACEMENT_CHARACTER, 1),
            };
            let unit = Unit { ch, offset, len };
            offset += len;
            unit
        })
        .collect()
}

fn ends_sentence(units: &[Unit], index: usize) -> bool {
    let Some(unit) = units.get(index) else {
        return false;
    };
    let next_is_space = units
        .get(index + 1)
        .is_none_or(|next| next.ch.is_whitespace());
    unit.ch == '\n' || (matches!(unit.ch, '.' | '!' | '?' | '…' | '։' | '؟') && next_is_space)
}

/// Splits a speak fragment into sentences, dropping the ones with nothing to say.
fn sentences(fragment: &Fragment) -> Vec<Utterance> {
    let units = units(&fragment.text);
    let mut utterances = Vec::new();
    let mut start = 0;
    for index in 0..units.len() {
        if ends_sentence(&units, index) || index + 1 == units.len() {
            if let Some(slice) = units.get(start..=index) {
                utterances.extend(utterance(slice, fragment));
            }
            start = index + 1;
        }
    }
    utterances
}

fn utterance(slice: &[Unit], fragment: &Fragment) -> Option<Utterance> {
    let first = slice.iter().position(|unit| !unit.ch.is_whitespace())?;
    let last = slice.iter().rposition(|unit| !unit.ch.is_whitespace())?;
    let trimmed = slice.get(first..=last)?;
    let begin = trimmed.first()?.offset;
    let end = trimmed.last().map(|unit| unit.offset + unit.len)?;
    Some(Utterance {
        text: trimmed.iter().map(|unit| unit.ch).collect(),
        offset: fragment.offset + begin,
        len: end - begin,
        words: words(trimmed, fragment.offset),
        prosody: fragment.prosody,
    })
}

fn words(units: &[Unit], base: u32) -> Vec<Word> {
    let mut words: Vec<Word> = Vec::new();
    let mut in_word = false;
    for (char_index, unit) in units.iter().enumerate() {
        let wordy = !unit.ch.is_whitespace();
        match (wordy, in_word, words.last_mut()) {
            (true, true, Some(word)) => word.len = unit.offset + unit.len + base - word.offset,
            (true, _, _) => words.push(Word {
                offset: base + unit.offset,
                len: unit.len,
                char_index,
            }),
            (false, _, _) => {}
        }
        in_word = wordy;
    }
    words
}

/// Spells a fragment letter by letter, each letter its own word.
fn spelled(fragment: &Fragment) -> Option<Utterance> {
    let letters: Vec<Unit> = units(&fragment.text)
        .into_iter()
        .filter(|unit| !unit.ch.is_whitespace())
        .collect();
    let begin = letters.first()?.offset;
    let end = letters.last().map(|unit| unit.offset + unit.len)?;
    let words = letters
        .iter()
        .enumerate()
        .map(|(index, unit)| Word {
            offset: fragment.offset + unit.offset,
            len: unit.len,
            char_index: index * 2,
        })
        .collect();
    let text = letters
        .iter()
        .map(|unit| unit.ch.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    Some(Utterance {
        text,
        offset: fragment.offset + begin,
        len: end - begin,
        words,
        prosody: fragment.prosody,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fragment(action: Action, text: &str, offset: u32) -> Fragment {
        Fragment {
            action,
            text: text.encode_utf16().collect(),
            offset,
            prosody: FragmentProsody {
                volume: 100,
                ..FragmentProsody::default()
            },
            silence_ms: 0,
        }
    }

    fn spoken(steps: &[Step]) -> Vec<&str> {
        steps
            .iter()
            .filter_map(|step| match step {
                Step::Speak(utterance) => Some(utterance.text.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn sentences_split_on_terminal_punctuation_and_newlines() {
        let steps = plan(&[fragment(
            Action::Speak,
            "Rojbaş! Tu çawa yî? Baş im.\nSpas",
            0,
        )]);
        assert_eq!(
            spoken(&steps),
            ["Rojbaş!", "Tu çawa yî?", "Baş im.", "Spas"]
        );
    }

    #[test]
    fn decimals_and_abbreviation_dots_without_a_space_stay_in_the_sentence() {
        let steps = plan(&[fragment(Action::Speak, "Bihayê wê 3.5 lîre ye.", 0)]);
        assert_eq!(spoken(&steps), ["Bihayê wê 3.5 lîre ye."]);
    }

    #[test]
    fn offsets_count_utf16_units_of_the_original_text() {
        // "𝄞" is two UTF-16 units; the fragment starts 10 units into the caller's text.
        let steps = plan(&[fragment(Action::Speak, "  𝄞 ez. Tu", 10)]);
        let Some(Step::Speak(first)) = steps.first() else {
            panic!("expected an utterance, got {steps:?}");
        };
        assert_eq!((first.offset, first.len), (12, 6));
        assert_eq!(
            first.words,
            [
                Word {
                    offset: 12,
                    len: 2,
                    char_index: 0
                },
                Word {
                    offset: 15,
                    len: 3,
                    char_index: 2
                },
            ]
        );
        let Some(Step::Speak(second)) = steps.get(1) else {
            panic!("expected a second utterance, got {steps:?}");
        };
        assert_eq!(
            (second.offset, second.len, second.text.as_str()),
            (19, 2, "Tu")
        );
    }

    #[test]
    fn blank_fragments_say_nothing() {
        assert!(plan(&[fragment(Action::Speak, " \n\t ", 0)]).is_empty());
    }

    #[test]
    fn silence_bookmarks_and_ignored_actions_keep_their_order() {
        let mut silence = fragment(Action::Silence, "", 0);
        silence.silence_ms = 250;
        let steps = plan(&[
            fragment(Action::Bookmark, "7", 0),
            silence,
            fragment(Action::Ignore, "ignored", 0),
            fragment(Action::Speak, "Erê", 3),
        ]);
        assert!(matches!(steps.first(), Some(Step::Bookmark { name }) if name == "7"));
        assert!(matches!(steps.get(1), Some(Step::Silence { ms: 250 })));
        assert!(matches!(steps.get(2), Some(Step::Speak(utterance)) if utterance.text == "Erê"));
        assert_eq!(steps.len(), 3);
    }

    #[test]
    fn spelling_reads_each_letter_as_a_word() {
        let steps = plan(&[fragment(Action::SpellOut, "Ç a", 4)]);
        let Some(Step::Speak(utterance)) = steps.first() else {
            panic!("expected an utterance, got {steps:?}");
        };
        assert_eq!(utterance.text, "Ç a");
        assert_eq!(
            utterance
                .words
                .iter()
                .map(|word| (word.offset, word.len, word.char_index))
                .collect::<Vec<_>>(),
            [(4, 1, 0), (6, 1, 2)]
        );
    }

    #[test]
    fn unpaired_surrogates_become_replacement_characters_without_shifting_offsets() {
        let mut broken = fragment(Action::Speak, "", 0);
        broken.text = vec![0xD800, u16::from(b' '), u16::from(b'a')];
        let steps = plan(&[broken]);
        let Some(Step::Speak(utterance)) = steps.first() else {
            panic!("expected an utterance, got {steps:?}");
        };
        assert_eq!(utterance.text, "\u{FFFD} a");
        assert_eq!(utterance.words.get(1).map(|word| word.offset), Some(2));
    }
}
