//! Safe Rust port of the Sonic speech-rate library.
//!
//! Original C library: Copyright 2010 Bill Cox, licensed under the Apache License 2.0.
//! Behaviour matches `sonic.c` for quality 0, chord pitch off, rate 1.0 and float I/O.
#![allow(clippy::indexing_slicing)]
#![allow(dead_code)]

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

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SonicError {
    SampleRateOutOfRange(u32),
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
        self.copy_input_to_output(frames);
        if self.volume != 1.0 {
            scale_samples(&mut self.output[original * self.channels..], self.volume);
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
