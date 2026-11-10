//! The body of `ISpTTSEngine::Speak`, written against two small traits so it runs in tests.
//!
//! SAPI calls `Speak` once per request and keeps the call open until all audio is written. The
//! caller cancels by setting `SPVES_ABORT`, which the engine must poll; screen readers cancel on
//! every key press, so the loop checks between sentences, between synthesized chunks and between
//! slices of written audio, and never writes more than [`SLICE_MS`] after an abort.
#![forbid(unsafe_code)]

use crate::fragments::{Step, Utterance};
use crate::prosody::{engine_prosody, EngineProsody};
use crate::timing::word_starts;

/// How much audio goes to SAPI between two abort checks.
pub const SLICE_MS: u32 = 40;
const BYTES_PER_SAMPLE: u64 = 2;

/// What the caller has asked for since the last check (`ISpTTSEngineSite::GetActions`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Actions {
    pub abort: bool,
    pub skip: bool,
}

/// An event for SAPI to fire once playback reaches `audio_offset`, a byte offset in the output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Sentence {
        audio_offset: u64,
        offset: u32,
        len: u32,
    },
    Word {
        audio_offset: u64,
        offset: u32,
        len: u32,
    },
    Bookmark {
        audio_offset: u64,
        name: String,
    },
}

/// The SAPI side: `ISpTTSEngineSite`.
pub trait Site {
    fn actions(&self) -> Actions;
    /// The caller's rate, `-10..=10`.
    fn rate(&self) -> i32;
    /// The caller's volume, `0..=100`.
    fn volume(&self) -> u16;
    /// How many sentences the caller wants skipped, negative to go back.
    fn skip_request(&self) -> i32;
    fn complete_skip(&self, skipped: i32);
    fn write(&self, audio: &[u8]) -> Result<(), String>;
    fn add_events(&self, events: &[Event]) -> Result<(), String>;
}

/// The engine side: one loaded voice producing mono 16-bit samples.
pub trait Synth {
    fn sample_rate(&self) -> u32;
    /// Synthesizes `text`, handing each chunk to `chunk` until it returns `false`.
    fn synthesize(
        &self,
        text: &str,
        prosody: EngineProsody,
        chunk: &mut dyn FnMut(&[i16]) -> bool,
    ) -> Result<(), String>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Completed,
    Aborted,
}

struct Output<'a> {
    site: &'a dyn Site,
    written: u64,
    slice_samples: usize,
}

impl Output<'_> {
    fn aborted(&self) -> bool {
        self.site.actions().abort
    }

    /// Writes samples a slice at a time, stopping at the first abort.
    fn write(&mut self, samples: &[i16]) -> Result<Outcome, String> {
        for slice in samples.chunks(self.slice_samples) {
            if self.aborted() {
                return Ok(Outcome::Aborted);
            }
            let bytes: Vec<u8> = slice
                .iter()
                .flat_map(|sample| sample.to_le_bytes())
                .collect();
            self.site.write(&bytes)?;
            self.written += slice.len() as u64 * BYTES_PER_SAMPLE;
        }
        Ok(Outcome::Completed)
    }
}

/// Speaks `steps` through `site`, returning early when the caller aborts.
pub fn speak(steps: &[Step], synth: &dyn Synth, site: &dyn Site) -> Result<Outcome, String> {
    let sample_rate = synth.sample_rate().max(1);
    let mut output = Output {
        site,
        written: 0,
        slice_samples: (sample_rate * SLICE_MS / 1000).max(1) as usize,
    };
    let mut index = 0;
    while let Some(step) = steps.get(index) {
        let actions = site.actions();
        if actions.abort {
            return Ok(Outcome::Aborted);
        }
        if actions.skip {
            index = skip(steps, index, site);
            continue;
        }
        let outcome = match step {
            Step::Speak(utterance) => speak_utterance(utterance, synth, &mut output)?,
            Step::Silence { ms } => {
                let samples = (u64::from(*ms) * u64::from(sample_rate) / 1000) as usize;
                output.write(&vec![0; samples])?
            }
            Step::Bookmark { name } => {
                site.add_events(&[Event::Bookmark {
                    audio_offset: output.written,
                    name: name.clone(),
                }])?;
                Outcome::Completed
            }
        };
        if outcome == Outcome::Aborted {
            return Ok(Outcome::Aborted);
        }
        index += 1;
    }
    Ok(Outcome::Completed)
}

