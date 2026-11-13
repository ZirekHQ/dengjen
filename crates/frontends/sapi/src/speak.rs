//! The body of `ISpTTSEngine::Speak`, written against two small traits so it runs in tests.
//!
//! SAPI calls `Speak` once per request and keeps the call open until all audio is written. The
//! caller cancels by setting `SPVES_ABORT` and skips by setting `SPVES_SKIP`, which the engine
//! must poll; screen readers cancel on every key press. A sentence is synthesized on a worker
//! thread while this thread polls every [`POLL`], and an abort or skip interrupts the engine's
//! inference rather than waiting for the sentence to finish. Written audio is checked a slice at
//! a time, so no more than [`SLICE_MS`] goes out after either request.
#![forbid(unsafe_code)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use crate::fragments::{Step, Utterance};
use crate::prosody::{engine_prosody, EngineProsody};
use crate::timing::word_starts;

/// How much audio goes to SAPI between two checks of the caller's actions.
pub const SLICE_MS: u32 = 40;
/// How often the caller's actions are checked while a sentence is being synthesized.
pub const POLL: Duration = Duration::from_millis(5);
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

/// The SAPI side: `ISpTTSEngineSite`. Only the thread that called [`speak`] uses it.
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

/// The engine side: one loaded voice producing mono 16-bit samples. [`Synth::synthesize`] runs
/// on a worker thread while [`Synth::interrupt`] is called from the thread that called [`speak`].
pub trait Synth: Sync {
    fn sample_rate(&self) -> u32;
    /// Synthesizes `text`, handing each chunk to `chunk` until it returns `false`.
    fn synthesize(
        &self,
        text: &str,
        prosody: EngineProsody,
        chunk: &mut dyn FnMut(&[i16]) -> bool,
    ) -> Result<(), String>;
    /// Stops a [`Synth::synthesize`] call running on another thread as soon as the engine can.
    /// That call may then fail; its result is discarded.
    fn interrupt(&self);
    /// The voice's language code, such as `ku_TR`, or an empty string when it is unknown.
    fn language(&self) -> &str;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Completed,
    Aborted,
}

/// How one step ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ended {
    Completed,
    Aborted,
    /// The caller asked to skip while the step was playing.
    Skipped,
}

/// Why a step stops early, if the caller asked it to.
fn interrupted(actions: Actions) -> Option<Ended> {
    if actions.abort {
        Some(Ended::Aborted)
    } else if actions.skip {
        Some(Ended::Skipped)
    } else {
        None
    }
}

/// Audio on its way to the caller, with the byte count that event offsets are measured in.
struct Output<'a> {
    site: &'a dyn Site,
    written: u64,
    slice_samples: usize,
}

impl Output<'_> {
    /// Whether the caller has asked to stop speaking.
    fn aborted(&self) -> bool {
        self.site.actions().abort
    }

    /// Writes samples a slice at a time, stopping at the first abort or skip.
    fn write(&mut self, samples: &[i16]) -> Result<Ended, String> {
        for slice in samples.chunks(self.slice_samples) {
            if let Some(ended) = interrupted(self.site.actions()) {
                return Ok(ended);
            }
            let bytes: Vec<u8> = slice
                .iter()
                .flat_map(|sample| sample.to_le_bytes())
                .collect();
            if let Err(error) = self.site.write(&bytes) {
                // SAPI refuses audio once the caller has aborted, and the abort can land between
                // the check above and this write. That is the caller's request, not a failure: a
                // host such as Narrator treats a failed `Speak` as a broken voice and switches away.
                let aborted = self.aborted();
                crate::log::line(|| format!("write failed ({error}); abort requested: {aborted}"));
                return if aborted {
                    Ok(Ended::Aborted)
                } else {
                    Err(error)
                };
            }
            self.written += slice.len() as u64 * BYTES_PER_SAMPLE;
        }
        Ok(Ended::Completed)
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
    let outcome = run(steps, synth, site, &mut output);
    crate::log::line(|| format!("speak: {outcome:?}, {} bytes written", output.written));
    outcome
}

