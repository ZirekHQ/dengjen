//! The speech thread: synthesizes sentence by sentence, keeps one sentence queued ahead so
//! there is no gap between them, and reports which sentence is audible.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

use async_channel::Sender;
use rodio::buffer::SamplesBuffer;
use rodio::{OutputStream, Sink};

use crate::engine::{Engine, Voice};

pub enum Command {
    /// Read `sentences` from index `start`.
    Play {
        sentences: Vec<String>,
        start: usize,
        voice: Voice,
    },
    Pause,
    Resume,
    Stop,
    /// Write all of `text` to a WAV file.
    Save {
        text: String,
        voice: Voice,
        path: PathBuf,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    /// The sentence at this index started playing.
    Started(usize),
    Finished,
    Saved(PathBuf),
    Failed(String),
}

const TICK: Duration = Duration::from_millis(30);

/// The sentence being heard: `queued` lists the sentences handed to the player in order, and
/// `remaining` is how many of them have not finished.
pub fn audible(queued: &VecDeque<usize>, remaining: usize) -> Option<usize> {
    queued
        .len()
        .checked_sub(remaining)
        .filter(|_| remaining > 0)
        .and_then(|index| queued.get(index).copied())
}

#[derive(Default)]
struct Reading {
    sentences: Vec<String>,
    voice: Option<Voice>,
    next: usize,
    queued: VecDeque<usize>,
    reported: Option<usize>,
}

impl Reading {
    fn active(&self) -> bool {
        self.voice.is_some()
    }
}

/// Runs until the command channel closes. Events go to `events`; a send error means the
/// window is gone, so the thread stops.
pub fn run(engine: &mut Engine, commands: &Receiver<Command>, events: &Sender<Event>) {
    let (_stream, handle) = match OutputStream::try_default() {
        Ok(output) => output,
        Err(error) => {
            let _ = events.send_blocking(Event::Failed(format!("no audio output: {error}")));
            return;
        }
    };
    let sink = match Sink::try_new(&handle) {
        Ok(sink) => sink,
        Err(error) => {
            let _ = events.send_blocking(Event::Failed(format!("no audio output: {error}")));
            return;
        }
    };
    let mut reading = Reading::default();
    loop {
        match commands.recv_timeout(TICK) {
            Ok(command) => {
                if !handle_command(command, engine, &sink, &mut reading, events) {
                    return;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        if !tick(engine, &sink, &mut reading, events) {
            return;
        }
    }
}

fn reset(sink: &Sink, reading: &mut Reading) {
    sink.clear();
    sink.play();
    *reading = Reading::default();
}

fn handle_command(
    command: Command,
    engine: &mut Engine,
    sink: &Sink,
    reading: &mut Reading,
    events: &Sender<Event>,
) -> bool {
    match command {
        Command::Play {
            sentences,
            start,
            voice,
        } => {
            reset(sink, reading);
            reading.sentences = sentences;
            reading.next = start;
            reading.voice = Some(voice);
            true
        }
        Command::Pause => {
            sink.pause();
            true
        }
        Command::Resume => {
            sink.play();
            true
        }
        Command::Stop => {
            reset(sink, reading);
            true
        }
        Command::Save { text, voice, path } => {
            let event = match engine.save(&voice, &text, &path) {
                Ok(()) => Event::Saved(path),
                Err(error) => Event::Failed(error),
            };
            events.send_blocking(event).is_ok()
        }
    }
}

fn tick(engine: &mut Engine, sink: &Sink, reading: &mut Reading, events: &Sender<Event>) -> bool {
    if !reading.active() {
        return true;
    }
    if sink.len() <= 1 && reading.next < reading.sentences.len() {
        let index = reading.next;
        reading.next += 1;
        let clip = reading
            .voice
            .as_ref()
            .zip(reading.sentences.get(index))
            .map(|(voice, text)| engine.speak(voice, text));
        match clip {
            Some(Ok(clip)) => {
                sink.append(SamplesBuffer::new(1, clip.sample_rate, clip.samples));
                reading.queued.push_back(index);
            }
            Some(Err(error)) => {
                reset(sink, reading);
                return events.send_blocking(Event::Failed(error)).is_ok();
            }
            None => {}
        }
    }
    let heard = audible(&reading.queued, sink.len());
    if heard.is_some() && heard != reading.reported {
        reading.reported = heard;
        if let Some(index) = heard {
            if events.send_blocking(Event::Started(index)).is_err() {
                return false;
            }
        }
    }
    if sink.empty() && reading.next >= reading.sentences.len() {
        *reading = Reading::default();
        return events.send_blocking(Event::Finished).is_ok();
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_audible_sentence_follows_the_player_queue() {
        let queued: VecDeque<usize> = [3, 4, 5].into();
        assert_eq!(audible(&queued, 3), Some(3));
        assert_eq!(audible(&queued, 2), Some(4));
        assert_eq!(audible(&queued, 1), Some(5));
        assert_eq!(audible(&queued, 0), None);
        assert_eq!(audible(&queued, 4), None);
        assert_eq!(audible(&VecDeque::new(), 0), None);
    }
}
