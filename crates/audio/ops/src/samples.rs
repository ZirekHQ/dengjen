use crate::hanning_window;
use std::path::Path;

const HALF_TURN: f32 = std::f32::consts::PI;
const I16_MIN_AS_F32: f32 = i16::MIN as f32;
const I16_MAX_AS_F32: f32 = i16::MAX as f32;
// Fixed gain: per-chunk peak normalization made loudness differ between sentences.
// 1.25 is 1/0.8; measured model peaks reach 0.77, louder samples clamp.
const REFERENCE_OUTPUT_GAIN: f32 = 1.25;

#[derive(Debug, Clone)]
pub struct AudioInfo {
    pub sample_rate: usize,
    pub num_channels: usize,
    pub sample_width: usize,
}

#[derive(Clone, Debug, Default)]
#[must_use]
pub struct AudioSamples(Vec<f32>);

impl AudioSamples {
    pub fn new(samples: Vec<f32>) -> Self {
        Self(samples)
    }

    pub fn as_slice(&self) -> &[f32] {
        &self.0
    }

    pub fn as_vec(&self) -> &Vec<f32> {
        &self.0
    }

    pub fn as_mut_vec(&mut self) -> &mut Vec<f32> {
        &mut self.0
    }

    pub fn into_vec(self) -> Vec<f32> {
        self.0
    }

