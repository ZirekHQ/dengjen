//! Maps SAPI's rate, volume and pitch onto dengjen's `0..=100` engine settings.
#![forbid(unsafe_code)]

use crate::fragments::FragmentProsody;

/// SAPI's rate and pitch scales run from -10 to 10.
const SAPI_RANGE: i32 = 10;
/// dengjen's speed parameter spans 0.5x to 5.5x over its 0-100 percent scale.
const ENGINE_SPEED_MIN: f32 = 0.5;
const ENGINE_SPEED_SPAN: f32 = 5.0;

/// The engine settings for one utterance; `None` leaves a setting at the voice's default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EngineProsody {
    pub rate: Option<u8>,
    pub volume: Option<u8>,
    pub pitch: Option<u8>,
}

/// Combines the caller's global rate (`ISpTTSEngineSite::GetRate`) and volume
/// (`GetVolume`, 0-100) with a fragment's XML adjustments.
pub fn engine_prosody(
    site_rate: i32,
    site_volume: u16,
    fragment: FragmentProsody,
) -> EngineProsody {
    EngineProsody {
        rate: rate(site_rate.saturating_add(fragment.rate_adj)),
        volume: volume(u32::from(site_volume), fragment.volume),
        pitch: pitch(fragment.pitch_adj),
    }
}

/// SAPI defines each rate step as a fixed ratio, with +10 three times as fast as 0 and -10 a third
/// as fast. The engine cannot go below half speed, so the slowest SAPI rates all map to it.
fn rate(sapi_rate: i32) -> Option<u8> {
    let sapi_rate = sapi_rate.clamp(-SAPI_RANGE, SAPI_RANGE);
    (sapi_rate != 0).then(|| {
        let speed = 3f32.powf(sapi_rate as f32 / SAPI_RANGE as f32);
        percent((speed - ENGINE_SPEED_MIN) / ENGINE_SPEED_SPAN * 100.0)
    })
}

fn volume(site_volume: u32, fragment_volume: u32) -> Option<u8> {
    let combined = site_volume.min(100) * fragment_volume.min(100) / 100;
    (combined != 100).then(|| percent(combined as f32))
}

/// The engine's pitch runs from 0.5x at 0 to 1.5x at 100, so each SAPI step is 5 percent.
fn pitch(sapi_pitch: i32) -> Option<u8> {
    let sapi_pitch = sapi_pitch.clamp(-SAPI_RANGE, SAPI_RANGE);
    (sapi_pitch != 0).then(|| percent(50.0 + sapi_pitch as f32 * 5.0))
}

fn percent(value: f32) -> u8 {
    value.round().clamp(0.0, 100.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fragment(rate_adj: i32, volume: u32, pitch_adj: i32) -> FragmentProsody {
        FragmentProsody {
            rate_adj,
            volume,
            pitch_adj,
        }
    }

    #[test]
    fn neutral_settings_leave_the_voice_untouched() {
        assert_eq!(
            engine_prosody(0, 100, fragment(0, 100, 0)),
            EngineProsody::default()
        );
    }

    #[test]
    fn rate_follows_sapis_three_times_per_ten_steps() {
        // 3x speed is (3 - 0.5) / 5 = 50 percent of the engine's range.
        assert_eq!(rate(10), Some(50));
        assert_eq!(rate(5), Some(25));
        // A third of normal speed is below the engine's floor of half speed.
        assert_eq!(rate(-10), Some(0));
        assert_eq!(rate(-3), Some(4));
        assert_eq!(rate(40), rate(10));
    }

    #[test]
    fn the_fragment_rate_adds_to_the_callers_rate() {
        assert_eq!(engine_prosody(3, 100, fragment(2, 100, 0)).rate, rate(5));
        assert_eq!(engine_prosody(3, 100, fragment(-3, 100, 0)).rate, None);
    }

    #[test]
    fn volumes_multiply_and_silence_stays_silent() {
        assert_eq!(volume(50, 50), Some(25));
        assert_eq!(volume(0, 100), Some(0));
        assert_eq!(volume(100, 100), None);
        assert_eq!(volume(500, 100), None);
    }

    #[test]
    fn pitch_moves_five_percent_per_step() {
        assert_eq!(pitch(0), None);
        assert_eq!(pitch(10), Some(100));
        assert_eq!(pitch(-10), Some(0));
        assert_eq!(pitch(2), Some(60));
    }
}
