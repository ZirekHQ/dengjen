mod common;

use common::{matrix, run_rust, signal};
use dengjen_audio_ops::SonicStream;
use proptest::prelude::*;

const SPEEDS: [f32; 7] = [0.5, 0.75, 1.0, 1.5, 2.0, 3.0, 5.5];
const PITCHES: [f32; 4] = [0.5, 1.0, 1.25, 1.5];
const VOLUMES: [f32; 3] = [0.25, 0.5, 1.0];

fn golden_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/sonic_golden.tsv")
}

#[test]
fn golden_matches_rust() {
    let golden = std::fs::read_to_string(golden_path()).expect("goldens present");
    let expected: std::collections::HashMap<&str, (usize, &str)> = golden
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            Some((parts.next()?, (parts.next()?.parse().ok()?, parts.next()?)))
        })
        .collect();
    for case in matrix(&SPEEDS, &PITCHES, &VOLUMES) {
        let input = signal(
            case.sample_rate,
            case.channels,
            case.sample_rate as usize / 2,
        );
        let out = run_rust(&case, &input);
        let (len, hash) = expected[case.id().as_str()];
        assert_eq!(out.len(), len, "length for {}", case.id());
        assert_eq!(
            format!("{:016x}", common::fnv1a(&out)),
            hash,
            "hash for {}",
            case.id()
        );
    }
    for (id, case, input) in common::edge_cases() {
        let out = run_rust(&case, &input);
        let (len, hash) = expected[id.as_str()];
        assert_eq!(out.len(), len, "length for {id}");
        assert_eq!(
            format!("{:016x}", common::fnv1a(&out)),
            hash,
            "hash for {id}"
        );
    }
}

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
