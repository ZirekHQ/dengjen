use std::io::{self, BufRead, Write};

use crate::lines::{Interrupt, Lines};
use crate::protocol::{self, Command, Message};
use crate::reply;
use crate::settings::Settings;
use crate::ssml::{self, Piece};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoiceInfo {
    pub name: String,
    pub language: String,
    pub variant: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Audio {
    pub sample_rate: u32,
    pub samples: Vec<i16>,
}

pub type Chunks<'a> = Box<dyn Iterator<Item = Result<Audio, String>> + 'a>;

pub trait Backend {
    fn init(&mut self) -> Result<String, String>;
    fn voices(&self) -> Vec<VoiceInfo>;
    /// Starts synthesis and yields audio chunk by chunk, so playback can begin before the text ends.
    fn synthesize<'a>(&'a mut self, text: &str, settings: &Settings) -> Result<Chunks<'a>, String>;
}

#[derive(PartialEq, Eq)]
enum Flow {
    Continue,
    Quit,
}

struct Session<B, W> {
    backend: B,
    settings: Settings,
    input: Lines,
    output: W,
}

pub fn run<B: Backend>(
    backend: B,
    input: impl BufRead + Send + 'static,
    output: impl Write,
) -> io::Result<()> {
    run_lines(backend, Lines::spawn(input)?, output)
}

fn run_lines<B: Backend>(backend: B, input: Lines, output: impl Write) -> io::Result<()> {
    let mut session = Session {
        backend,
        settings: Settings::default(),
        input,
        output,
    };
    if !session.init()? {
        return Ok(());
    }
    while let Some(line) = session.input.next_line()? {
        let flow = session.dispatch(protocol::parse_command(&line))?;
        session.output.flush()?;
        if flow == Flow::Quit {
            break;
        }
    }
    Ok(())
}

impl<B: Backend, W: Write> Session<B, W> {
    fn say(&mut self, text: &str) -> io::Result<()> {
        reply::line(&mut self.output, text)
    }

    fn init(&mut self) -> io::Result<bool> {
        match self.input.next_line()?.as_deref() {
            Some("INIT") => {}
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "server did not start with INIT",
                ))
            }
        }
        let (code, status, message, ready) = match self.backend.init() {
            Ok(message) => ("299", "OK LOADED SUCCESSFULLY", message, true),
            Err(message) => ("399", "ERR CANT INIT MODULE", message, false),
        };
        self.say(&format!("{code}-{message}"))?;
        self.say(&format!("{code} {status}"))?;
        self.output.flush()?;
        Ok(ready)
    }

    fn dispatch(&mut self, command: Command) -> io::Result<Flow> {
        match command {
            Command::Speak(kind) => return self.speak(kind),
            Command::Set => return self.settings_block(Block::Settings),
            Command::Audio => return self.settings_block(Block::Audio),
            Command::Loglevel => return self.settings_block(Block::LogLevel),
            Command::Debug(line) => self.say(debug_reply(&line))?,
            Command::ListVoices { language, variant } => self.list_voices(language, variant)?,
            Command::Quit => {
                self.say("210 OK QUIT")?;
                return Ok(Flow::Quit);
            }
            Command::Stop | Command::Pause => {}
            Command::Unknown => self.say(reply::UNKNOWN)?,
        }
        Ok(Flow::Continue)
    }

    fn settings_block(&mut self, block: Block) -> io::Result<Flow> {
        self.say(block.opening())?;
        let Some(lines) = protocol::read_block(|| self.input.next_line())? else {
            return Ok(Flow::Quit);
        };
        let error = lines
            .iter()
            .fold(None, |error, line| match protocol::parse_assignment(line) {
                None => Some(reply::BAD_SYNTAX),
                Some((name, value)) if self.accepts(block, name, value) => error,
                Some(_) => Some(reply::BAD_PARAM),
            });
        self.say(error.unwrap_or_else(|| block.done()))?;
        Ok(Flow::Continue)
    }

    fn accepts(&mut self, block: Block, name: &str, value: &str) -> bool {
        match block {
            Block::Settings => self.settings.apply(name, value).is_ok(),
            Block::Audio => name == "audio_output_method" && value == "server",
            Block::LogLevel => name == "log_level" && value.parse::<i32>().is_ok(),
        }
    }

    fn list_voices(&mut self, language: Option<String>, variant: Option<String>) -> io::Result<()> {
        let wanted = |voice: &&VoiceInfo| {
            let lang_ok = language
                .as_deref()
                .is_none_or(|l| matches_language(l, &voice.language));
            let variant_ok = variant
                .as_deref()
                .is_none_or(|v| v.eq_ignore_ascii_case(&voice.variant));
            lang_ok && variant_ok
        };
        let voices = self.backend.voices();
        let rows: Vec<String> = voices
            .iter()
            .filter(wanted)
            .map(|v| format!("200-{}\t{}\t{}", v.name, v.language, v.variant))
            .collect();
        if rows.is_empty() {
            return self.say("304 CANT LIST VOICES");
        }
        rows.iter().try_for_each(|row| self.say(row))?;
        self.say("200 OK VOICE LIST SENT")
    }

    fn speak(&mut self, kind: Message) -> io::Result<Flow> {
        self.say("202 OK RECEIVING MESSAGE")?;
        let Some(lines) = protocol::read_block(|| self.input.next_line())? else {
            return Ok(Flow::Quit);
        };
        match message_pieces(kind, &lines) {
            Err(error) => self.say(error)?,
            Ok(pieces) => self.speak_pieces(pieces)?,
        }
        Ok(Flow::Continue)
    }

    fn speak_pieces(&mut self, pieces: Vec<Piece>) -> io::Result<()> {
        let mut started = false;
        let mut ending = "702 END";
        for piece in pieces {
            match self.play(piece, &mut started)? {
                Played::Continue => {}
                Played::Interrupted(interrupt) => {
                    ending = interrupt.event();
                    break;
                }
                Played::Failed => break,
            }
        }
        self.say(if started { ending } else { reply::CANT_SPEAK })
    }

    fn play(&mut self, piece: Piece, started: &mut bool) -> io::Result<Played> {
        match piece {
            Piece::Mark(name) => {
                begin(&mut self.output, started)?;
                match self.input.interrupt() {
                    Some(interrupt) => Ok(Played::Interrupted(interrupt)),
                    None => send_mark(&mut self.output, &name).map(|()| Played::Continue),
                }
            }
            Piece::Text(text) => match self.backend.synthesize(&text, &self.settings) {
                Ok(chunks) => {
                    begin(&mut self.output, started)?;
                    stream_audio(chunks, &mut self.output, &mut self.input)
                }
                Err(error) => {
                    eprintln!("sd_dengjen: {error}");
                    Ok(Played::Failed)
                }
            },
        }
    }
}

