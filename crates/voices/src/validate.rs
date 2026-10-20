use crate::error::{VoiceError, VoiceResult};

const MAX_KEY_LEN: usize = 128;
const MAX_PATH_LEN: usize = 512;
const DEVICE_NAMES: [&str; 4] = ["CON", "PRN", "AUX", "NUL"];
const NUMBERED_DEVICES: [&str; 2] = ["COM", "LPT"];

fn allowed(c: char) -> bool {
    c.is_ascii_alphanumeric() || c.is_alphabetic() || matches!(c, '_' | '.' | '+' | '-')
}

fn is_device_name(segment: &str) -> bool {
    let stem = segment
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    DEVICE_NAMES.contains(&stem.as_str())
        || NUMBERED_DEVICES.iter().any(|prefix| {
            stem.strip_prefix(prefix)
                .is_some_and(|n| matches!(n, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9"))
        })
}

fn segment_ok(segment: &str) -> bool {
    !segment.is_empty()
        && !segment.starts_with('.')
        && !segment.ends_with('.')
        && segment.chars().all(allowed)
        && !is_device_name(segment)
}

pub fn validate_key(key: &str) -> VoiceResult<&str> {
    (key.len() <= MAX_KEY_LEN && segment_ok(key))
        .then_some(key)
        .ok_or_else(|| VoiceError::InvalidKey(key.to_owned()))
}

pub fn file_name(path: &str) -> VoiceResult<&str> {
    (path.len() <= MAX_PATH_LEN && path.split('/').all(segment_ok))
        .then(|| path.rsplit('/').next())
        .flatten()
        .ok_or_else(|| VoiceError::UnsafePath(path.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn accepts_piper_style_keys() {
        for key in [
            "en_US-lessac-medium",
            "ku_TR-berfin_renas-medium",
            "en_GB-vctk+rt",
            "pt_PT-tugão-medium",
        ] {
            assert_eq!(validate_key(key), Ok(key));
        }
    }

    #[test]
    fn rejects_unsafe_keys() {
        for key in [
            "",
            ".",
            "..",
            ".hidden",
            "a/b",
            "a\\b",
            "a b",
            "a\0b",
            "a\u{202e}b",
            "a²",
            "a／b",
        ] {
            assert_eq!(
                validate_key(key),
                Err(VoiceError::InvalidKey(key.to_owned()))
            );
        }
        assert!(validate_key(&"a".repeat(129)).is_err());
        assert!(validate_key(&"a".repeat(128)).is_ok());
    }

    #[test]
    fn rejects_trailing_dots_and_windows_device_names() {
        for key in [
            "a.",
            "voice..",
            "CON",
            "con",
            "Nul.onnx",
            "aux.tar.gz",
            "prn",
            "COM1",
            "lpt9.x",
        ] {
            assert_eq!(
                validate_key(key),
                Err(VoiceError::InvalidKey(key.to_owned()))
            );
            let path = format!("a/{key}");
            assert_eq!(file_name(&path), Err(VoiceError::UnsafePath(path.clone())));
        }
        for key in ["COM0", "LPT10", "console", "nul_voice", "a.con", "comx"] {
            assert_eq!(validate_key(key), Ok(key));
        }
    }

    #[test]
    fn file_name_rejects_paths_over_the_length_limit() {
        let fits = format!("{}/b", "a".repeat(MAX_PATH_LEN - 2));
        let long = format!("{}/b", "a".repeat(MAX_PATH_LEN - 1));
        assert_eq!(file_name(&fits), Ok("b"));
        assert_eq!(file_name(&long), Err(VoiceError::UnsafePath(long.clone())));
    }

    #[test]
    fn file_name_returns_the_last_segment() {
        let path = "ku/ku_TR/berfin_renas/medium/ku_TR-berfin_renas-medium.onnx";
        assert_eq!(file_name(path), Ok("ku_TR-berfin_renas-medium.onnx"));
        assert_eq!(file_name("MODEL_CARD"), Ok("MODEL_CARD"));
        let accented = "pt/pt_PT/tugão/medium/pt_PT-tugão-medium.onnx";
        assert_eq!(file_name(accented), Ok("pt_PT-tugão-medium.onnx"));
    }

    #[test]
    fn file_name_rejects_traversal_and_odd_paths() {
        let bad = [
            "",
            "/",
            "/etc/passwd",
            "../x",
            "a/../b",
            "a//b",
            "a/b/",
            "a\\b",
            "./a",
            "a/.b",
        ];
        for path in bad {
            assert_eq!(
                file_name(path),
                Err(VoiceError::UnsafePath(path.to_owned()))
            );
        }
    }

    proptest! {
        #[test]
        fn accepted_keys_never_escape(key in "[A-Za-z0-9_.+/\\\\-]{0,40}") {
            if let Ok(accepted) = validate_key(&key) {
                prop_assert!(!accepted.is_empty());
                prop_assert!(!accepted.contains('/') && !accepted.contains('\\'));
                prop_assert!(!accepted.starts_with('.') && !accepted.ends_with('.'));
                prop_assert!(!is_device_name(accepted));
            }
        }

        #[test]
        fn accepted_paths_have_no_dot_or_empty_segments(path in "[A-Za-z0-9_.+/\\\\-]{0,60}") {
            if let Ok(name) = file_name(&path) {
                prop_assert!(!path.split('/').any(|s| s.is_empty() || s.starts_with('.') || s.ends_with('.')));
                prop_assert!(!path.split('/').any(is_device_name));
                prop_assert!(!name.contains('/'));
            }
        }

        #[test]
        fn well_formed_keys_are_accepted(key in "[A-Za-z0-9_+-]([A-Za-z0-9_.+-]{0,39}[A-Za-z0-9_+-])?") {
            prop_assume!(!is_device_name(&key));
            prop_assert_eq!(validate_key(&key), Ok(key.as_str()));
        }

        #[test]
        fn well_formed_paths_yield_their_last_segment(
            segments in proptest::collection::vec("[A-Za-z0-9_+-]([A-Za-z0-9_.+-]{0,9}[A-Za-z0-9_+-])?", 1..5)
        ) {
            prop_assume!(!segments.iter().any(|segment| is_device_name(segment)));
            let path = segments.join("/");
            prop_assert_eq!(file_name(&path), Ok(segments.last().unwrap().as_str()));
        }
    }
}
