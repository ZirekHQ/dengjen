use dengjen_audio_ops::AudioSamples;
use divan::Bencher;

const SAMPLE_COUNT: usize = 44_100 * 10;

fn overlap_pair_source() -> impl Fn() -> (AudioSamples, AudioSamples) {
    let template: Vec<f32> = (0..SAMPLE_COUNT).map(|idx| idx as f32).collect();
    move || (template.clone().into(), template.clone().into())
}

#[divan::bench]
fn bench_overlap_with(divan_bencher: Bencher) {
    divan_bencher
        .with_inputs(overlap_pair_source())
        .bench_refs(|(left, right)| left.overlap_with(right, SAMPLE_COUNT / 2));
}

fn speech_like(frames: usize) -> Vec<f32> {
    let tau = 2.0 * std::f32::consts::PI;
    (0..frames)
        .map(|n| {
            let t = n as f32 / 22_050.0;
            0.4 * (tau * 140.0 * t).sin() + 0.2 * (tau * 280.0 * t).sin()
        })
        .collect()
}

fn run_sonic(speed: f32, pitch: f32, input: &[f32]) -> usize {
    let mut stream =
        dengjen_audio_ops::SonicStream::new(22_050, 1).expect("valid sonic dimensions");
    stream.set_speed(speed);
    stream.set_pitch(pitch);
    stream.write_f32(input);
    stream.flush();
    let available = stream.available_frames();
    stream.read_f32(available).len()
}

#[divan::bench]
fn bench_sonic_speed_1_5(divan_bencher: Bencher) {
    let input = speech_like(22_050 * 10);
    divan_bencher.bench_local(|| run_sonic(1.5, 1.0, &input));
}

#[divan::bench]
fn bench_sonic_speed_and_pitch(divan_bencher: Bencher) {
    let input = speech_like(22_050 * 10);
    divan_bencher.bench_local(|| run_sonic(1.5, 1.25, &input));
}

fn main() {
    divan::main();
}
