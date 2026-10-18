//! Resource caps for the gRPC server, configurable through environment variables.

pub(crate) const MAX_VOICES_VAR: &str = "DENGJEN_GRPC_MAX_VOICES";
pub(crate) const MAX_TEXT_BYTES_VAR: &str = "DENGJEN_GRPC_MAX_TEXT_BYTES";
pub(crate) const MAX_CONCURRENT_SYNTHESIS_VAR: &str = "DENGJEN_GRPC_MAX_CONCURRENT_SYNTHESIS";
pub(crate) const MAX_CONCURRENT_RECOGNITION_VAR: &str = "DENGJEN_GRPC_MAX_CONCURRENT_RECOGNITION";
pub(crate) const MAX_MESSAGE_BYTES_VAR: &str = "DENGJEN_GRPC_MAX_MESSAGE_BYTES";

const DEFAULT_MAX_VOICES: usize = 16;
const DEFAULT_MAX_TEXT_BYTES: usize = 64 * 1024;
const DEFAULT_MAX_CONCURRENT_SYNTHESIS: usize = 4;
const DEFAULT_MAX_CONCURRENT_RECOGNITION: usize = 1;
const DEFAULT_MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Limits {
    pub max_voices: usize,
    pub max_text_bytes: usize,
    pub max_concurrent_synthesis: usize,
    pub max_concurrent_recognition: usize,
    pub max_message_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_voices: DEFAULT_MAX_VOICES,
            max_text_bytes: DEFAULT_MAX_TEXT_BYTES,
            max_concurrent_synthesis: DEFAULT_MAX_CONCURRENT_SYNTHESIS,
            max_concurrent_recognition: DEFAULT_MAX_CONCURRENT_RECOGNITION,
            max_message_bytes: DEFAULT_MAX_MESSAGE_BYTES,
        }
    }
}

fn positive_or(lookup: &impl Fn(&str) -> Option<String>, name: &str, default: usize) -> usize {
    lookup(name)
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|&value| value > 0)
        .unwrap_or(default)
}

impl Limits {
    /// Reads each cap from `lookup`; unset, unparsable or zero values keep the default.
    pub(crate) fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        let defaults = Self::default();
        Self {
            max_voices: positive_or(&lookup, MAX_VOICES_VAR, defaults.max_voices),
            max_text_bytes: positive_or(&lookup, MAX_TEXT_BYTES_VAR, defaults.max_text_bytes),
            max_concurrent_synthesis: positive_or(
                &lookup,
                MAX_CONCURRENT_SYNTHESIS_VAR,
                defaults.max_concurrent_synthesis,
            ),
            max_concurrent_recognition: positive_or(
                &lookup,
                MAX_CONCURRENT_RECOGNITION_VAR,
                defaults.max_concurrent_recognition,
            ),
            max_message_bytes: positive_or(
                &lookup,
                MAX_MESSAGE_BYTES_VAR,
                defaults.max_message_bytes,
            ),
        }
    }

    pub(crate) fn from_env() -> Self {
        Self::from_lookup(|name| std::env::var(name).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn lookup_from(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |name| map.get(name).cloned()
    }

    #[test]
    fn unset_variables_keep_the_defaults() {
        assert_eq!(Limits::from_lookup(lookup_from(&[])), Limits::default());
    }

    #[test]
    fn set_variables_override_the_matching_cap_only() {
        let limits = Limits::from_lookup(lookup_from(&[
            (MAX_VOICES_VAR, "3"),
            (MAX_TEXT_BYTES_VAR, " 10 "),
        ]));
        assert_eq!(limits.max_voices, 3);
        assert_eq!(limits.max_text_bytes, 10);
        assert_eq!(
            limits.max_concurrent_synthesis,
            Limits::default().max_concurrent_synthesis
        );
        assert_eq!(
            limits.max_message_bytes,
            Limits::default().max_message_bytes
        );
    }

    #[test]
    fn zero_and_unparsable_values_fall_back_to_the_default() {
        let limits = Limits::from_lookup(lookup_from(&[
            (MAX_VOICES_VAR, "0"),
            (MAX_CONCURRENT_SYNTHESIS_VAR, "many"),
            (MAX_MESSAGE_BYTES_VAR, "-4"),
        ]));
        assert_eq!(limits, Limits::default());
    }

    #[test]
    fn recognition_cap_defaults_to_one() {
        assert_eq!(Limits::default().max_concurrent_recognition, 1);
    }

    #[test]
    fn recognition_cap_reads_its_own_variable() {
        let limits = Limits::from_lookup(lookup_from(&[(MAX_CONCURRENT_RECOGNITION_VAR, "3")]));
        assert_eq!(limits.max_concurrent_recognition, 3);
        assert_eq!(
            limits.max_concurrent_synthesis,
            Limits::default().max_concurrent_synthesis
        );
    }

    #[test]
    fn zero_recognition_cap_falls_back_to_the_default() {
        let limits = Limits::from_lookup(lookup_from(&[(MAX_CONCURRENT_RECOGNITION_VAR, "0")]));
        assert_eq!(limits.max_concurrent_recognition, 1);
    }
}
