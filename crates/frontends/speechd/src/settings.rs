const PERCENT_RANGE: std::ops::RangeInclusive<i32> = -100..=100;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    pub rate: i32,
    pub pitch: i32,
    pub volume: i32,
    pub language: Option<String>,
    pub synthesis_voice: Option<String>,
}

fn percent(value: &str) -> Option<i32> {
    value
        .parse()
        .ok()
        .filter(|number| PERCENT_RANGE.contains(number))
}

fn optional(value: &str) -> Option<String> {
    (value != "NULL").then(|| value.to_owned())
}

fn scaled(value: i32, neutral: i32, below: f32, above: f32) -> Option<u8> {
    let slope = if value < 0 { below } else { above };
    (value != 0).then(|| {
        (neutral as f32 + value as f32 * slope)
            .round()
            .clamp(0.0, 100.0) as u8
    })
}

impl Settings {
    /// Engine rate, or `None` at the neutral setting.
    pub fn engine_rate(&self) -> Option<u8> {
        scaled(self.rate, 10, 0.1, 0.4)
    }

    /// Engine pitch, or `None` at the neutral setting.
    pub fn engine_pitch(&self) -> Option<u8> {
        scaled(self.pitch, 50, 0.5, 0.5)
    }

    /// Engine volume, or `None` at full volume. Positive speechd values are ignored.
    pub fn engine_volume(&self) -> Option<u8> {
        scaled(self.volume.min(0), 100, 1.0, 1.0)
    }

    pub(crate) fn apply(&mut self, name: &str, value: &str) -> Result<(), ()> {
        match name {
            "rate" => self.rate = percent(value).ok_or(())?,
            "pitch" => self.pitch = percent(value).ok_or(())?,
            "volume" => self.volume = percent(value).ok_or(())?,
            "language" => self.language = optional(value),
            "synthesis_voice" => self.synthesis_voice = optional(value),
            "pitch_range" => percent(value).map(drop).ok_or(())?,
            "punctuation_mode" | "spelling_mode" | "cap_let_recogn" | "voice" => {}
            _ => return Err(()),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentages_accept_the_full_range_and_reject_the_rest() {
        let mut settings = Settings::default();
        assert!(settings.apply("rate", "-100").is_ok());
        assert!(settings.apply("pitch", "100").is_ok());
        assert!(settings.apply("volume", "101").is_err());
        assert!(settings.apply("rate", "fast").is_err());
        assert_eq!(
            (settings.rate, settings.pitch, settings.volume),
            (-100, 100, 0)
        );
    }

    #[test]
    fn engine_levels_are_neutral_at_zero_and_span_the_engine_range() {
        let at = |rate, pitch, volume| Settings {
            rate,
            pitch,
            volume,
            ..Settings::default()
        };
        let neutral = at(0, 0, 0);
        assert_eq!(
            (
                neutral.engine_rate(),
                neutral.engine_pitch(),
                neutral.engine_volume()
            ),
            (None, None, None)
        );
        assert_eq!(
            (at(-100, 0, 0).engine_rate(), at(100, 0, 0).engine_rate()),
            (Some(0), Some(50))
        );
        assert_eq!(
            (at(0, -100, 0).engine_pitch(), at(0, 100, 0).engine_pitch()),
            (Some(0), Some(100))
        );
        assert_eq!(
            (at(0, 0, -100).engine_volume(), at(0, 0, 50).engine_volume()),
            (Some(0), None)
        );
    }

    #[test]
    fn null_clears_language_and_voice() {
        let mut settings = Settings::default();
        settings.apply("language", "ku").unwrap();
        settings.apply("synthesis_voice", "ku_TR-x").unwrap();
        assert_eq!(settings.language.as_deref(), Some("ku"));
        settings.apply("language", "NULL").unwrap();
        settings.apply("synthesis_voice", "NULL").unwrap();
        assert_eq!((settings.language, settings.synthesis_voice), (None, None));
    }

    #[test]
    fn ignored_modes_are_accepted_and_unknown_names_are_not() {
        let mut settings = Settings::default();
        assert!(settings.apply("punctuation_mode", "some").is_ok());
        assert!(settings.apply("voice", "MALE1").is_ok());
        assert!(settings.apply("bogus", "1").is_err());
        assert_eq!(settings, Settings::default());
    }
}