/// Plays `steps` in order, handling skips between and during steps; [`speak`] logs the outcome.
fn run(
    steps: &[Step],
    synth: &dyn Synth,
    site: &dyn Site,
    output: &mut Output<'_>,
) -> Result<Outcome, String> {
    let sample_rate = synth.sample_rate().max(1);
    let mut index = 0;
    while let Some(step) = steps.get(index) {
        let actions = site.actions();
        if actions.abort {
            return Ok(Outcome::Aborted);
        }
        if actions.skip {
            index = skip(steps, index, false, site);
            continue;
        }
        let ended = match step {
            Step::Speak(utterance) => {
                crate::log::line(|| {
                    let text: String = utterance.text.chars().take(40).collect();
                    format!(
                        "step: speak {text:?} ({} chars)",
                        utterance.text.chars().count()
                    )
                });
                speak_utterance(utterance, synth, output)?
            }
            Step::Silence { ms } => {
                crate::log::line(|| format!("step: silence {ms} ms"));
                let samples = (u64::from(*ms) * u64::from(sample_rate) / 1000) as usize;
                output.write(&vec![0; samples])?
            }
            Step::Bookmark { name } => {
                crate::log::line(|| format!("step: bookmark {name:?}"));
                site.add_events(&[Event::Bookmark {
                    audio_offset: output.written,
                    name: name.clone(),
                }])?;
                Ended::Completed
            }
        };
        index = match ended {
            Ended::Completed => index + 1,
            Ended::Aborted => return Ok(Outcome::Aborted),
            Ended::Skipped => skip(steps, index, true, site),
        };
    }
    Ok(Outcome::Completed)
}

