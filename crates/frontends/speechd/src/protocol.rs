use std::io::{self, BufRead};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Message {
    Text,
    Char,
    Key,
    SoundIcon,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Speak(Message),
    Stop,
    Pause,
    ListVoices {
        language: Option<String>,
        variant: Option<String>,
    },
    Set,
    Audio,
    Loglevel,
    Debug,
    Quit,
    Unknown,
}

pub(crate) fn parse_command(line: &str) -> Command {
    match line {
        "SPEAK" => Command::Speak(Message::Text),
        "SOUND_ICON" => Command::Speak(Message::SoundIcon),
        "CHAR" => Command::Speak(Message::Char),
        "KEY" => Command::Speak(Message::Key),
        "STOP" => Command::Stop,
        "PAUSE" => Command::Pause,
        "SET" => Command::Set,
        "AUDIO" => Command::Audio,
        "LOGLEVEL" => Command::Loglevel,
        "QUIT" => Command::Quit,
        _ if line.starts_with("DEBUG") => Command::Debug,
        _ if line.starts_with("LIST VOICES") => list_voices(line),
        _ => Command::Unknown,
    }
}

fn list_voices(line: &str) -> Command {
    let mut words = line.split_whitespace().skip(2).map(str::to_owned);
    let language = words.next();
    let variant = language.as_ref().and_then(|_| words.next());
    Command::ListVoices { language, variant }
}

pub(crate) fn read_line(input: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut bytes = Vec::new();
    if input.read_until(b'\n', &mut bytes)? == 0 {
        return Ok(None);
    }
    let line = String::from_utf8_lossy(&bytes);
    Ok(Some(line.trim_end_matches(['\n', '\r']).to_owned()))
}

pub(crate) fn read_block(input: &mut impl BufRead) -> io::Result<Option<Vec<String>>> {
    let mut lines = Vec::new();
    loop {
        match read_line(input)? {
            None => return Ok(None),
            Some(line) if line == "." => return Ok(Some(lines)),
            Some(line) => lines.push(line.strip_prefix('.').map_or(line.clone(), str::to_owned)),
        }
    }
}

pub(crate) fn parse_assignment(line: &str) -> Option<(&str, &str)> {
    line.split_once('=')
        .filter(|(name, value)| !name.is_empty() && !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_map_to_variants() {
        assert_eq!(parse_command("SPEAK"), Command::Speak(Message::Text));
        assert_eq!(parse_command("CHAR"), Command::Speak(Message::Char));
        assert_eq!(parse_command("STOP"), Command::Stop);
        assert_eq!(parse_command("QUIT"), Command::Quit);
        assert_eq!(parse_command("DEBUG ON /tmp/x"), Command::Debug);
        assert_eq!(parse_command("NOPE"), Command::Unknown);
        assert_eq!(parse_command("speak"), Command::Unknown);
    }

    #[test]
    fn list_voices_carries_the_optional_filters() {
        let all = Command::ListVoices {
            language: None,
            variant: None,
        };
        assert_eq!(parse_command("LIST VOICES"), all);
        assert_eq!(
            parse_command("LIST VOICES ku male1"),
            Command::ListVoices {
                language: Some("ku".into()),
                variant: Some("male1".into())
            }
        );
    }

    #[test]
    fn a_block_ends_at_a_lone_dot_and_unescapes_leading_dots() {
        let mut input = "a\n..b\r\n.\nnext\n".as_bytes();
        assert_eq!(
            read_block(&mut input).unwrap(),
            Some(vec!["a".to_owned(), ".b".to_owned()])
        );
        assert_eq!(read_line(&mut input).unwrap().as_deref(), Some("next"));
    }

    #[test]
    fn a_block_cut_by_eof_is_none() {
        assert_eq!(read_block(&mut "a\n".as_bytes()).unwrap(), None);
    }

    #[test]
    fn invalid_utf8_is_replaced_not_fatal() {
        let mut input: &[u8] = &[b'a', 0xff, b'\n'];
        assert_eq!(read_line(&mut input).unwrap().as_deref(), Some("a\u{fffd}"));
    }

    #[test]
    fn assignments_need_a_name_and_a_value() {
        assert_eq!(parse_assignment("rate=10"), Some(("rate", "10")));
        assert_eq!(parse_assignment("a=b=c"), Some(("a", "b=c")));
        assert_eq!(parse_assignment("rate"), None);
        assert_eq!(parse_assignment("=1"), None);
        assert_eq!(parse_assignment("rate="), None);
    }
}
