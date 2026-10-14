//! Safe Rust port of the Sonic speech-rate library.
//!
//! Original C library: Copyright 2010 Bill Cox, licensed under the Apache License 2.0.
//! Behaviour matches `sonic.c` for quality 0, chord pitch off, rate 1.0 and float I/O.
#![allow(clippy::indexing_slicing)]

use std::fmt;

const MIN_PITCH_HZ: i32 = 65;
const MAX_PITCH_HZ: i32 = 400;
const AMDF_FREQ: i32 = 4000;
const MIN_VOLUME: f32 = 0.01;
const MAX_VOLUME: f32 = 100.0;
const MIN_SPEED: f32 = 0.05;
const MAX_SPEED: f32 = 20.0;
const MIN_PITCH_SETTING: f32 = 0.05;
const MAX_PITCH_SETTING: f32 = 20.0;
const MIN_SAMPLE_RATE: u32 = 1000;
const MAX_SAMPLE_RATE: u32 = 500_000;
const MAX_CHANNELS: u32 = 32;
const SINC_FILTER_POINTS: usize = 12;

static SINC_TABLE: [i16; 601] = [
    0, 0, 0, 0, 0, 0, 0, -1, -1, -2, -2, -3, -4, -6, -7, -9, -10, -12, -14, -17, -19, -21, -24,
    -26, -29, -32, -34, -37, -40, -42, -44, -47, -48, -50, -51, -52, -53, -53, -53, -52, -50, -48,
    -46, -43, -39, -34, -29, -22, -16, -8, 0, 9, 19, 29, 41, 53, 65, 79, 92, 107, 121, 137, 152,
    168, 184, 200, 215, 231, 247, 262, 276, 291, 304, 317, 328, 339, 348, 357, 363, 369, 372, 374,
    375, 373, 369, 363, 355, 345, 332, 318, 300, 281, 259, 234, 208, 178, 147, 113, 77, 39, 0, -41,
    -85, -130, -177, -225, -274, -324, -375, -426, -478, -530, -581, -632, -682, -731, -779, -825,
    -870, -912, -951, -989, -1023, -1053, -1080, -1104, -1123, -1138, -1149, -1154, -1155, -1151,
    -1141, -1125, -1105, -1078, -1046, -1007, -963, -913, -857, -796, -728, -655, -576, -492, -403,
    -309, -210, -107, 0, 111, 225, 342, 462, 584, 708, 833, 958, 1084, 1209, 1333, 1455, 1575,
    1693, 1807, 1916, 2022, 2122, 2216, 2304, 2384, 2457, 2522, 2579, 2625, 2663, 2689, 2706, 2711,
    2705, 2687, 2657, 2614, 2559, 2491, 2411, 2317, 2211, 2092, 1960, 1815, 1658, 1489, 1308, 1115,
    912, 698, 474, 241, 0, -249, -506, -769, -1037, -1310, -1586, -1864, -2144, -2424, -2703,
    -2980, -3254, -3523, -3787, -4043, -4291, -4529, -4757, -4972, -5174, -5360, -5531, -5685,
    -5819, -5935, -6029, -6101, -6150, -6175, -6175, -6149, -6096, -6015, -5905, -5767, -5599,
    -5401, -5172, -4912, -4621, -4298, -3944, -3558, -3141, -2693, -2214, -1705, -1166, -597, 0,
    625, 1277, 1955, 2658, 3386, 4135, 4906, 5697, 6506, 7332, 8173, 9027, 9893, 10769, 11654,
    12544, 13439, 14335, 15232, 16128, 17019, 17904, 18782, 19649, 20504, 21345, 22170, 22977,
    23763, 24527, 25268, 25982, 26669, 27327, 27953, 28547, 29107, 29632, 30119, 30569, 30979,
    31349, 31678, 31964, 32208, 32408, 32565, 32677, 32744, 32767, 32744, 32677, 32565, 32408,
    32208, 31964, 31678, 31349, 30979, 30569, 30119, 29632, 29107, 28547, 27953, 27327, 26669,
    25982, 25268, 24527, 23763, 22977, 22170, 21345, 20504, 19649, 18782, 17904, 17019, 16128,
    15232, 14335, 13439, 12544, 11654, 10769, 9893, 9027, 8173, 7332, 6506, 5697, 4906, 4135, 3386,
    2658, 1955, 1277, 625, 0, -597, -1166, -1705, -2214, -2693, -3141, -3558, -3944, -4298, -4621,
    -4912, -5172, -5401, -5599, -5767, -5905, -6015, -6096, -6149, -6175, -6175, -6150, -6101,
    -6029, -5935, -5819, -5685, -5531, -5360, -5174, -4972, -4757, -4529, -4291, -4043, -3787,
    -3523, -3254, -2980, -2703, -2424, -2144, -1864, -1586, -1310, -1037, -769, -506, -249, 0, 241,
    474, 698, 912, 1115, 1308, 1489, 1658, 1815, 1960, 2092, 2211, 2317, 2411, 2491, 2559, 2614,
    2657, 2687, 2705, 2711, 2706, 2689, 2663, 2625, 2579, 2522, 2457, 2384, 2304, 2216, 2122, 2022,
    1916, 1807, 1693, 1575, 1455, 1333, 1209, 1084, 958, 833, 708, 584, 462, 342, 225, 111, 0,
    -107, -210, -309, -403, -492, -576, -655, -728, -796, -857, -913, -963, -1007, -1046, -1078,
    -1105, -1125, -1141, -1151, -1155, -1154, -1149, -1138, -1123, -1104, -1080, -1053, -1023,
    -989, -951, -912, -870, -825, -779, -731, -682, -632, -581, -530, -478, -426, -375, -324, -274,
    -225, -177, -130, -85, -41, 0, 39, 77, 113, 147, 178, 208, 234, 259, 281, 300, 318, 332, 345,
    355, 363, 369, 373, 375, 374, 372, 369, 363, 357, 348, 339, 328, 317, 304, 291, 276, 262, 247,
    231, 215, 200, 184, 168, 152, 137, 121, 107, 92, 79, 65, 53, 41, 29, 19, 9, 0, -8, -16, -22,
    -29, -34, -39, -43, -46, -48, -50, -52, -53, -53, -53, -52, -51, -50, -48, -47, -44, -42, -40,
    -37, -34, -32, -29, -26, -24, -21, -19, -17, -14, -12, -10, -9, -7, -6, -4, -3, -2, -2, -1, -1,
    0, 0, 0, 0, 0, 0, 0,
];