/// Synthesizes one sentence, queues its word and sentence events, and writes its audio.
///
/// Ends early with [`Ended::Aborted`] or [`Ended::Skipped`] when the caller asks during
/// synthesis or playback.
fn speak_utterance(
    utterance: &Utterance,
    synth: &dyn Synth,
    output: &mut Output<'_>,
) -> Result<Ended, String> {
    let site = output.site;
    let prosody = engine_prosody(site.rate(), site.volume(), utterance.prosody);
    crate::log::line(|| format!("prosody: {prosody:?}"));
    // Only the text sent to the engine changes; word events still point into the caller's text.
    let spoken = if crate::english::applies_to(synth.language()) {
        crate::english::respell(&utterance.text)
    } else {
        crate::english::Respelled::unchanged(&utterance.text)
    };
    if *spoken.text != *utterance.text {
        crate::log::line(|| format!("respelled: {:?}", spoken.text));
    }
    let samples = match synthesize(&spoken.text, prosody, synth, site)? {
        Synthesized::Audio(samples) => samples,
        Synthesized::Stopped(ended) => return Ok(ended),
    };
    let start = output.written;
    let mut events = vec![Event::Sentence {
        audio_offset: start,
        offset: utterance.offset,
        len: utterance.len,
    }];
    events.extend(
        word_starts(utterance, &spoken, samples.len())
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

enum Synthesized {
    Audio(Vec<i16>),
    /// The caller aborted or skipped before the sentence was ready.
    Stopped(Ended),
}

/// Synthesizes one sentence on a worker thread while this thread watches the caller's actions.
///
/// An ONNX inference cannot be split, so a sentence used to finish (100 to 300 ms) before an
/// abort was seen. Polling here and interrupting the engine stops it within a few milliseconds.
/// The site is a COM object handed to this thread, so only this thread calls it.
fn synthesize(
    text: &str,
    prosody: EngineProsody,
    synth: &dyn Synth,
    site: &dyn Site,
) -> Result<Synthesized, String> {
    let stop = AtomicBool::new(false);
    let started = std::time::Instant::now();
    std::thread::scope(|scope| {
        let (sender, receiver) = mpsc::channel::<Vec<i16>>();
        let stop = &stop;
        let worker = scope.spawn(move || {
            synth.synthesize(text, prosody, &mut |chunk| {
                sender.send(chunk.to_vec()).is_ok() && !stop.load(Ordering::Relaxed)
            })
        });
        let mut samples = Vec::new();
        let mut first_audio = None;
        loop {
            match receiver.recv_timeout(POLL) {
                Ok(chunk) => {
                    first_audio.get_or_insert_with(|| started.elapsed());
                    samples.extend_from_slice(&chunk);
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            if let Some(ended) = interrupted(site.actions()) {
                let seen = started.elapsed();
                stop.store(true, Ordering::Relaxed);
                // The request can arrive before the engine has started its inference, which
                // would then run to the end, so keep asking until the worker returns.
                while !worker.is_finished() {
                    synth.interrupt();
                    std::thread::sleep(Duration::from_millis(1));
                }
                // The worker's result is the interruption's error, or audio nobody wants.
                let _ = worker.join();
                // How long the engine took to stop, apart from how long it ran before the request.
                crate::log::line(|| {
                    format!(
                        "synthesis {ended:?} after {seen:?}, stopped {:?} later",
                        started.elapsed().saturating_sub(seen)
                    )
                });
                return Ok(Synthesized::Stopped(ended));
            }
        }
        crate::log::line(|| {
            format!(
                "first audio after {first_audio:?}, done after {:?}",
                started.elapsed()
            )
        });
        match worker.join() {
            Ok(result) => result.map(|()| Synthesized::Audio(samples)),
            Err(_) => Err("the synthesis thread panicked".to_owned()),
        }
    })
}

/// The leading integer of a bookmark's name, which SAPI hands clients in `wParam` as `_wtol` reads
/// it: an optional sign, then digits, and 0 when there are none.
pub fn bookmark_number(name: &str) -> isize {
    let trimmed = name.trim();
    let (negative, unsigned) = trimmed
        .strip_prefix('-')
        .map_or((false, trimmed), |rest| (true, rest));
    let digits: String = unsigned.chars().take_while(char::is_ascii_digit).collect();
    let magnitude = digits.parse::<isize>().unwrap_or(0);
    if negative {
        -magnitude
    } else {
        magnitude
    }
}

/// Moves past (or back over) the sentences the caller asked to skip, and reports how many.
///
/// `during` says the request arrived while step `index` was playing rather than before it
/// started. Forward, the step at `index` counts as the first one skipped either way. Backward,
/// a sentence that was playing is replayed by a skip of one, while between steps a skip of one
/// returns to the sentence before `index`.
fn skip(steps: &[Step], index: usize, during: bool, site: &dyn Site) -> usize {
    let wanted = site.skip_request();
    let sentences = |range: &mut dyn Iterator<Item = usize>| -> Vec<usize> {
        range
            .filter(|&at| matches!(steps.get(at), Some(Step::Speak(_))))
            .collect()
    };
    let (target, skipped) = match wanted {
        // The caller withdrew the request; a step it interrupted plays again from its start.
        0 => (index, 0),
        forward if forward > 0 => {
            let ahead = sentences(&mut (index..steps.len()));
            let count = ahead.len().min(forward.unsigned_abs() as usize);
            let target = ahead.get(count).copied().unwrap_or(steps.len());
            (target, count as i32)
        }
        backward => {
            let end = if during { index + 1 } else { index };
            let behind = sentences(&mut (0..end).rev());
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

/// How many bytes of `offered` audio a successful `ISpTTSEngineSite::Write` took. SAPI's own
/// site reports the count, but System.Speech's managed host (and so every .NET app that speaks
/// through `SpeechSynthesizer`) returns `S_OK` without setting it; success means the whole
/// buffer was taken, so a zero count is read as all of it rather than as a refusal.
pub fn bytes_taken(offered: u32, reported: u32) -> u32 {
    match reported {
        0 => offered,
        reported => reported.min(offered),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_successful_write_with_no_count_took_the_whole_buffer() {
        assert_eq!(bytes_taken(4096, 4096), 4096);
        assert_eq!(bytes_taken(4096, 1024), 1024);
        assert_eq!(bytes_taken(4096, 0), 4096);
        assert_eq!(bytes_taken(4096, 9000), 4096);
    }

    #[test]
    fn a_bookmark_number_is_the_leading_integer_of_its_name() {
        assert_eq!(bookmark_number("7"), 7);
        assert_eq!(bookmark_number(" 12abc"), 12);
        assert_eq!(bookmark_number("-3"), -3);
        // A minus sign counts only at the start, as with `_wtol`.
        assert_eq!(bookmark_number("1-2"), 1);
        assert_eq!(bookmark_number("END12"), 0);
        assert_eq!(bookmark_number(""), 0);
    }
    use crate::fragments::{plan, Action, Fragment, FragmentProsody};
    use std::cell::{Cell, RefCell};
    use std::sync::Mutex;
    use std::time::Instant;

    const RATE: u32 = 1000;

    /// Produces one sample per character, in two chunks, each sample its character's index.
    struct FakeSynth {
        calls: Mutex<Vec<(String, EngineProsody)>>,
    }

    impl FakeSynth {
        fn new() -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<(String, EngineProsody)> {
            self.calls.lock().expect("calls").clone()
        }

        fn texts(&self) -> Vec<String> {
            self.calls().into_iter().map(|(text, _)| text).collect()
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
            self.calls
                .lock()
                .expect("calls")
                .push((text.to_owned(), prosody));
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

        fn interrupt(&self) {}

        fn language(&self) -> &str {
            "ku"
        }
    }

    #[derive(Default)]
    struct FakeSite {
        written: RefCell<Vec<u8>>,
        writes: Cell<usize>,
        events: RefCell<Vec<Event>>,
        /// Abort once this many `actions` calls have happened.
        abort_after: Option<usize>,
        /// Every write fails, and the caller's abort is raised by the first one.
        abort_during_write: Cell<bool>,
        aborting: Cell<bool>,
        checks: Cell<usize>,
        skip: Cell<Option<i32>>,
        /// Requests a skip of this many sentences once the first slice has been written.
        skip_after_first_write: Cell<Option<i32>>,
        completed_skips: RefCell<Vec<i32>>,
        rate: i32,
    }

    impl Site for FakeSite {
        fn actions(&self) -> Actions {
            self.checks.set(self.checks.get() + 1);
            Actions {
                abort: self.aborting.get()
                    || self
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
            if self.abort_during_write.get() {
                self.aborting.set(true);
                return Err("the caller aborted".to_owned());
            }
            if let Some(count) = self.skip_after_first_write.take() {
                self.skip.set(Some(count));
            }
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
        let texts = synth.texts();
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
        assert!(synth.calls().is_empty());
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
        assert_eq!(synth.calls().len(), 1);
    }

    #[test]
    fn a_write_that_fails_because_the_caller_aborted_is_an_abort_not_an_error() {
        let synth = FakeSynth::new();
        let site = FakeSite::default();
        site.abort_during_write.set(true);
        let steps = plan(&[fragment(Action::Speak, "hello there", 0)]);
        assert_eq!(speak(&steps, &synth, &site), Ok(Outcome::Aborted));
    }

    #[test]
    fn a_write_that_fails_without_an_abort_is_still_an_error() {
        struct BrokenSite(FakeSite);
        impl Site for BrokenSite {
            fn actions(&self) -> Actions {
                Actions::default()
            }
            fn rate(&self) -> i32 {
                0
            }
            fn volume(&self) -> u16 {
                100
            }
            fn skip_request(&self) -> i32 {
                0
            }
            fn complete_skip(&self, _skipped: i32) {}
            fn write(&self, _audio: &[u8]) -> Result<(), String> {
                Err("the audio sink is gone".to_owned())
            }
            fn add_events(&self, events: &[Event]) -> Result<(), String> {
                self.0.add_events(events)
            }
        }
        let steps = plan(&[fragment(Action::Speak, "hello there", 0)]);
        assert_eq!(
            speak(&steps, &FakeSynth::new(), &BrokenSite(FakeSite::default())),
            Err("the audio sink is gone".to_owned())
        );
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
            synth.calls().first().map(|(_, prosody)| prosody.rate),
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
        let texts = synth.texts();
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
        assert!(synth.calls().is_empty());
        assert_eq!(*site.completed_skips.borrow(), [2]);
    }

    #[test]
    fn skipping_back_returns_to_an_earlier_sentence() {
        let steps = plan(&[fragment(Action::Speak, "Yek. Du. Sê.", 0)]);
        let site = FakeSite::default();
        site.skip.set(Some(-1));
        assert_eq!(skip(&steps, 2, false, &site), 1);
        site.skip.set(Some(-5));
        assert_eq!(skip(&steps, 2, false, &site), 0);
        assert_eq!(*site.completed_skips.borrow(), [-1, -2]);
    }

    /// Blocks like an ONNX inference until it is interrupted, then fails like one.
    #[derive(Default)]
    struct BlockingSynth {
        interrupted: AtomicBool,
    }

    impl Synth for BlockingSynth {
        fn sample_rate(&self) -> u32 {
            RATE
        }

        /// Waits for `interrupt`, giving up after a few seconds so a broken test cannot hang.
        fn synthesize(
            &self,
            _text: &str,
            _prosody: EngineProsody,
            chunk: &mut dyn FnMut(&[i16]) -> bool,
        ) -> Result<(), String> {
            let started = Instant::now();
            while !self.interrupted.load(Ordering::Relaxed) {
                if started.elapsed() > Duration::from_secs(10) {
                    chunk(&[1; 100]);
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            Err("terminated".to_owned())
        }

        fn interrupt(&self) {
            self.interrupted.store(true, Ordering::Relaxed);
        }

        fn language(&self) -> &str {
            "ku"
        }
    }

    #[test]
    fn an_abort_during_an_inference_interrupts_it_instead_of_waiting() {
        let synth = BlockingSynth::default();
        // The step's own check passes; the first poll during synthesis sees the abort.
        let site = FakeSite {
            abort_after: Some(1),
            ..FakeSite::default()
        };
        let started = Instant::now();
        let steps = plan(&[fragment(Action::Speak, "Rojbaş.", 0)]);
        assert_eq!(speak(&steps, &synth, &site), Ok(Outcome::Aborted));
        assert!(synth.interrupted.load(Ordering::Relaxed));
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(site.written.borrow().is_empty());
    }

    #[test]
    fn a_skip_during_an_inference_interrupts_it_and_moves_on() {
        let synth = BlockingSynth::default();
        let site = FakeSite::default();
        site.skip.set(Some(1));
        // The request is seen only once synthesis has started, not by the step's own check.
        let site = SkipOnPoll {
            inner: site,
            polls: Cell::new(0),
        };
        let steps = plan(&[fragment(Action::Speak, "Yek.", 0)]);
        assert_eq!(speak(&steps, &synth, &site), Ok(Outcome::Completed));
        assert!(synth.interrupted.load(Ordering::Relaxed));
        assert_eq!(*site.inner.completed_skips.borrow(), [1]);
    }

    /// Hides a pending skip from the first `actions` check.
    struct SkipOnPoll {
        inner: FakeSite,
        polls: Cell<usize>,
    }

    impl Site for SkipOnPoll {
        /// Reports no skip on the first call and the inner site's actions after that.
        fn actions(&self) -> Actions {
            self.polls.set(self.polls.get() + 1);
            let actions = self.inner.actions();
            Actions {
                skip: actions.skip && self.polls.get() > 1,
                ..actions
            }
        }
        fn rate(&self) -> i32 {
            0
        }
        fn volume(&self) -> u16 {
            100
        }
        fn skip_request(&self) -> i32 {
            self.inner.skip_request()
        }
        fn complete_skip(&self, skipped: i32) {
            self.inner.complete_skip(skipped);
        }
        fn write(&self, audio: &[u8]) -> Result<(), String> {
            self.inner.write(audio)
        }
        fn add_events(&self, events: &[Event]) -> Result<(), String> {
            self.inner.add_events(events)
        }
    }

    /// Three sentences, the first longer than one slice of audio.
    fn three_sentences() -> Vec<Step> {
        let long = "Yek ".repeat(20);
        plan(&[fragment(Action::Speak, &format!("{}yek. Du. Sê.", long), 0)])
    }

    #[test]
    fn a_skip_forward_while_a_sentence_plays_moves_to_the_next_one() {
        let synth = FakeSynth::new();
        let site = FakeSite::default();
        site.skip_after_first_write.set(Some(1));
        let steps = three_sentences();
        assert_eq!(speak(&steps, &synth, &site), Ok(Outcome::Completed));
        let texts = synth.texts();
        assert_eq!(
            texts.get(1..),
            Some(&["Du.".to_owned(), "Sê.".to_owned()][..])
        );
        let slice = (RATE * SLICE_MS / 1000) as usize;
        // One slice of the first sentence, then all of the second and third.
        assert_eq!(samples(&site).len(), slice + 3 + 3);
        assert_eq!(*site.completed_skips.borrow(), [1]);
    }

    #[test]
    fn a_skip_back_while_a_sentence_plays_replays_it() {
        let synth = FakeSynth::new();
        let site = FakeSite::default();
        site.skip_after_first_write.set(Some(-1));
        let steps = three_sentences();
        assert_eq!(speak(&steps, &synth, &site), Ok(Outcome::Completed));
        let texts = synth.texts();
        assert_eq!(texts.len(), 4);
        assert_eq!(texts.first(), texts.get(1));
        assert_eq!(*site.completed_skips.borrow(), [-1]);
    }

    #[test]
    fn a_skip_back_mid_sentence_counts_the_playing_sentence_first() {
        let steps = plan(&[fragment(Action::Speak, "Yek. Du. Sê.", 0)]);
        let site = FakeSite::default();
        site.skip.set(Some(-2));
        assert_eq!(skip(&steps, 2, true, &site), 1);
        assert_eq!(*site.completed_skips.borrow(), [-2]);
    }

    #[test]
    fn a_kurmanji_voice_hears_english_ui_words_respelled() {
        let synth = FakeSynth::new();
        let site = FakeSite::default();
        let steps = plan(&[fragment(Action::Speak, "Bişkok selected", 0)]);
        speak(&steps, &synth, &site).expect("speak");
        assert_eq!(synth.texts(), ["Bişkok silektid"]);
    }

    #[test]
    fn word_events_after_a_respelled_word_follow_the_spoken_text() {
        let synth = FakeSynth::new();
        let site = FakeSite::default();
        let steps = plan(&[fragment(Action::Speak, "checked a", 0)]);
        speak(&steps, &synth, &site).expect("speak");
        // "çekt a" is 6 samples and "a" is its sixth char, so it starts at sample 5 (byte 10),
        // while the event still points at "a" in the caller's text.
        assert_eq!(
            site.events.borrow().last(),
            Some(&Event::Word {
                audio_offset: 10,
                offset: 8,
                len: 1
            })
        );
    }
}