enum Played {
    Continue,
    Interrupted(Interrupt),
    Failed,
}

fn begin(output: &mut impl Write, started: &mut bool) -> io::Result<()> {
    if !*started {
        reply::line(output, "200 OK SPEAKING")?;
        reply::line(output, "701 BEGIN")?;
        *started = true;
    }
    Ok(())
}

fn send_mark(output: &mut impl Write, name: &str) -> io::Result<()> {
    reply::line(output, &format!("700-{name}"))?;
    reply::line(output, "700 INDEX MARK")?;
    output.flush()
}

fn stream_audio(
    chunks: Chunks<'_>,
    output: &mut impl Write,
    input: &mut Lines,
) -> io::Result<Played> {
    for chunk in chunks {
        if let Some(interrupt) = input.interrupt() {
            return Ok(Played::Interrupted(interrupt));
        }
        match chunk {
            Ok(audio) => {
                output.write_all(&reply::audio_frame(&audio.samples, audio.sample_rate))?;
                output.flush()?;
            }
            Err(error) => {
                eprintln!("sd_dengjen: {error}");
                return Ok(Played::Failed);
            }
        }
    }
    Ok(Played::Continue)
}

fn matches_language(requested: &str, language: &str) -> bool {
    let primary = language.split('-').next().unwrap_or(language);
    requested.eq_ignore_ascii_case(language) || requested.eq_ignore_ascii_case(primary)
}

#[derive(Clone, Copy)]
enum Block {
    Settings,
    Audio,
    LogLevel,
}

impl Block {
    fn opening(self) -> &'static str {
        match self {
            Block::Settings => "203 OK RECEIVING SETTINGS",
            Block::Audio => "207 OK RECEIVING AUDIO SETTINGS",
            Block::LogLevel => "207 OK RECEIVING LOGLEVEL SETTINGS",
        }
    }

    fn done(self) -> &'static str {
        match self {
            Block::Settings => "203 OK SETTINGS RECEIVED",
            Block::Audio => "203 OK AUDIO INITIALIZED",
            Block::LogLevel => "203 OK LOGLEVEL SET",
        }
    }
}