    pub fn take(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.0)
    }

    pub fn take_range(&mut self, sample_range: std::ops::Range<usize>) -> Vec<f32> {
        let clamped_end = sample_range.end.min(self.0.len());
        let clamped_start = sample_range.start.min(clamped_end);
        self.0.drain(clamped_start..clamped_end).collect()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn to_i16_vec(&self) -> Vec<i16> {
        if self.0.is_empty() {
            return Vec::new();
        }

        self.0
            .iter()
            .map(|&sample| {
                if sample.is_finite() {
                    (sample * REFERENCE_OUTPUT_GAIN * I16_MAX_AS_F32)
                        .clamp(I16_MIN_AS_F32, I16_MAX_AS_F32) as i16
                } else {
                    0
                }
            })
            .collect()
    }

    pub fn as_wave_bytes(&self) -> Vec<u8> {
        self.to_i16_vec()
            .into_iter()
            .flat_map(i16::to_le_bytes)
            .collect()
    }

    pub fn merge(&mut self, mut other: Self) {
        self.0.append(&mut other.0);
    }

    pub fn normalize(&mut self, max_value: f32) {
        if self.0.is_empty() {
            return;
        }

        let peak_magnitude = self
            .0
            .iter()
            .filter(|sample| sample.is_finite())
            .map(|sample| sample.abs())
            .fold(0.0f32, f32::max);
        let divisor = peak_magnitude.max(max_value) / max_value.abs();
        self.0.iter_mut().for_each(|sample| {
            *sample = if sample.is_finite() {
                *sample / divisor
            } else {
                0.0
            };
        });
    }

    pub fn apply_hanning_window(&mut self) -> Result<(), crate::AudioOpsError> {
        if self.0.is_empty() {
            return Ok(());
        }
        let window = hanning_window::get_hann_window(self.0.len())?;
        self.0
            .iter_mut()
            .zip(window)
            .for_each(|(sample, gain)| *sample *= gain);
        Ok(())
    }

    pub fn overlap_with(&mut self, other: &mut Self, overlap_len: usize) {
        let overlap_len = overlap_len.min(self.0.len()).min(other.0.len());
        if overlap_len == 1 {
            let tail_start = self.0.len() - 1;
            self.0[tail_start] = (self.0[tail_start] + other.0[0]) * 0.5;
        } else if overlap_len > 1 {
            let tail_start = self.0.len() - overlap_len;
            let span = (overlap_len - 1) as f32;
            for offset in 0..overlap_len {
                let gain_in = offset as f32 / span;
                let gain_out = 1.0 - gain_in;
                self.0[tail_start + offset] =
                    self.0[tail_start + offset] * gain_out + other.0[offset] * gain_in;
            }
        }
        self.0.extend_from_slice(&other.0[overlap_len..]);
    }

    pub fn fade_in(&mut self, fade_samples: usize) {
        let span = fade_samples.min(self.0.len());
        let span_f32 = span as f32;
        self.0
            .iter_mut()
            .take(span)
            .enumerate()
            .for_each(|(i, sample)| {
                *sample *= (i as f32 / span_f32 * HALF_TURN / 2.0).sin();
            });
    }

    pub fn fade_out(&mut self, fade_samples: usize) {
        let span = fade_samples.min(self.0.len());
        let span_f32 = span as f32;
        self.0
            .iter_mut()
            .rev()
            .take(span)
            .enumerate()
            .for_each(|(i, sample)| {
                *sample *= (i as f32 / span_f32 * HALF_TURN / 2.0).sin();
            });
    }

    pub fn crossfade(&mut self, fade_samples: usize) {
        let length = self.0.len();
        let span = fade_samples.min(length / 2);

        if span < 2 {
            return;
        }
        let span_f32 = (span - 1) as f32;
        for i in 0..span {
            let gain = (i as f32 / span_f32 * HALF_TURN / 2.0).sin();
            self.0[i] *= gain;
            self.0[length - 1 - i] *= gain;
        }
    }

    pub fn zero_samples_above(&mut self, sample_range: std::ops::Range<usize>, cutoff: f32) {
        let end = sample_range.end.min(self.0.len());
        let clamped = sample_range.start.min(end)..end;
        self.0[clamped].iter_mut().for_each(|sample| {
            *sample = if *sample < cutoff { *sample } else { 0.0 };
        });
    }

    pub fn zero_samples_below(&mut self, sample_range: std::ops::Range<usize>, cutoff: f32) {
        let end = sample_range.end.min(self.0.len());
        let clamped = sample_range.start.min(end)..end;
        self.0[clamped].iter_mut().for_each(|sample| {
            *sample = if *sample > cutoff { *sample } else { 0.0 };
        });
    }

    pub fn strip_silence(&mut self, sample_range: std::ops::Range<usize>, silence_threshold: f32) {
        let end = sample_range.end.min(self.0.len());
        let clamped = sample_range.start.min(end)..end;
        if clamped.is_empty() {
            return;
        }
        let retained: Vec<f32> = self.0[clamped.clone()]
            .iter()
            .copied()
            .filter(|sample| sample.abs() > silence_threshold)
            .collect();
        self.0.splice(clamped, retained);
    }

    pub fn trim_trailing_silence(&mut self, threshold: f32, num_channels: usize) {
        let num_channels = num_channels.max(1);
        let is_active = |sample: &f32| sample.is_finite() && sample.abs() > threshold;
        let frame_count = self.0.len() / num_channels;
        let last_active_frame = (0..frame_count).rev().find(|&frame| {
            self.0[frame * num_channels..(frame + 1) * num_channels]
                .iter()
                .any(is_active)
        });
        match last_active_frame {
            Some(frame) => self.0.truncate((frame + 1) * num_channels),
            None => self.0.clear(),
        }
    }

    pub fn to_decibel(&self) -> Vec<f32> {
        self.0
            .iter()
            .map(|sample| 20.0 * sample.abs().log10())
            .collect()
    }
}

impl From<AudioSamples> for Vec<f32> {
    fn from(samples: AudioSamples) -> Self {
        samples.into_vec()
    }
}

impl From<Vec<f32>> for AudioSamples {
    fn from(raw: Vec<f32>) -> Self {
        Self::new(raw)
    }
}