/// Rejected stream dimensions.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SonicError {
    /// Sample rate outside the supported range.
    SampleRateOutOfRange(u32),
    /// Channel count outside the supported range.
    ChannelsOutOfRange(u32),
}

impl fmt::Display for SonicError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SampleRateOutOfRange(rate) => write!(
                f,
                "sample rate {rate} is outside {MIN_SAMPLE_RATE}..={MAX_SAMPLE_RATE}"
            ),
            Self::ChannelsOutOfRange(count) => {
                write!(f, "channel count {count} is outside 1..={MAX_CHANNELS}")
            }
        }
    }
}

impl std::error::Error for SonicError {}

/// NaN maps to `min`, matching the C `CLAMP` macro.
fn clamp(value: f32, min: f32, max: f32) -> f32 {
    if value >= min {
        if value <= max {
            value
        } else {
            max
        }
    } else {
        min
    }
}

/// Streaming pitch-synchronous speed, pitch and volume changer.
#[derive(Debug)]
pub struct SonicStream {
    input: Vec<i16>,
    output: Vec<i16>,
    pitch_buffer: Vec<i16>,
    down_sample: Vec<i16>,
    speed: f32,
    volume: f32,
    pitch: f32,
    sample_period: f32,
    input_play_time: f32,
    time_error: f32,
    old_rate_position: i32,
    new_rate_position: i32,
    channels: usize,
    sample_rate: i32,
    min_period: i32,
    max_period: i32,
    max_required: usize,
    prev_period: i32,
    prev_min_diff: i32,
}