fn debug_reply(line: &str) -> &'static str {
    let mut words = line.split_whitespace().skip(1);
    match (words.next(), words.next()) {
        (Some("ON"), Some(_)) => "200 OK DEBUGGING ON",
        (Some("OFF"), _) => "200 OK DEBUGGING OFF",
        _ => reply::BAD_SYNTAX,
    }
}

fn message_pieces(kind: Message, lines: &[String]) -> Result<Vec<Piece>, &'static str> {
    let text = lines.join("\n");
    let pieces = match kind {
        _ if text.is_empty() => return Err(reply::CANT_SPEAK),
        Message::SoundIcon => return Err(reply::CANT_SPEAK),
        Message::Text => ssml::pieces(&text),
        _ if lines.len() > 1 => return Err(reply::MULTILINE),
        Message::Char | Message::Key if text == "space" => vec![Piece::Text(" ".to_owned())],
        _ => vec![Piece::Text(text)],
    };
    let speakable = pieces.iter().any(|piece| matches!(piece, Piece::Text(_)));
    if speakable {
        Ok(pieces)
    } else {
        Err(reply::CANT_SPEAK)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        fail_init: bool,
        fail_speak: bool,
        fail_midway: bool,
        spoken: Vec<(String, Settings)>,
    }

    fn fake() -> Fake {
        Fake {
            fail_init: false,
            fail_speak: false,
            fail_midway: false,
            spoken: Vec::new(),
        }
    }

    impl Backend for &mut Fake {
        fn init(&mut self) -> Result<String, String> {
            if self.fail_init {
                Err("no voices".into())
            } else {
                Ok("ready".into())
            }
        }

        fn voices(&self) -> Vec<VoiceInfo> {
            let voice = |name: &str, language: &str| VoiceInfo {
                name: name.into(),
                language: language.into(),
                variant: "none".into(),
            };
            vec![voice("ku_TR-x", "ku-TR"), voice("en_GB-y", "en-GB")]
        }

        fn synthesize<'a>(
            &'a mut self,
            text: &str,
            settings: &Settings,
        ) -> Result<Chunks<'a>, String> {
            self.spoken.push((text.to_owned(), settings.clone()));
            if self.fail_speak {
                return Err("boom".into());
            }
            let chunk = || {
                Ok(Audio {
                    sample_rate: 22050,
                    samples: vec![1, 2],
                })
            };
            let second = if self.fail_midway {
                Err("boom".to_owned())
            } else {
                chunk()
            };
            Ok(Box::new([chunk(), second].into_iter()))
        }
    }

    fn scripted(input: &str) -> Lines {
        let (sender, receiver) = std::sync::mpsc::channel();
        input
            .lines()
            .for_each(|line| sender.send(Ok(line.to_owned())).unwrap());
        Lines::new(receiver)
    }

    fn transcript(backend: &mut Fake, input: &str) -> String {
        let mut output = Vec::new();
        run_lines(backend, scripted(input), &mut output).unwrap();
        String::from_utf8_lossy(&output).into_owned()
    }

    #[test]
    fn stop_during_speech_ends_it_with_a_stop_event_and_no_audio() {
        let out = transcript(&mut fake(), "INIT\nSPEAK\nx\n.\nSTOP\nQUIT\n");
        assert!(out.contains("701 BEGIN\n703 STOP\n210 OK QUIT\n"));
        assert!(!out.contains("705 AUDIO"));
    }

    #[test]
    fn ssml_marks_follow_the_audio_of_the_text_before_them() {
        let mut backend = fake();
        let input = "INIT\nSPEAK\n<speak>Silav.<mark name=\"__spd_0\"/> a &amp; b<mark name=\"__spd_1\"/></speak>\n.\n";
        let out = transcript(&mut backend, input);
        let spoken: Vec<&str> = backend
            .spoken
            .iter()
            .map(|(text, _)| text.as_str())
            .collect();
        assert_eq!(spoken, ["Silav.", "a & b"]);
        assert!(out.contains("705 AUDIO\n700-__spd_0\n700 INDEX MARK\n705-bits=16"));
        assert!(out.ends_with("705 AUDIO\n700-__spd_1\n700 INDEX MARK\n702 END\n"));
    }

    #[test]
    fn an_interrupt_withholds_the_marks_after_it() {
        let input = "INIT\nSPEAK\n<speak>a<mark name=\"m\"/>b</speak>\n.\nSTOP\n";
        let out = transcript(&mut fake(), input);
        assert!(out.ends_with("701 BEGIN\n703 STOP\n"));
        assert!(!out.contains("700-"));
    }

    #[test]
    fn a_message_of_only_marks_cant_speak() {
        let out = transcript(
            &mut fake(),
            "INIT\nSPEAK\n<speak><mark name=\"m\"/></speak>\n.\n",
        );
        assert!(out.ends_with("202 OK RECEIVING MESSAGE\n301 ERROR CANT SPEAK\n"));
    }

    #[test]
    fn pause_during_speech_reports_a_pause_event() {
        let out = transcript(&mut fake(), "INIT\nSPEAK\nx\n.\nPAUSE\nQUIT\n");
        assert!(out.contains("701 BEGIN\n704 PAUSE\n210 OK QUIT\n"));
    }

    #[test]
    fn quit_during_speech_stops_it_and_still_quits() {
        let out = transcript(&mut fake(), "INIT\nSPEAK\nx\n.\nQUIT\n");
        assert!(out.ends_with("701 BEGIN\n703 STOP\n210 OK QUIT\n"));
    }

    #[test]
    fn a_spoken_line_reading_stop_is_not_a_command() {
        let mut backend = fake();
        let out = transcript(&mut backend, "INIT\nSPEAK\nx\n.\nSPEAK\nSTOP\n.\n");
        assert_eq!(out.matches("702 END\n").count(), 2);
        assert_eq!(backend.spoken[1].0, "STOP");
    }

    #[test]
    fn init_reports_the_backend_message() {
        let out = transcript(&mut fake(), "INIT\nQUIT\n");
        assert_eq!(out, "299-ready\n299 OK LOADED SUCCESSFULLY\n210 OK QUIT\n");
    }

    #[test]
    fn a_failed_init_ends_the_session() {
        let mut backend = Fake {
            fail_init: true,
            ..fake()
        };
        let out = transcript(&mut backend, "INIT\nQUIT\n");
        assert_eq!(out, "399-no voices\n399 ERR CANT INIT MODULE\n");
    }

    #[test]
    fn anything_but_init_first_is_an_error() {
        let mut output = Vec::new();
        assert!(run(&mut fake(), "SPEAK\n".as_bytes(), &mut output).is_err());
    }

    #[test]
    fn set_stores_valid_settings_and_flags_bad_ones() {
        let mut backend = fake();
        let out = transcript(
            &mut backend,
            "INIT\nSET\nrate=20\nvolume=999\n.\nSET\nbogus\n.\nSET\nrate=5\n.\nQUIT\n",
        );
        assert!(out.contains("203 OK RECEIVING SETTINGS\n303 ERROR INVALID PARAMETER OR VALUE\n"));
        assert!(out.contains("203 OK RECEIVING SETTINGS\n302 ERROR BAD SYNTAX\n"));
        assert!(out.contains("203 OK RECEIVING SETTINGS\n203 OK SETTINGS RECEIVED\n"));
    }

    #[test]
    fn audio_accepts_only_server_output() {
        let ok = transcript(
            &mut fake(),
            "INIT\nAUDIO\naudio_output_method=server\n.\nQUIT\n",
        );
        assert!(ok.contains("207 OK RECEIVING AUDIO SETTINGS\n203 OK AUDIO INITIALIZED\n"));
        let bad = transcript(
            &mut fake(),
            "INIT\nAUDIO\naudio_output_method=alsa\n.\nQUIT\n",
        );
        assert!(
            bad.contains("207 OK RECEIVING AUDIO SETTINGS\n303 ERROR INVALID PARAMETER OR VALUE\n")
        );
    }

    #[test]
    fn speak_synthesizes_with_the_stored_settings_and_frames_the_audio() {
        let mut backend = fake();
        let out = transcript(
            &mut backend,
            "INIT\nSET\nrate=20\n.\nSPEAK\nSilav\n..dot\n.\n",
        );
        assert_eq!(backend.spoken.len(), 1);
        assert_eq!(backend.spoken[0].0, "Silav\n.dot");
        assert_eq!(backend.spoken[0].1.rate, 20);
        assert!(out.contains("202 OK RECEIVING MESSAGE\n200 OK SPEAKING\n701 BEGIN\n705-bits=16"));
        assert!(out.ends_with("\n705 AUDIO\n702 END\n"));
    }

    #[test]
    fn empty_text_and_synthesis_errors_cant_speak() {
        let empty = transcript(&mut fake(), "INIT\nSPEAK\n.\nQUIT\n");
        assert!(empty.contains("202 OK RECEIVING MESSAGE\n301 ERROR CANT SPEAK\n"));
        let mut backend = Fake {
            fail_speak: true,
            ..fake()
        };
        let failed = transcript(&mut backend, "INIT\nSPEAK\nx\n.\nQUIT\n");
        assert!(failed.contains("202 OK RECEIVING MESSAGE\n301 ERROR CANT SPEAK\n"));
    }

    #[test]
    fn char_is_one_line_and_space_is_a_space() {
        let mut backend = fake();
        let out = transcript(&mut backend, "INIT\nCHAR\nspace\n.\nCHAR\na\nb\n.\nQUIT\n");
        assert_eq!(backend.spoken[0].0, " ");
        assert!(out.contains("305 DATA MORE THAN ONE LINE\n"));
        assert_eq!(backend.spoken.len(), 1);
    }

    #[test]
    fn list_voices_filters_by_language_and_primary_subtag() {
        let all = transcript(&mut fake(), "INIT\nLIST VOICES\nQUIT\n");
        assert!(all.contains(
            "200-ku_TR-x\tku-TR\tnone\n200-en_GB-y\ten-GB\tnone\n200 OK VOICE LIST SENT\n"
        ));
        let ku = transcript(&mut fake(), "INIT\nLIST VOICES ku\nQUIT\n");
        assert!(ku.contains("200-ku_TR-x") && !ku.contains("en_GB"));
        let none = transcript(&mut fake(), "INIT\nLIST VOICES fr\nQUIT\n");
        assert!(none.contains("304 CANT LIST VOICES\n"));
    }

    #[test]
    fn unknown_commands_are_rejected_and_stop_pause_are_silent() {
        let out = transcript(&mut fake(), "INIT\nFOO\nSTOP\nPAUSE\nQUIT\n");
        assert_eq!(
            out,
            "299-ready\n299 OK LOADED SUCCESSFULLY\n300 ERR UNKNOWN COMMAND\n210 OK QUIT\n"
        );
    }

    #[test]
    fn every_chunk_is_its_own_frame_between_begin_and_end() {
        let out = transcript(&mut fake(), "INIT\nSPEAK\nx\n.\n");
        assert_eq!(out.matches("705 AUDIO\n").count(), 2);
        assert!(out.contains("701 BEGIN\n705-bits=16"));
        assert!(out.ends_with("705 AUDIO\n702 END\n"));
    }

    #[test]
    fn a_failure_mid_stream_keeps_the_audio_so_far_and_still_ends() {
        let mut backend = Fake {
            fail_midway: true,
            ..fake()
        };
        let out = transcript(&mut backend, "INIT\nSPEAK\nx\n.\n");
        assert_eq!(out.matches("705 AUDIO\n").count(), 1);
        assert!(out.ends_with("705 AUDIO\n702 END\n"));
    }

    #[test]
    fn loglevel_and_debug_are_acknowledged() {
        let out = transcript(
            &mut fake(),
            "INIT\nLOGLEVEL\nlog_level=3\n.\nLOGLEVEL\nlog_level=x\n.\nDEBUG ON /tmp/f\nDEBUG OFF\nDEBUG\nQUIT\n",
        );
        assert!(out.contains("207 OK RECEIVING LOGLEVEL SETTINGS\n203 OK LOGLEVEL SET\n"));
        assert!(out.contains(
            "207 OK RECEIVING LOGLEVEL SETTINGS\n303 ERROR INVALID PARAMETER OR VALUE\n"
        ));
        assert!(out.contains("200 OK DEBUGGING ON\n200 OK DEBUGGING OFF\n302 ERROR BAD SYNTAX\n"));
    }

    #[test]
    fn sound_icons_are_refused_instead_of_spoken() {
        let mut backend = fake();
        let out = transcript(&mut backend, "INIT\nSOUND_ICON\nbell\n.\nQUIT\n");
        assert!(out.contains("202 OK RECEIVING MESSAGE\n301 ERROR CANT SPEAK\n"));
        assert!(backend.spoken.is_empty());
    }

    #[test]
    fn eof_ends_the_session_cleanly() {
        let out = transcript(&mut fake(), "INIT\nSPEAK\nhalf");
        assert!(out.ends_with("202 OK RECEIVING MESSAGE\n"));
    }
}