impl IntoIterator for AudioSamples {
    type Item = f32;
    type IntoIter = std::vec::IntoIter<f32>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

#[derive(Debug, Clone)]
#[must_use]
pub struct Audio {
    pub samples: AudioSamples,
    pub info: AudioInfo,
    pub inference_ms: Option<f32>,
}

impl Audio {
    pub fn new(samples: AudioSamples, sample_rate: usize, inference_ms: Option<f32>) -> Self {
        Self {
            samples,
            info: AudioInfo {
                sample_rate,
                num_channels: 1,
                sample_width: 2,
            },
            inference_ms,
        }
    }

    pub fn into_vec(self) -> Vec<f32> {
        self.samples.into_vec()
    }

    pub fn as_wave_bytes(&self) -> Vec<u8> {
        self.samples.as_wave_bytes()
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    pub fn duration_ms(&self) -> f32 {
        self.len() as f32 / self.info.sample_rate as f32 * 1000.0
    }

    pub fn inference_ms(&self) -> Option<f32> {
        self.inference_ms
    }

    pub fn real_time_factor(&self) -> Option<f32> {
        let inference_ms = self.inference_ms?;
        let duration_ms = self.duration_ms();
        if duration_ms == 0.0 {
            return Some(0.0);
        }
        Some(inference_ms / duration_ms)
    }

    pub fn save_to_file(&self, filename: &Path) -> Result<(), crate::WaveWriterError> {
        crate::write_wave_samples_to_file(
            filename,
            self.samples.to_i16_vec().iter(),
            self.info.sample_rate as u32,
            self.info.num_channels as u32,
            self.info.sample_width as u32,
        )
    }
}

impl IntoIterator for Audio {
    type Item = f32;
    type IntoIter = std::vec::IntoIter<f32>;

    fn into_iter(self) -> Self::IntoIter {
        self.samples.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fade_in() {
        let mut buffer = AudioSamples::from(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]);
        buffer.fade_in(4);
        assert_eq!(buffer.as_slice()[0], 0.0);
    }

    #[test]
    fn test_fade_out() {
        let mut buffer = AudioSamples::from(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]);
        buffer.fade_out(4);
        let last = buffer.len() - 1;
        assert_eq!(buffer.as_slice()[last], 0.0);
    }

    #[test]
    fn overlap_with_sums_the_overlap_region_instead_of_appending_it() {
        let base = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let mut head = AudioSamples::from(base.clone());
        let mut tail = AudioSamples::from(base.clone());
        head.overlap_with(&mut tail, 4);
        assert_eq!(head.len(), base.len() * 2 - 4);
    }

    #[test]
    fn overlap_with_does_not_zero_the_seam_samples() {
        let mut head = AudioSamples::from(vec![1.0; 4]);
        let mut tail = AudioSamples::from(vec![1.0; 4]);
        head.overlap_with(&mut tail, 4);
        assert!(head.into_vec().into_iter().all(|sample| sample != 0.0));
    }

    #[test]
    fn overlap_with_clamps_overlap_len_to_the_shorter_buffer() {
        let mut head = AudioSamples::from(vec![1.0, 2.0]);
        let mut tail = AudioSamples::from(vec![3.0, 4.0, 5.0]);
        head.overlap_with(&mut tail, 100);
        assert_eq!(head.len(), 3);
    }

    #[test]
    fn test_zero_samples_above() {
        let mut buffer = AudioSamples::from(vec![0.0, 0.1, 2.2, 0.0, 0.5, 0.0, 0.7, 0.0]);
        buffer.zero_samples_above(0..5, 0.5);
        let zeroed = buffer.into_iter().filter(|&sample| sample == 0.0).count();
        assert_eq!(zeroed, 6);
    }

    #[test]
    fn test_zero_samples_below() {
        let mut buffer = AudioSamples::from(vec![0.0, 0.1, 2.2, 0.0, 0.5, 0.0, 0.7, 0.0]);
        let whole = 0..buffer.len();
        buffer.zero_samples_below(whole, 0.5);
        let surviving = buffer.into_iter().filter(|&sample| sample != 0.0).count();
        assert_eq!(surviving, 2);
    }

