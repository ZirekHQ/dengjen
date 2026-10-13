#![allow(dead_code)]

use dengjen_audio_ops::SonicStream;

#[derive(Clone, Copy, Debug)]
pub struct Case {
    pub sample_rate: u32,
    pub channels: u32,
    pub speed: f32,
    pub pitch: f32,
    pub volume: f32,
}

impl Case {
    pub fn id(&self) -> String {
        format!(
            "sr{}-ch{}-s{}-p{}-v{}",
            self.sample_rate, self.channels, self.speed, self.pitch, self.volume
        )
    }
}

pub fn signal(sample_rate: u32, channels: u32, frames: usize) -> Vec<f32> {
    let mut state: u32 = 0x2545_F491;
    let mut next_noise = move || {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        ((state >> 8) as f32 / 16_777_216.0 - 0.5) * 0.04
    };
    let rate = sample_rate as f32;
    let tau = 2.0 * std::f32::consts::PI;
    let mut out = Vec::with_capacity(frames * channels as usize);
    for n in 0..frames {
        let t = n as f32 / rate;
        let f0 = 140.0 + 25.0 * (tau * 3.0 * t).sin();
        let phase = tau * f0 * t;
        let envelope = 0.5 + 0.4 * (tau * 4.0 * t).sin();
        let voiced =
            envelope * (0.6 * phase.sin() + 0.25 * (2.0 * phase).sin() + 0.1 * (3.0 * phase).sin());
        for ch in 0..channels {
            out.push((voiced * (1.0 - 0.1 * ch as f32) + next_noise()).clamp(-0.95, 0.95));
        }
    }
    out
}

pub fn run_rust(case: &Case, input: &[f32]) -> Vec<f32> {
    let mut stream = SonicStream::new(case.sample_rate, case.channels).expect("valid dimensions");
    stream.set_speed(case.speed);
    stream.set_pitch(case.pitch);
    stream.set_volume(case.volume);
    stream.write_f32(input);
    stream.flush();
    let available = stream.available_frames();
    stream.read_f32(available)
}

pub fn matrix(speeds: &[f32], pitches: &[f32], volumes: &[f32]) -> Vec<Case> {
    let mut cases = Vec::new();
    for &sample_rate in &[16_000u32, 22_050, 44_100] {
        for &channels in &[1u32, 2] {
            for &speed in speeds {
                for &pitch in pitches {
                    for &volume in volumes {
                        cases.push(Case {
                            sample_rate,
                            channels,
                            speed,
                            pitch,
                            volume,
                        });
                    }
                }
            }
        }
    }
    cases
}

pub fn edge_cases() -> Vec<(String, Case, Vec<f32>)> {
    let base = Case {
        sample_rate: 22_050,
        channels: 1,
        speed: 1.5,
        pitch: 1.25,
        volume: 0.8,
    };
    let window = 2 * (base.sample_rate as usize / 65);
    let mut cases: Vec<_> = [0, 1, 7, window - 1, window, window + 1, 3 * window + 13]
        .into_iter()
        .map(|frames| {
            (
                format!("edge-frames-{frames}"),
                base,
                signal(base.sample_rate, 1, frames),
            )
        })
        .collect();
    let stereo = Case {
        channels: 2,
        ..base
    };
    let mut partial = signal(stereo.sample_rate, 2, 5000);
    partial.push(0.25);
    cases.push(("edge-partial-frame".to_string(), stereo, partial));
    cases
}

pub fn fnv1a(samples: &[f32]) -> u64 {
    samples
        .iter()
        .fold(0xcbf2_9ce4_8422_2325u64, |hash, sample| {
            sample.to_bits().to_le_bytes().iter().fold(hash, |h, &b| {
                (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
            })
        })
}
