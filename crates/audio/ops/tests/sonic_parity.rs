mod common;

use common::{first_mismatch, matrix, run_c, run_rust, signal};

fn assert_parity(speeds: &[f32], pitches: &[f32], volumes: &[f32]) {
    for case in matrix(speeds, pitches, volumes) {
        let input = signal(
            case.sample_rate,
            case.channels,
            case.sample_rate as usize / 2,
        );
        let expected = run_c(&case, &input);
        let actual = run_rust(&case, &input);
        assert_eq!(
            first_mismatch(&expected, &actual),
            None,
            "case {} (c={} samples, rust={} samples)",
            case.id(),
            expected.len(),
            actual.len()
        );
    }
}

#[test]
fn identity_matches_c() {
    assert_parity(&[1.0], &[1.0], &[1.0]);
}

#[test]
fn speed_changes_match_c() {
    assert_parity(&[0.5, 0.75, 1.5, 2.0, 3.0, 5.5], &[1.0], &[1.0]);
}

#[test]
fn pitch_changes_match_c() {
    assert_parity(&[1.0], &[0.5, 0.75, 1.25, 1.5, 2.0], &[1.0]);
}

#[test]
fn volume_changes_match_c() {
    assert_parity(&[1.0], &[1.0], &[0.01, 0.25, 0.5, 2.0]);
}

#[test]
fn combined_params_match_c() {
    assert_parity(&[0.5, 1.5, 3.0, 5.5], &[0.5, 1.0, 1.5], &[0.5, 1.0]);
}

#[test]
fn edge_inputs_match_c() {
    for (id, case, input) in common::edge_cases() {
        assert_eq!(
            first_mismatch(&run_c(&case, &input), &run_rust(&case, &input)),
            None,
            "{id}"
        );
    }
}

use dengjen_audio_ops::SonicStream;
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn arbitrary_input_and_params_never_panic(
        rate_idx in 0usize..4,
        channels in 1u32..=3,
        speed in 0.05f32..20.0,
        pitch in 0.05f32..20.0,
        volume in 0.0f32..120.0,
        samples in proptest::collection::vec(-4.0f32..4.0, 0..6000),
        chunk in 1usize..2500,
    ) {
        let rate = [8_000u32, 16_000, 22_050, 48_000][rate_idx];
        let mut stream = SonicStream::new(rate, channels).unwrap();
        stream.set_speed(speed);
        stream.set_pitch(pitch);
        stream.set_volume(volume);
        for part in samples.chunks(chunk) {
            stream.write_f32(part);
        }
        stream.flush();
        let available = stream.available_frames();
        let out = stream.read_f32(available);
        prop_assert_eq!(out.len(), available * channels as usize);
        prop_assert!(out.iter().all(|s| s.is_finite() && s.abs() <= 1.0001));
    }
}