    #[test]
    fn zero_samples_above_clamps_an_out_of_range_sample_range_instead_of_panicking() {
        let mut buffer = AudioSamples::from(vec![1.0, 2.0, 3.0]);
        buffer.zero_samples_above(1..100, 1.5);
        assert_eq!(buffer.into_vec(), vec![1.0, 0.0, 0.0]);
    }

    #[test]
    // clippy flags 10..1 as always-empty; deliberate here to prove clamping doesn't panic on it.
    #[allow(clippy::reversed_empty_ranges)]
    fn zero_samples_above_clamps_a_range_that_would_reverse_after_independent_end_clamping() {
        let mut buffer = AudioSamples::from(vec![1.0, 2.0, 3.0]);
        buffer.zero_samples_above(10..1, 0.0);
        assert_eq!(buffer.into_vec(), vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn test_normalize() {
        let mut buffer = AudioSamples::from(vec![0.0, 0.1, 2.2, 0.0, 0.5, 0.0, 0.7, 0.0]);
        buffer.normalize(1.0);
        let peak = buffer
            .into_vec()
            .into_iter()
            .max_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap();
        assert_eq!(peak, 1.0);
    }

    #[test]
    fn normalize_scales_by_true_peak_magnitude_for_a_negative_skewed_buffer() {
        let mut buffer = AudioSamples::from(vec![-5.0, 0.1]);
        buffer.normalize(1.0);
        let peak_magnitude = buffer
            .into_vec()
            .into_iter()
            .map(f32::abs)
            .max_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap();
        assert_eq!(peak_magnitude, 1.0);
    }

    #[test]
    fn zero_samples_above_zeroes_a_nan_sample() {
        let mut buffer = AudioSamples::from(vec![f32::NAN]);
        buffer.zero_samples_above(0..1, 0.5);
        assert_eq!(buffer.into_vec(), vec![0.0]);
    }

    #[test]
    fn zero_samples_below_zeroes_a_nan_sample() {
        let mut buffer = AudioSamples::from(vec![f32::NAN]);
        buffer.zero_samples_below(0..1, 0.5);
        assert_eq!(buffer.into_vec(), vec![0.0]);
    }

    #[test]
    fn to_i16_vec_degrades_a_nan_sample_to_silence_instead_of_panicking() {
        let buffer = AudioSamples::from(vec![f32::NAN, 1.0]);
        assert_eq!(buffer.to_i16_vec(), vec![0, 32767]);
    }

    #[test]
    fn normalize_degrades_a_nan_sample_to_silence_without_poisoning_the_rest() {
        let mut buffer = AudioSamples::from(vec![f32::NAN, 2.0]);
        buffer.normalize(1.0);
        let samples = buffer.into_vec();
        assert_eq!(samples[0], 0.0);
        assert_eq!(samples[1], 1.0);
    }

    #[test]
    fn zero_samples_above_zeroes_positive_infinity_and_keeps_negative_infinity() {
        let mut buffer = AudioSamples::from(vec![f32::INFINITY, f32::NEG_INFINITY]);
        buffer.zero_samples_above(0..2, 0.0);
        assert_eq!(buffer.into_vec(), vec![0.0, f32::NEG_INFINITY]);
    }

    #[test]
    fn zero_samples_below_keeps_positive_infinity_and_zeroes_negative_infinity() {
        let mut buffer = AudioSamples::from(vec![f32::INFINITY, f32::NEG_INFINITY]);
        buffer.zero_samples_below(0..2, 0.0);
        assert_eq!(buffer.into_vec(), vec![f32::INFINITY, 0.0]);
    }

    #[test]
    // clippy flags 10..1 as always-empty; deliberate here to prove clamping doesn't panic on it.
    #[allow(clippy::reversed_empty_ranges)]
    fn zero_samples_below_clamps_a_range_that_would_reverse_after_independent_end_clamping() {
        let mut buffer = AudioSamples::from(vec![1.0, 2.0, 3.0]);
        buffer.zero_samples_below(10..1, 0.0);
        assert_eq!(buffer.into_vec(), vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn normalize_degrades_an_infinite_sample_to_silence_without_poisoning_the_rest() {
        let mut buffer = AudioSamples::from(vec![f32::INFINITY, 0.5]);
        buffer.normalize(1.0);
        let samples = buffer.into_vec();
        assert_eq!(samples[0], 0.0);
        assert_eq!(samples[1], 0.5);
    }

    #[test]
    fn to_i16_vec_degrades_an_infinite_sample_to_silence_without_poisoning_the_rest() {
        let buffer = AudioSamples::from(vec![f32::INFINITY, 1.0]);
        assert_eq!(buffer.to_i16_vec(), vec![0, 32767]);
    }

    #[test]
    fn strip_silence_removes_samples_at_or_below_the_threshold() {
        let mut buffer = AudioSamples::from(vec![0.0, 0.1, 2.2, 0.0, 0.5, 0.0, 0.7, 0.0]);
        let whole = 0..buffer.len();
        buffer.strip_silence(whole, 0.0);
        assert_eq!(buffer.into_vec(), vec![0.1, 2.2, 0.5, 0.7]);
    }

    #[test]
    fn strip_silence_preserves_negative_samples_above_the_threshold() {
        let mut buffer = AudioSamples::from(vec![-0.9, 0.0, 0.8, -0.05]);
        let whole = 0..buffer.len();
        buffer.strip_silence(whole, 0.1);
        assert_eq!(buffer.into_vec(), vec![-0.9, 0.8]);
    }

    #[test]
    fn strip_silence_clamps_an_out_of_range_sample_range_instead_of_panicking() {
        let mut buffer = AudioSamples::from(vec![1.0, 0.0, 3.0]);
        buffer.strip_silence(1..100, 0.0);
        assert_eq!(buffer.into_vec(), vec![1.0, 3.0]);
    }

    #[test]
    fn trim_trailing_silence_removes_a_trailing_run_below_the_threshold() {
        let mut buffer = AudioSamples::from(vec![0.8, -0.6, 0.02, 0.01, -0.005, 0.0]);
        buffer.trim_trailing_silence(0.015, 1);
        assert_eq!(buffer.into_vec(), vec![0.8, -0.6, 0.02]);
    }

    #[test]
    fn trim_trailing_silence_leaves_a_mid_buffer_quiet_passage_untouched() {
        let mut buffer = AudioSamples::from(vec![0.8, 0.01, 0.0, 0.01, -0.6]);
        buffer.trim_trailing_silence(0.015, 1);
        assert_eq!(buffer.into_vec(), vec![0.8, 0.01, 0.0, 0.01, -0.6]);
    }

    #[test]
    fn trim_trailing_silence_leaves_an_all_loud_buffer_untouched() {
        let mut buffer = AudioSamples::from(vec![0.8, -0.6, 0.9]);
        buffer.trim_trailing_silence(0.015, 1);
        assert_eq!(buffer.into_vec(), vec![0.8, -0.6, 0.9]);
    }

    #[test]
    fn trim_trailing_silence_clears_a_buffer_entirely_below_the_threshold() {
        let mut buffer = AudioSamples::from(vec![0.01, -0.005, 0.0]);
        buffer.trim_trailing_silence(0.015, 1);
        assert_eq!(buffer.into_vec(), Vec::<f32>::new());
    }

    #[test]
    fn trim_trailing_silence_treats_a_trailing_infinity_as_silence() {
        let mut buffer = AudioSamples::from(vec![0.8, -0.6, f32::INFINITY, f32::NEG_INFINITY]);
        buffer.trim_trailing_silence(0.015, 1);
        assert_eq!(buffer.into_vec(), vec![0.8, -0.6]);
    }

    #[test]
    fn trim_trailing_silence_treats_a_trailing_nan_as_silence() {
        let mut buffer = AudioSamples::from(vec![0.8, -0.6, f32::NAN]);
        buffer.trim_trailing_silence(0.015, 1);
        assert_eq!(buffer.into_vec(), vec![0.8, -0.6]);
    }

    #[test]
    fn trim_trailing_silence_keeps_a_stereo_frame_with_one_active_channel() {
        let mut buffer = AudioSamples::from(vec![0.8, 0.0]);
        buffer.trim_trailing_silence(0.015, 2);
        assert_eq!(buffer.into_vec(), vec![0.8, 0.0]);
    }

    #[test]
    fn trim_trailing_silence_drops_a_stereo_frame_silent_in_every_channel() {
        let mut buffer = AudioSamples::from(vec![0.8, 0.6, 0.0, 0.0]);
        buffer.trim_trailing_silence(0.015, 2);
        assert_eq!(buffer.into_vec(), vec![0.8, 0.6]);
    }

    #[test]
    fn to_i16_vec_applies_the_same_gain_to_chunks_with_different_peaks() {
        let quiet = i32::from(AudioSamples::from(vec![0.1]).to_i16_vec()[0]);
        let loud = i32::from(AudioSamples::from(vec![0.4]).to_i16_vec()[0]);
        assert!((loud - 4 * quiet).abs() <= 4, "{quiet} {loud}");
    }

    #[test]
    fn to_i16_vec_keeps_a_near_silent_chunk_near_silent() {
        let hiss = AudioSamples::from(vec![0.002, -0.002]).to_i16_vec();
        assert!(hiss.iter().all(|sample| sample.abs() < 100), "{hiss:?}");
    }

    #[test]
    fn to_i16_vec_clamps_samples_pushed_past_full_scale_by_the_gain() {
        let buffer = AudioSamples::from(vec![1.0, -1.0]);
        assert_eq!(buffer.to_i16_vec(), vec![i16::MAX, i16::MIN]);
    }

    #[test]
    fn to_i16_vec_returns_empty_for_empty_samples() {
        let empty = AudioSamples::from(Vec::<f32>::new());
        assert_eq!(empty.to_i16_vec(), Vec::<i16>::new());
    }

    #[test]
    fn to_i16_vec_scales_all_zero_samples_without_dividing_by_zero() {
        let silence = AudioSamples::from(vec![0.0, 0.0, 0.0]);
        assert_eq!(silence.to_i16_vec(), vec![0, 0, 0]);
    }

    #[test]
    fn take_range_clamps_end_to_available_length() {
        let mut buffer = AudioSamples::from(vec![1.0, 2.0, 3.0]);
        let removed = buffer.take_range(1..100);
        assert_eq!(removed, vec![2.0, 3.0]);
        assert_eq!(buffer.len(), 1);
    }

    #[test]
    fn take_range_clamps_start_instead_of_panicking_when_start_exceeds_the_buffer() {
        let mut buffer = AudioSamples::from(vec![1.0, 2.0, 3.0]);
        let removed = buffer.take_range(5..100);
        assert_eq!(removed, Vec::<f32>::new());
        assert_eq!(buffer.len(), 3);
    }

    #[test]
    fn merge_appends_other_samples_in_order() {
        let mut first = AudioSamples::from(vec![1.0, 2.0]);
        let second = AudioSamples::from(vec![3.0, 4.0]);
        first.merge(second);
        assert_eq!(first.into_vec(), vec![1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn apply_hanning_window_tapers_first_sample_to_zero() {
        let mut buffer = AudioSamples::from(vec![1.0; 10]);
        buffer.apply_hanning_window().unwrap();
        let windowed = buffer.as_vec();
        assert_eq!(windowed[0], 0.0);
        assert!(windowed[5] > windowed[0]);
    }

    #[test]
    fn crossfade_attenuates_both_edges_symmetrically_and_leaves_the_middle_untouched() {
        let mut buffer = AudioSamples::from(vec![1.0; 10]);
        buffer.crossfade(4);
        let faded = buffer.as_vec();
        assert_eq!(faded[0], faded[9]);
        assert_eq!(faded[1], faded[8]);
        assert!(faded[0] < 1.0);
        assert_eq!(faded[4], 1.0);
        assert_eq!(faded[5], 1.0);
    }

    #[test]
    fn crossfade_clamps_fade_length_to_half_of_total_samples() {
        let mut buffer = AudioSamples::from(vec![1.0; 6]);
        buffer.crossfade(100);
        let faded = buffer.as_vec();
        assert_eq!(faded.len(), 6);
        assert!(faded.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn crossfade_is_a_noop_when_fade_length_resolves_below_two() {
        let unfaded = vec![1.0, 2.0, 3.0, 4.0];
        let mut buffer = AudioSamples::from(unfaded.clone());
        buffer.crossfade(1);
        assert_eq!(buffer.as_vec(), &unfaded);
    }

    #[test]
    fn crossfade_is_a_noop_for_zero_fade_samples() {
        let unfaded = vec![1.0, 2.0, 3.0, 4.0];
        let mut buffer = AudioSamples::from(unfaded.clone());
        buffer.crossfade(0);
        assert_eq!(buffer.as_vec(), &unfaded);
    }

    #[test]
    fn apply_hanning_window_on_empty_samples_is_a_noop() {
        let mut buffer = AudioSamples::from(Vec::<f32>::new());
        buffer.apply_hanning_window().unwrap();
        assert!(buffer.is_empty());
    }

    #[test]
    fn apply_hanning_window_errors_on_single_sample() {
        let mut buffer = AudioSamples::from(vec![1.0]);
        assert_eq!(
            buffer.apply_hanning_window(),
            Err(crate::AudioOpsError::InvalidWindowLength(1))
        );
    }

    #[test]
    fn to_decibel_converts_full_scale_amplitude_to_zero_db() {
        let buffer = AudioSamples::from(vec![1.0, 0.5]);
        let decibels = buffer.to_decibel();
        assert_eq!(decibels[0], 0.0);
        assert!(decibels[1] < 0.0);
    }

    #[test]
    fn to_decibel_of_zero_amplitude_is_negative_infinity() {
        let silence = AudioSamples::from(vec![0.0]);
        assert_eq!(silence.to_decibel()[0], f32::NEG_INFINITY);
    }

    #[test]
    fn real_time_factor_returns_none_without_inference_time() {
        let clip = Audio::new(AudioSamples::from(vec![0.0; 100]), 100, None);
        assert_eq!(clip.real_time_factor(), None);
    }

    #[test]
    fn real_time_factor_returns_zero_for_zero_duration_audio() {
        let clip = Audio::new(AudioSamples::from(Vec::new()), 100, Some(5.0));
        assert_eq!(clip.real_time_factor(), Some(0.0));
    }

    #[test]
    fn real_time_factor_divides_inference_ms_by_duration_ms() {
        let clip = Audio::new(AudioSamples::from(vec![0.0; 100]), 100, Some(50.0));
        assert_eq!(clip.real_time_factor(), Some(0.05));
    }

    #[test]
    fn crossfade_golden_values_for_a_known_input() {
        let mut buffer = AudioSamples::from(vec![1.0; 8]);
        buffer.crossfade(2);
        assert_eq!(
            buffer.as_vec(),
            &vec![0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.0]
        );
    }

    #[test]
    fn overlap_with_golden_values_for_a_known_input() {
        let mut head = AudioSamples::from(vec![1.0, 2.0]);
        let mut tail = AudioSamples::from(vec![3.0, 4.0]);
        head.overlap_with(&mut tail, 2);

        assert_eq!(head.as_vec(), &vec![1.0, 4.0]);
    }

    #[test]
    fn overlap_with_a_single_sample_overlap_averages_instead_of_using_a_degenerate_ramp() {
        let mut head = AudioSamples::from(vec![1.0, 2.0]);
        let mut tail = AudioSamples::from(vec![3.0, 4.0]);
        head.overlap_with(&mut tail, 1);
        assert_eq!(head.into_vec(), vec![1.0, 2.5, 4.0]);
    }

    #[test]
    fn overlap_with_gains_sum_to_exactly_one_at_every_offset() {
        let mut head = AudioSamples::from(vec![1.0; 8]);
        let mut tail = AudioSamples::from(vec![1.0; 8]);
        head.overlap_with(&mut tail, 8);
        assert!(head.into_vec().into_iter().all(|s| s == 1.0));
    }

    #[test]
    fn to_i16_vec_golden_values_for_a_known_input() {
        let buffer = AudioSamples::from(vec![-1.0, 0.5, 1.0]);
        assert_eq!(buffer.to_i16_vec(), vec![-32768, 20479, 32767]);
    }
}

#[cfg(test)]
mod proptest_tests {
    use super::*;
    use proptest::prelude::*;

    fn sample() -> impl Strategy<Value = f32> {
        -1000.0f32..1000.0f32
    }

    fn samples(max_len: usize) -> impl Strategy<Value = Vec<f32>> {
        prop::collection::vec(sample(), 0..=max_len)
    }

    proptest! {
        #[test]
        fn overlap_with_length_matches_the_clamped_overlap_formula(
            head in samples(20),
            tail in samples(20),
            overlap_len in 0usize..30,
        ) {
            let head_len = head.len();
            let tail_len = tail.len();
            let mut head = AudioSamples::from(head);
            let mut tail = AudioSamples::from(tail);
            head.overlap_with(&mut tail, overlap_len);
            let clamped_overlap = overlap_len.min(head_len).min(tail_len);
            prop_assert_eq!(head.len(), head_len + tail_len - clamped_overlap);
        }

        #[test]
        fn overlap_with_zero_overlap_len_is_a_plain_append(
            head in samples(20),
            tail in samples(20),
        ) {
            let expected: Vec<f32> = head.iter().copied().chain(tail.iter().copied()).collect();
            let mut head = AudioSamples::from(head);
            let mut tail = AudioSamples::from(tail);
            head.overlap_with(&mut tail, 0);
            prop_assert_eq!(head.into_vec(), expected);
        }

        #[test]
        fn overlap_with_two_silent_buffers_stays_silent(
            head_len in 0usize..20,
            tail_len in 0usize..20,
            overlap_len in 0usize..30,
        ) {
            let mut head = AudioSamples::from(vec![0.0f32; head_len]);
            let mut tail = AudioSamples::from(vec![0.0f32; tail_len]);
            head.overlap_with(&mut tail, overlap_len);
            prop_assert!(head.into_vec().into_iter().all(|s| s == 0.0));
        }

        #[test]
        fn strip_silence_never_grows_and_leaves_samples_outside_the_range_untouched(
            data in samples(30),
            start in 0usize..30,
            len in 0usize..30,
            threshold in 0.0f32..2.0,
        ) {
            let end = (start + len).min(data.len());
            let start = start.min(end);
            let before_len = data.len();
            let prefix = data[..start].to_vec();
            let suffix = data[end..].to_vec();
            let expected_retained: Vec<f32> = data[start..end]
                .iter()
                .copied()
                .filter(|s| s.abs() > threshold)
                .collect();

            let mut buffer = AudioSamples::from(data);
            buffer.strip_silence(start..end, threshold);
            let result = buffer.into_vec();

            prop_assert!(result.len() <= before_len);
            prop_assert_eq!(&result[..start], &prefix[..]);
            prop_assert_eq!(&result[start..start + expected_retained.len()], &expected_retained[..]);
            prop_assert_eq!(&result[start + expected_retained.len()..], &suffix[..]);
        }
    }
}
