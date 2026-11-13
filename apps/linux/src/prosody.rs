//! Maps the window's speed and pitch sliders (-100..=100, 0 = the voice's own) onto the
//! engine's 0..=100 scales, the same way the Speech Dispatcher module does.

pub const SLIDER_MIN: f64 = -100.0;
pub const SLIDER_MAX: f64 = 100.0;

fn scaled(value: f64, neutral: f64, below: f64, above: f64) -> Option<u8> {
    let value = value.round().clamp(SLIDER_MIN, SLIDER_MAX);
    let slope = if value < 0.0 { below } else { above };
    // The clamp keeps the value in 0..=100, so the cast cannot truncate.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    (value != 0.0).then(|| (neutral + value * slope).round().clamp(0.0, 100.0) as u8)
}

/// Engine rate for the speed slider, or `None` at the voice's own speed.
pub fn engine_rate(slider: f64) -> Option<u8> {
    scaled(slider, 10.0, 0.1, 0.4)
}

/// Engine pitch for the pitch slider, or `None` at the voice's own pitch.
pub fn engine_pitch(slider: f64) -> Option<u8> {
    scaled(slider, 50.0, 0.5, 0.5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_middle_of_each_slider_leaves_the_voice_alone() {
        assert_eq!(engine_rate(0.0), None);
        assert_eq!(engine_pitch(0.4), None);
    }

    #[test]
    fn the_ends_of_each_slider_reach_the_engine_limits() {
        assert_eq!(engine_rate(SLIDER_MIN), Some(0));
        assert_eq!(engine_rate(SLIDER_MAX), Some(50));
        assert_eq!(engine_pitch(SLIDER_MIN), Some(0));
        assert_eq!(engine_pitch(SLIDER_MAX), Some(100));
        assert_eq!(engine_rate(1e9), Some(50));
    }
}