fn speak_utterance(
    utterance: &Utterance,
    synth: &dyn Synth,
    output: &mut Output<'_>,
) -> Result<Outcome, String> {
    let site = output.site;
    let prosody = engine_prosody(site.rate(), site.volume(), utterance.prosody);
    let mut samples = Vec::new();
    let mut aborted = false;
    synth.synthesize(&utterance.text, prosody, &mut |chunk| {
        samples.extend_from_slice(chunk);
        aborted = site.actions().abort;
        !aborted
    })?;
    if aborted {
        return Ok(Outcome::Aborted);
    }
    let start = output.written;
    let mut events = vec![Event::Sentence {
        audio_offset: start,
        offset: utterance.offset,
        len: utterance.len,
    }];
    events.extend(
        word_starts(utterance, samples.len())
            .into_iter()
            .map(|(word, sample)| Event::Word {
                audio_offset: start + sample as u64 * BYTES_PER_SAMPLE,
                offset: word.offset,
                len: word.len,
            }),
    );
    site.add_events(&events)?;
    output.write(&samples)
}

/// Moves past (or back over) the sentences the caller asked to skip, and reports how many.
fn skip(steps: &[Step], index: usize, site: &dyn Site) -> usize {
    let wanted = site.skip_request();
    let sentences = |range: &mut dyn Iterator<Item = usize>| -> Vec<usize> {
        range
            .filter(|&at| matches!(steps.get(at), Some(Step::Speak(_))))
            .collect()
    };
    let (target, skipped) = match wanted {
        0 => (index, 0),
        forward if forward > 0 => {
            let ahead = sentences(&mut (index..steps.len()));
            let count = ahead.len().min(forward.unsigned_abs() as usize);
            let target = ahead.get(count).copied().unwrap_or(steps.len());
            (target, count as i32)
        }
        backward => {
            let behind = sentences(&mut (0..index).rev());
            let count = behind.len().min(backward.unsigned_abs() as usize);
            let target = count
                .checked_sub(1)
                .and_then(|last| behind.get(last))
                .copied()
                .unwrap_or(index);
            (target, -(count as i32))
        }
    };
    site.complete_skip(skipped);
    target
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fragments::{plan, Action, Fragment, FragmentProsody};
    use std::cell::{Cell, RefCell};

    const RATE: u32 = 1000;

    /// Produces one sample per character, in two chunks, each sample its character's index.
    struct FakeSynth {
        calls: RefCell<Vec<(String, EngineProsody)>>,
    }

    impl FakeSynth {
        fn new() -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl Synth for FakeSynth {
        fn sample_rate(&self) -> u32 {
            RATE
        }

        fn synthesize(
            &self,
            text: &str,
            prosody: EngineProsody,
            chunk: &mut dyn FnMut(&[i16]) -> bool,
        ) -> Result<(), String> {
            self.calls.borrow_mut().push((text.to_owned(), prosody));
            if text.contains("FAIL") {
                return Err("synthesis failed".to_owned());
            }
            let samples: Vec<i16> = (0..text.chars().count() as i16).collect();
            let (first, second) = samples.split_at(samples.len() / 2);
            if chunk(first) {
                chunk(second);
            }
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeSite {
        written: RefCell<Vec<u8>>,
        writes: Cell<usize>,
        events: RefCell<Vec<Event>>,
        /// Abort once this many `actions` calls have happened.
        abort_after: Option<usize>,
        checks: Cell<usize>,
        skip: Cell<Option<i32>>,
        completed_skips: RefCell<Vec<i32>>,
        rate: i32,
    }

    impl Site for FakeSite {
        fn actions(&self) -> Actions {
            self.checks.set(self.checks.get() + 1);
            Actions {
                abort: self
                    .abort_after
                    .is_some_and(|after| self.checks.get() > after),
                skip: self.skip.get().is_some(),
            }
        }

        fn rate(&self) -> i32 {
            self.rate
        }

        fn volume(&self) -> u16 {
            100
        }

        fn skip_request(&self) -> i32 {
            self.skip.take().unwrap_or(0)
        }

        fn complete_skip(&self, skipped: i32) {
            self.completed_skips.borrow_mut().push(skipped);
        }

        fn write(&self, audio: &[u8]) -> Result<(), String> {
            self.writes.set(self.writes.get() + 1);
            self.written.borrow_mut().extend_from_slice(audio);
            Ok(())
        }

        fn add_events(&self, events: &[Event]) -> Result<(), String> {
            self.events.borrow_mut().extend_from_slice(events);
            Ok(())
        }
    }

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

    fn samples(site: &FakeSite) -> Vec<i16> {
        site.written
            .borrow()
            .chunks(2)
            .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
            .collect()
    }

    #[test]
    fn sentences_are_synthesized_one_at_a_time_and_written_in_order() {
        let synth = FakeSynth::new();
        let site = FakeSite::default();
        let steps = plan(&[fragment(Action::Speak, "Rojbaş. Baş im.", 0)]);
        assert_eq!(speak(&steps, &synth, &site), Ok(Outcome::Completed));
        let texts: Vec<String> = synth
            .calls
            .borrow()
            .iter()
            .map(|(t, _)| t.clone())
            .collect();
        assert_eq!(texts, ["Rojbaş.", "Baş im."]);
        let expected: Vec<i16> = (0..7).chain(0..7).collect();
        assert_eq!(samples(&site), expected);
    }

    #[test]
    fn word_and_sentence_events_point_into_the_written_audio() {
        let synth = FakeSynth::new();
        let site = FakeSite::default();
        let steps = plan(&[fragment(Action::Speak, "ab cd. ef", 5)]);
        speak(&steps, &synth, &site).expect("speak");
        // "ab cd." is 6 samples (12 bytes); "ef" follows at byte 12.
        assert_eq!(
            *site.events.borrow(),
            [
                Event::Sentence {
                    audio_offset: 0,
                    offset: 5,
                    len: 6
                },
                Event::Word {
                    audio_offset: 0,
                    offset: 5,
                    len: 2
                },
                Event::Word {
                    audio_offset: 6,
                    offset: 8,
                    len: 3
                },
                Event::Sentence {
                    audio_offset: 12,
                    offset: 12,
                    len: 2
                },
                Event::Word {
                    audio_offset: 12,
                    offset: 12,
                    len: 2
                },
            ]
        );
    }

    #[test]
    fn bookmarks_fire_where_the_audio_before_them_ends_and_silence_is_written() {
        let synth = FakeSynth::new();
        let site = FakeSite::default();
        let mut silence = fragment(Action::Silence, "", 0);
        silence.silence_ms = 100;
        let steps = plan(&[
            fragment(Action::Speak, "abcd", 0),
            fragment(Action::Bookmark, "42", 0),
            silence,
        ]);
        speak(&steps, &synth, &site).expect("speak");
        assert!(site.events.borrow().contains(&Event::Bookmark {
            audio_offset: 8,
            name: "42".to_owned()
        }));
        // 4 samples of speech, then 100 ms of silence at 1000 Hz.
        assert_eq!(samples(&site).len(), 104);
        assert!(samples(&site).iter().skip(4).all(|sample| *sample == 0));
    }

    #[test]
    fn an_abort_before_speaking_writes_nothing() {
        let synth = FakeSynth::new();
        let site = FakeSite {
            abort_after: Some(0),
            ..FakeSite::default()
        };
        let steps = plan(&[fragment(Action::Speak, "Rojbaş. Baş im.", 0)]);
        assert_eq!(speak(&steps, &synth, &site), Ok(Outcome::Aborted));
        assert!(site.written.borrow().is_empty());
        assert!(synth.calls.borrow().is_empty());
    }

    #[test]
    fn an_abort_during_synthesis_stops_before_the_next_chunk_and_writes_nothing() {
        let synth = FakeSynth::new();
        // The first check is the step's own; the second is after the first chunk.
        let site = FakeSite {
            abort_after: Some(1),
            ..FakeSite::default()
        };
        let steps = plan(&[fragment(Action::Speak, "Rojbaş. Baş im.", 0)]);
        assert_eq!(speak(&steps, &synth, &site), Ok(Outcome::Aborted));
        assert!(site.written.borrow().is_empty());
        assert_eq!(synth.calls.borrow().len(), 1);
    }

    #[test]
    fn an_abort_while_writing_stops_within_one_slice() {
        let synth = FakeSynth::new();
        let mut silence = fragment(Action::Silence, "", 0);
        silence.silence_ms = 1000;
        // One check for the step, then one per slice: abort after three slices.
        let site = FakeSite {
            abort_after: Some(4),
            ..FakeSite::default()
        };
        assert_eq!(
            speak(&plan(&[silence]), &synth, &site),
            Ok(Outcome::Aborted)
        );
        let slice = (RATE * SLICE_MS / 1000) as usize;
        assert_eq!(site.writes.get(), 3);
        assert_eq!(samples(&site).len(), 3 * slice);
    }

    #[test]
    fn the_callers_rate_reaches_the_engine() {
        let synth = FakeSynth::new();
        let site = FakeSite {
            rate: 10,
            ..FakeSite::default()
        };
        speak(&plan(&[fragment(Action::Speak, "Zû", 0)]), &synth, &site).expect("speak");
        assert_eq!(
            synth
                .calls
                .borrow()
                .first()
                .map(|(_, prosody)| prosody.rate),
            Some(Some(50))
        );
    }

    #[test]
    fn synthesis_errors_are_returned() {
        let synth = FakeSynth::new();
        let site = FakeSite::default();
        let steps = plan(&[fragment(Action::Speak, "FAIL", 0)]);
        assert_eq!(
            speak(&steps, &synth, &site),
            Err("synthesis failed".to_owned())
        );
    }

    #[test]
    fn skipping_forward_drops_sentences_and_reports_the_count() {
        let synth = FakeSynth::new();
        let site = FakeSite::default();
        site.skip.set(Some(2));
        let steps = plan(&[fragment(Action::Speak, "Yek. Du. Sê.", 0)]);
        speak(&steps, &synth, &site).expect("speak");
        let texts: Vec<String> = synth
            .calls
            .borrow()
            .iter()
            .map(|(t, _)| t.clone())
            .collect();
        assert_eq!(texts, ["Sê."]);
        assert_eq!(*site.completed_skips.borrow(), [2]);
    }

    #[test]
    fn skipping_past_the_end_finishes_and_reports_what_was_there() {
        let synth = FakeSynth::new();
        let site = FakeSite::default();
        site.skip.set(Some(9));
        let steps = plan(&[fragment(Action::Speak, "Yek. Du.", 0)]);
        assert_eq!(speak(&steps, &synth, &site), Ok(Outcome::Completed));
        assert!(synth.calls.borrow().is_empty());
        assert_eq!(*site.completed_skips.borrow(), [2]);
    }

    #[test]
    fn skipping_back_returns_to_an_earlier_sentence() {
        let steps = plan(&[fragment(Action::Speak, "Yek. Du. Sê.", 0)]);
        let site = FakeSite::default();
        site.skip.set(Some(-1));
        assert_eq!(skip(&steps, 2, &site), 1);
        site.skip.set(Some(-5));
        assert_eq!(skip(&steps, 2, &site), 0);
        assert_eq!(*site.completed_skips.borrow(), [-1, -2]);
    }
}
