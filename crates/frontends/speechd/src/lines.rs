use std::collections::VecDeque;
use std::io::{self, BufRead};
use std::sync::mpsc::{self, Receiver};
use std::thread;

use crate::protocol::{self, Command};

type Item = io::Result<String>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Interrupt {
    Stop,
    Pause,
    Quit,
}

impl Interrupt {
    pub(crate) fn event(self) -> &'static str {
        match self {
            Interrupt::Stop | Interrupt::Quit => "703 STOP",
            Interrupt::Pause => "704 PAUSE",
        }
    }
}

pub(crate) struct Lines {
    receiver: Receiver<Item>,
    queued: VecDeque<Item>,
}

impl Lines {
    pub(crate) fn new(receiver: Receiver<Item>) -> Self {
        Self {
            receiver,
            queued: VecDeque::new(),
        }
    }

    pub(crate) fn spawn(mut input: impl BufRead + Send + 'static) -> io::Result<Self> {
        let (sender, receiver) = mpsc::channel();
        thread::Builder::new()
            .name("sd_dengjen-stdin".to_owned())
            .spawn(move || loop {
                let item = match protocol::read_line(&mut input) {
                    Ok(Some(line)) => Ok(line),
                    Ok(None) => break,
                    Err(error) => Err(error),
                };
                let failed = item.is_err();
                if sender.send(item).is_err() || failed {
                    break;
                }
            })?;
        Ok(Self::new(receiver))
    }

    pub(crate) fn next_line(&mut self) -> io::Result<Option<String>> {
        match self.queued.pop_front() {
            Some(item) => item.map(Some),
            None => self.receiver.recv().map_or(Ok(None), |item| item.map(Some)),
        }
    }

    /// Consumes a queued `STOP` or `PAUSE`; leaves `QUIT` queued for the main loop.
    pub(crate) fn interrupt(&mut self) -> Option<Interrupt> {
        self.queued.extend(self.receiver.try_iter());
        let (index, interrupt) = find_interrupt(&self.queued)?;
        if interrupt != Interrupt::Quit {
            self.queued.remove(index);
        }
        Some(interrupt)
    }
}

/// Skips data blocks, so a spoken line reading `STOP` is not a command.
fn find_interrupt(queued: &VecDeque<Item>) -> Option<(usize, Interrupt)> {
    let mut in_block = false;
    for (index, item) in queued.iter().enumerate() {
        let Ok(line) = item else { return None };
        if in_block {
            in_block = line != ".";
            continue;
        }
        match protocol::parse_command(line) {
            Command::Stop => return Some((index, Interrupt::Stop)),
            Command::Pause => return Some((index, Interrupt::Pause)),
            Command::Quit => return Some((index, Interrupt::Quit)),
            Command::Speak(_) | Command::Set | Command::Audio | Command::Loglevel => {
                in_block = true;
            }
            _ => {}
        }
    }
    None
}
