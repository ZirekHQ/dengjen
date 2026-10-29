use std::io::{self, BufRead, Write};

use crate::protocol::{self, Command, Message};
use crate::reply;
use crate::settings::Settings;

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

pub trait Backend {
    fn init(&mut self) -> Result<String, String>;
    fn voices(&self) -> Vec<VoiceInfo>;
    fn synthesize(&mut self, text: &str, settings: &Settings) -> Result<Audio, String>;
}

#[derive(PartialEq, Eq)]
enum Flow {
    Continue,
    Quit,
}

struct Session<B, R, W> {
    backend: B,
    settings: Settings,
    input: R,
    output: W,
}

pub fn run<B: Backend>(backend: B, input: impl BufRead, output: impl Write) -> io::Result<()> {
    let mut session = Session {
        backend,
        settings: Settings::default(),
        input,
        output,
    };
    if !session.init()? {
        return Ok(());
    }
    while let Some(line) = protocol::read_line(&mut session.input)? {
        let flow = session.dispatch(protocol::parse_command(&line))?;
        session.output.flush()?;
        if flow == Flow::Quit {
            break;
        }
    }
    Ok(())
}

impl<B: Backend, R: BufRead, W: Write> Session<B, R, W> {
    fn say(&mut self, text: &str) -> io::Result<()> {
        reply::line(&mut self.output, text)
    }

    fn init(&mut self) -> io::Result<bool> {
        match protocol::read_line(&mut self.input)?.as_deref() {
            Some("INIT") => {}
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "server did not start with INIT",
                ))
            }
        }
        let (code, message, ready) = match self.backend.init() {
            Ok(message) => ("299 OK LOADED SUCCESSFULLY", message, true),
            Err(message) => ("399 ERR CANT INIT MODULE", message, false),
        };
        let prefix = &code[..3];
        self.say(&format!("{prefix}-{message}"))?;
        self.say(code)?;
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
        let Some(lines) = protocol::read_block(&mut self.input)? else {
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
        let Some(lines) = protocol::read_block(&mut self.input)? else {
            return Ok(Flow::Quit);
        };
        match message_text(kind, &lines) {
            Err(error) => self.say(error)?,
            Ok(text) => self.speak_text(&text)?,
        }
        Ok(Flow::Continue)
    }

    fn speak_text(&mut self, text: &str) -> io::Result<()> {
        match self.backend.synthesize(text, &self.settings) {
            Err(_) => self.say(reply::CANT_SPEAK),
            Ok(audio) => {
                self.say("200 OK SPEAKING")?;
                self.say("701 BEGIN")?;
                self.output
                    .write_all(&reply::audio_frame(&audio.samples, audio.sample_rate))?;
                self.say("702 END")
            }
        }
    }
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

fn message_text(kind: Message, lines: &[String]) -> Result<String, &'static str> {
    let text = lines.join("\n");
    match kind {
        _ if text.is_empty() => Err(reply::CANT_SPEAK),
        Message::SoundIcon => Err(reply::CANT_SPEAK),
        Message::Text => Ok(text),
        _ if lines.len() > 1 => Err(reply::MULTILINE),
        Message::Char | Message::Key if text == "space" => Ok(" ".to_owned()),
        _ => Ok(text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        fail_init: bool,
        fail_speak: bool,
        spoken: Vec<(String, Settings)>,
    }

    fn fake() -> Fake {
        Fake {
            fail_init: false,
            fail_speak: false,
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

        fn synthesize(&mut self, text: &str, settings: &Settings) -> Result<Audio, String> {
            self.spoken.push((text.to_owned(), settings.clone()));
            if self.fail_speak {
                return Err("boom".into());
            }
            Ok(Audio {
                sample_rate: 22050,
                samples: vec![1, 2],
            })
        }
    }

    fn transcript(backend: &mut Fake, input: &str) -> String {
        let mut output = Vec::new();
        run(backend, input.as_bytes(), &mut output).unwrap();
        String::from_utf8_lossy(&output).into_owned()
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
            "INIT\nSET\nrate=20\n.\nSPEAK\nSilav\n..dot\n.\nQUIT\n",
        );
        assert_eq!(backend.spoken.len(), 1);
        assert_eq!(backend.spoken[0].0, "Silav\n.dot");
        assert_eq!(backend.spoken[0].1.rate, 20);
        assert!(out.contains("202 OK RECEIVING MESSAGE\n200 OK SPEAKING\n701 BEGIN\n705-bits=16"));
        assert!(out.contains("\n705 AUDIO\n702 END\n210 OK QUIT\n"));
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
