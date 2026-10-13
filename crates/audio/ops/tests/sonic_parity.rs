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