impl SonicStream {
    /// Errors with [`SonicError`] when the dimensions are out of range.
    pub fn new(sample_rate: u32, channels: u32) -> Result<Self, SonicError> {
        if !(MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(&sample_rate) {
            return Err(SonicError::SampleRateOutOfRange(sample_rate));
        }
        if !(1..=MAX_CHANNELS).contains(&channels) {
            return Err(SonicError::ChannelsOutOfRange(channels));
        }
        let rate = sample_rate as i32;
        let max_period = rate / MIN_PITCH_HZ;
        let max_required = 2 * max_period as usize;
        Ok(Self {
            input: Vec::new(),
            output: Vec::new(),
            pitch_buffer: Vec::new(),
            down_sample: vec![0; max_required],
            speed: 1.0,
            volume: 1.0,
            pitch: 1.0,
            sample_period: (1.0f64 / f64::from(rate)) as f32,
            input_play_time: 0.0,
            time_error: 0.0,
            old_rate_position: 0,
            new_rate_position: 0,
            channels: channels as usize,
            sample_rate: rate,
            min_period: rate / MAX_PITCH_HZ,
            max_period,
            max_required,
            prev_period: 0,
            prev_min_diff: 0,
        })
    }

    pub fn set_speed(&mut self, speed: f32) {
        self.speed = clamp(speed, MIN_SPEED, MAX_SPEED);
    }

    pub fn set_pitch(&mut self, pitch: f32) {
        self.pitch = clamp(pitch, MIN_PITCH_SETTING, MAX_PITCH_SETTING);
        self.old_rate_position = 0;
        self.new_rate_position = 0;
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.volume = clamp(volume, MIN_VOLUME, MAX_VOLUME);
    }

    /// Buffers whole frames of `samples` and processes them; a trailing
    /// partial frame is dropped.
    pub fn write_f32(&mut self, samples: &[f32]) {
        let frames = samples.len() / self.channels;
        let whole = frames * self.channels;
        self.input
            .extend(samples[..whole].iter().map(|&s| (s * 32767.0) as i16));
        let speed = self.speed / self.pitch;
        self.input_play_time += frames as f32 * self.sample_period / speed;
        self.process_stream_input();
    }

    /// Forces output from whatever is buffered; flushing mid-word can distort.
    pub fn flush(&mut self) {
        let remaining = self.input_frames();
        let speed = self.speed / self.pitch;
        let rate = self.pitch;
        let expected = self.output_frames()
            + ((remaining as f32 / speed + self.pitch_frames() as f32) / rate + 0.5) as usize;
        let padding = 2 * self.max_required;
        self.input
            .resize(self.input.len() + padding * self.channels, 0);
        self.input_play_time += padding as f32 * self.sample_period / speed;
        self.process_stream_input();
        self.output
            .truncate(expected.min(self.output_frames()) * self.channels);
        self.input.clear();
        self.input_play_time = 0.0;
        self.time_error = 0.0;
        self.pitch_buffer.clear();
    }

    pub fn available_frames(&self) -> usize {
        self.output_frames()
    }

    /// Removes up to `max_frames` frames from the output and returns them interleaved.
    pub fn read_f32(&mut self, max_frames: usize) -> Vec<f32> {
        let count = self.output_frames().min(max_frames) * self.channels;
        self.output
            .drain(..count)
            .map(|s| f32::from(s) / 32767.0)
            .collect()
    }

    fn input_frames(&self) -> usize {
        self.input.len() / self.channels
    }

    fn output_frames(&self) -> usize {
        self.output.len() / self.channels
    }

    fn pitch_frames(&self) -> usize {
        self.pitch_buffer.len() / self.channels
    }

    fn process_stream_input(&mut self) {
        let original = self.output_frames();
        let frames = self.input_frames();
        if frames == 0 {
            return;
        }
        let local_speed = frames as f32 * self.sample_period / self.input_play_time;
        if f64::from(local_speed) > 1.00001 || f64::from(local_speed) < 0.99999 {
            self.change_speed(local_speed);
        } else {
            self.copy_input_to_output(frames);
        }
        let rate = self.pitch;
        if rate != 1.0 {
            self.adjust_rate(rate, original);
        }
        if self.volume != 1.0 {
            scale_samples(&mut self.output[original * self.channels..], self.volume);
        }
    }

    fn adjust_rate(&mut self, rate: f32, original_frames: usize) {
        let mut new_rate = (self.sample_rate as f32 / rate) as i32;
        let mut old_rate = self.sample_rate;
        while new_rate > (1 << 14) || old_rate > (1 << 14) {
            new_rate >>= 1;
            old_rate >>= 1;
        }
        if self.output_frames() == original_frames {
            return;
        }
        self.move_new_samples_to_pitch_buffer(original_frames);
        let ch = self.channels;
        let limit = self.pitch_frames() as i32 - SINC_FILTER_POINTS as i32;
        let mut position = 0i32;
        while position < limit {
            while (self.old_rate_position + 1) * new_rate > self.new_rate_position * old_rate {
                let base = position as usize * ch;
                for channel in 0..ch {
                    let window = &self.pitch_buffer[base + channel..];
                    let sample = self.interpolate(window, old_rate, new_rate);
                    self.output.push(sample);
                }
                self.new_rate_position += 1;
            }
            self.old_rate_position += 1;
            if self.old_rate_position == old_rate {
                self.old_rate_position = 0;
                self.new_rate_position = 0;
            }
            position += 1;
        }
        self.remove_pitch_samples(position as usize);
    }

    fn move_new_samples_to_pitch_buffer(&mut self, original_frames: usize) {
        let moved = self.output.split_off(original_frames * self.channels);
        self.pitch_buffer.extend(moved);
    }

    fn remove_pitch_samples(&mut self, frames: usize) {
        let count = (frames * self.channels).min(self.pitch_buffer.len());
        self.pitch_buffer.drain(..count);
    }

    fn interpolate(&self, input: &[i16], old_rate: i32, new_rate: i32) -> i16 {
        let position = self.new_rate_position * old_rate;
        let left = self.old_rate_position * new_rate;
        let right = (self.old_rate_position + 1) * new_rate;
        let ratio = right - position - 1;
        let width = right - left;
        let mut total = 0i32;
        let mut overflow_count = 0i32;
        for i in 0..SINC_FILTER_POINTS {
            let weight = find_sinc_coefficient(i as i32, ratio, width);
            let value = i32::from(input[i * self.channels]) * weight;
            let old_sign = sign(total);
            total = total.wrapping_add(value);
            if old_sign != sign(total) && sign(value) == old_sign {
                overflow_count += old_sign;
            }
        }
        match overflow_count.signum() {
            1 => i16::MAX,
            -1 => i16::MIN,
            _ => (total >> 16) as i16,
        }
    }

    fn change_speed(&mut self, speed: f32) {
        let num_frames = self.input_frames();
        if num_frames < self.max_required {
            return;
        }
        let mut position = 0usize;
        loop {
            let copying = (speed > 1.0 && speed < 2.0 && self.time_error < 0.0)
                || (speed < 1.0 && speed > 0.5 && self.time_error > 0.0);
            if copying {
                position += self.copy_unmodified_samples(speed, position);
            } else {
                let period = self.find_pitch_period(position, true) as usize;
                let new_samples = if speed > 1.0 {
                    let added = self.skip_pitch_period(speed, position, period);
                    position += period + added;
                    if speed < 2.0 {
                        self.time_error += added as f32 * self.sample_period
                            - (period + added) as f32 * self.input_play_time / num_frames as f32;
                    }
                    added
                } else {
                    let added = self.insert_pitch_period(speed, position, period);
                    position += added;
                    if speed > 0.5 {
                        self.time_error += (period + added) as f32 * self.sample_period
                            - added as f32 * self.input_play_time / num_frames as f32;
                    }
                    added
                };
                if new_samples == 0 {
                    return;
                }
            }
            if position + self.max_required > num_frames {
                break;
            }
        }
        self.remove_input_samples(position);
    }

    fn copy_unmodified_samples(&mut self, speed: f32, position: usize) -> usize {
        let available = self.input_frames() - position;
        let to_copy = 1.0f64
            - f64::from(self.time_error * speed)
                / (f64::from(self.sample_period) * (f64::from(speed) - 1.0));
        let to_copy = to_copy as f32;
        let copied = if to_copy > available as f32 {
            available
        } else {
            (to_copy as i32).max(0) as usize
        };
        let start = position * self.channels;
        self.output
            .extend_from_slice(&self.input[start..start + copied * self.channels]);
        self.time_error = (f64::from(self.time_error)
            + f64::from(copied as f32 * self.sample_period) * (f64::from(speed) - 1.0)
                / f64::from(speed)) as f32;
        copied
    }

    fn skip_pitch_period(&mut self, speed: f32, position: usize, period: usize) -> usize {
        let new_samples = if speed >= 2.0 {
            (period as f32 / (speed - 1.0)) as usize
        } else {
            period
        };
        let ch = self.channels;
        let out_start = self.output.len();
        self.output.resize(out_start + new_samples * ch, 0);
        let samples = &self.input[position * ch..];
        overlap_add(
            new_samples,
            ch,
            &mut self.output[out_start..],
            samples,
            &samples[period * ch..],
        );
        new_samples
    }

    fn insert_pitch_period(&mut self, speed: f32, position: usize, period: usize) -> usize {
        let new_samples = if speed <= 0.5 {
            (period as f32 * speed / (1.0 - speed)) as usize
        } else {
            period
        };
        let ch = self.channels;
        let samples = &self.input[position * ch..];
        self.output.extend_from_slice(&samples[..period * ch]);
        let out_start = self.output.len();
        self.output.resize(out_start + new_samples * ch, 0);
        overlap_add(
            new_samples,
            ch,
            &mut self.output[out_start..],
            &samples[period * ch..],
            samples,
        );
        new_samples
    }

    fn find_pitch_period(&mut self, position: usize, prefer_new_period: bool) -> i32 {
        let skip = if self.sample_rate > AMDF_FREQ {
            self.sample_rate / AMDF_FREQ
        } else {
            1
        };
        let ch = self.channels;
        let samples = &self.input[position * ch..];
        let (mut period, mut min_diff, mut max_diff);
        if ch == 1 && skip == 1 {
            (period, min_diff, max_diff) =
                find_pitch_period_in_range(samples, self.min_period, self.max_period);
        } else {
            down_sample_input(
                samples,
                &mut self.down_sample,
                self.max_required / skip as usize,
                ch * skip as usize,
            );
            (period, min_diff, max_diff) = find_pitch_period_in_range(
                &self.down_sample,
                self.min_period / skip,
                self.max_period / skip,
            );
            if skip != 1 {
                period *= skip;
                let low = (period - (skip << 2)).max(self.min_period);
                let high = (period + (skip << 2)).min(self.max_period);
                if ch == 1 {
                    (period, min_diff, max_diff) = find_pitch_period_in_range(samples, low, high);
                } else {
                    down_sample_input(samples, &mut self.down_sample, self.max_required, ch);
                    (period, min_diff, max_diff) =
                        find_pitch_period_in_range(&self.down_sample, low, high);
                }
            }
        }
        let result = if self.prev_period_better(min_diff, max_diff, prefer_new_period) {
            self.prev_period
        } else {
            period
        };
        self.prev_min_diff = min_diff;
        self.prev_period = period;
        result
    }

    fn prev_period_better(&self, min_diff: i32, max_diff: i32, prefer_new_period: bool) -> bool {
        if min_diff == 0 || self.prev_period == 0 {
            return false;
        }
        if prefer_new_period {
            max_diff <= min_diff * 3 && min_diff * 2 > self.prev_min_diff * 3
        } else {
            min_diff > self.prev_min_diff
        }
    }

    fn copy_input_to_output(&mut self, frames: usize) {
        self.output
            .extend_from_slice(&self.input[..frames * self.channels]);
        self.remove_input_samples(frames);
    }

    fn remove_input_samples(&mut self, position: usize) {
        let total = self.input_frames();
        let remaining = total.saturating_sub(position);
        self.input
            .drain(..(position * self.channels).min(self.input.len()));
        self.input_play_time = (self.input_play_time * remaining as f32) / total as f32;
    }
}

fn scale_samples(samples: &mut [i16], volume: f32) {
    let fixed_point = (volume * 256.0) as i32;
    for sample in samples {
        let value = (i32::from(*sample) * fixed_point) >> 8;
        *sample = value.clamp(-32767, 32767) as i16;
    }
}

fn down_sample_input(samples: &[i16], out: &mut [i16], num_values: usize, per_value: usize) {
    for (value, chunk) in out
        .iter_mut()
        .zip(samples.chunks_exact(per_value))
        .take(num_values)
    {
        let sum: i32 = chunk.iter().map(|&s| i32::from(s)).sum();
        *value = (sum / per_value as i32) as i16;
    }
}

fn find_pitch_period_in_range(
    samples: &[i16],
    min_period: i32,
    max_period: i32,
) -> (i32, i32, i32) {
    let mut best_period = 0i32;
    let mut worst_period = 255i32;
    let mut min_diff = 1u64;
    let mut max_diff = 0u64;
    for period in min_period..=max_period {
        let p = period as usize;
        let diff: u64 = samples[..p]
            .iter()
            .zip(&samples[p..2 * p])
            .map(|(&s, &q)| u64::from(s.abs_diff(q)))
            .sum();
        if best_period == 0 || diff * (best_period as u64) < min_diff * (period as u64) {
            min_diff = diff;
            best_period = period;
        }
        if diff * (worst_period as u64) > max_diff * (period as u64) {
            max_diff = diff;
            worst_period = period;
        }
    }
    let best = best_period.max(1) as u64;
    (
        best_period,
        (min_diff / best) as i32,
        (max_diff / worst_period as u64) as i32,
    )
}

fn overlap_add(
    num_samples: usize,
    channels: usize,
    out: &mut [i16],
    ramp_down: &[i16],
    ramp_up: &[i16],
) {
    let n = num_samples as i32;
    for t in 0..num_samples {
        for ch in 0..channels {
            let idx = t * channels + ch;
            let value = (i32::from(ramp_down[idx]) * (n - t as i32)
                + i32::from(ramp_up[idx]) * t as i32)
                / n;
            out[idx] = value as i16;
        }
    }
}

fn find_sinc_coefficient(i: i32, ratio: i32, width: i32) -> i32 {
    let lobe_points = (SINC_TABLE.len() as i32 - 1) / SINC_FILTER_POINTS as i32;
    let left = i * lobe_points + (ratio * lobe_points) / width;
    let right = left + 1;
    let position = i * lobe_points * width + ratio * lobe_points - left * width;
    let left_val = i32::from(SINC_TABLE[left as usize]);
    let right_val = i32::from(SINC_TABLE[right as usize]);
    ((left_val * (width - position) + right_val * position) * 2) / width
}

fn sign(value: i32) -> i32 {
    if value >= 0 {
        1
    } else {
        -1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(frames: usize) -> Vec<f32> {
        (0..frames).map(|n| (n as f32 * 0.05).sin() * 0.5).collect()
    }

    fn process(stream: &mut SonicStream, input: &[f32]) -> Vec<f32> {
        stream.write_f32(input);
        stream.flush();
        let available = stream.available_frames();
        stream.read_f32(available)
    }

    #[test]
    fn out_of_range_dimensions_are_errors() {
        assert_eq!(
            SonicStream::new(999, 1).err(),
            Some(SonicError::SampleRateOutOfRange(999))
        );
        assert_eq!(
            SonicStream::new(500_001, 1).err(),
            Some(SonicError::SampleRateOutOfRange(500_001))
        );
        assert_eq!(
            SonicStream::new(22_050, 0).err(),
            Some(SonicError::ChannelsOutOfRange(0))
        );
        assert_eq!(
            SonicStream::new(22_050, 33).err(),
            Some(SonicError::ChannelsOutOfRange(33))
        );
    }

    #[test]
    fn out_of_range_float_input_saturates() {
        let mut stream = SonicStream::new(22_050, 1).unwrap();
        stream.write_f32(&[2.0, -2.0, f32::NAN, f32::INFINITY]);
        let converted: Vec<i16> = stream.output.clone();
        assert_eq!(converted, vec![i16::MAX, i16::MIN, 0, i16::MAX]);
    }

    #[test]
    fn nan_parameters_clamp_to_minimum() {
        let mut stream = SonicStream::new(22_050, 1).unwrap();
        stream.set_speed(f32::NAN);
        stream.set_pitch(f32::NAN);
        stream.set_volume(f32::NAN);
        assert_eq!(
            (stream.speed, stream.pitch, stream.volume),
            (MIN_SPEED, MIN_PITCH_SETTING, MIN_VOLUME)
        );
    }

    #[test]
    fn chunked_reads_equal_single_read() {
        let input = tone(30_000);
        let mut whole = SonicStream::new(22_050, 1).unwrap();
        whole.set_speed(1.7);
        let expected = process(&mut whole, &input);

        let mut chunked = SonicStream::new(22_050, 1).unwrap();
        chunked.set_speed(1.7);
        chunked.write_f32(&input);
        chunked.flush();
        let mut actual = Vec::new();
        loop {
            let part = chunked.read_f32(137);
            if part.is_empty() {
                break;
            }
            actual.extend(part);
        }
        assert_eq!(actual, expected);
    }

    #[test]
    fn write_after_flush_and_double_flush_do_not_panic() {
        let mut stream = SonicStream::new(16_000, 2).unwrap();
        stream.set_speed(2.5);
        stream.set_pitch(0.7);
        stream.write_f32(&tone(8_000));
        stream.flush();
        stream.flush();
        stream.write_f32(&tone(8_000));
        stream.flush();
        assert!(stream.available_frames() > 0);
    }
}
