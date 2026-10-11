use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::f32::consts::PI;

#[derive(Debug, PartialEq)]
#[non_exhaustive]
pub enum AudioOpsError {
    InvalidWindowLength(usize),
}

impl std::fmt::Display for AudioOpsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidWindowLength(length) => write!(f, "invalid window length: {length}"),
        }
    }
}

impl std::error::Error for AudioOpsError {}

const PRECOMPUTED_HANN_WINDOW_LENGTHS: [usize; 7] = [64, 128, 256, 512, 1024, 2048, 4096];

static HANN_WINDOW_LOOKUP_TABLE: Lazy<HashMap<usize, Vec<f32>>> =
    Lazy::new(build_hann_window_lookup_table);

fn build_hann_window_lookup_table() -> HashMap<usize, Vec<f32>> {
    PRECOMPUTED_HANN_WINDOW_LENGTHS
        .iter()
        .map(|&length| (length, hann_window_values(length)))
        .collect()
}

/// Hann window of `window_length` samples, served from a precomputed table when available.
///
/// Errors with `InvalidWindowLength` for `window_length` of `0` or `1`.
pub fn get_hann_window(window_length: usize) -> Result<Vec<f32>, AudioOpsError> {
    if window_length <= 1 {
        return Err(AudioOpsError::InvalidWindowLength(window_length));
    }
    Ok(HANN_WINDOW_LOOKUP_TABLE
        .get(&window_length)
        .cloned()
        .unwrap_or_else(|| hann_window_values(window_length)))
}

/// `w(n) = 0.5 - 0.5 * cos(2π * n / (N - 1))`, exploiting the symmetry `w(n) == w(N - 1 - n)`
/// so each value is computed once from its distance to the nearer edge.
fn hann_window_values(length: usize) -> Vec<f32> {
    let angular_step = 2.0 * PI / (length - 1) as f32;
    (0..length)
        .map(|n| {
            let distance = n.min(length - 1 - n);
            0.5 - 0.5 * (angular_step * distance as f32).cos()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_hann_window_errors_on_zero_length() {
        assert_eq!(
            get_hann_window(0),
            Err(AudioOpsError::InvalidWindowLength(0))
        );
    }

    #[test]
    fn get_hann_window_errors_on_length_one() {
        assert_eq!(
            get_hann_window(1),
            Err(AudioOpsError::InvalidWindowLength(1))
        );
    }

    #[test]
    fn get_hann_window_starts_and_ends_at_zero_and_peaks_near_the_center() {
        let window = get_hann_window(64).unwrap();
        assert_eq!(window.len(), 64);
        assert_eq!(window[0], 0.0);
        assert_eq!(window[63], 0.0);
        let max = window.iter().cloned().fold(f32::MIN, f32::max);
        let max_index = window.iter().position(|&v| v == max).unwrap();
        assert!(
            (28..36).contains(&max_index),
            "expected peak near center, got index {max_index}"
        );
    }

    #[test]
    fn get_hann_window_lookup_table_matches_direct_computation() {
        assert_eq!(get_hann_window(64).unwrap(), hann_window_values(64));
    }

    #[test]
    fn get_hann_window_computes_on_the_fly_for_a_length_not_in_the_lookup_table() {
        assert!(!PRECOMPUTED_HANN_WINDOW_LENGTHS.contains(&10));
        let window = get_hann_window(10).unwrap();
        assert_eq!(window.len(), 10);
        assert_eq!(window[0], 0.0);
        assert_eq!(window[9], 0.0);
    }
}
