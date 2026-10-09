fn main() {
    divan::main();
}

mod dev_utils;

#[divan::bench_group(sample_count = 20, sample_size = 10)]
mod speech_streams {
    use super::*;
    use dengjen_tts::DengjenResult;
    use dengjen_tts_core::CancellationToken;
    use divan::{black_box, Bencher};

    fn first_chunk_wave_len<T>(
        mut stream: impl Iterator<Item = DengjenResult<T>>,
        wave_len: impl FnOnce(&T) -> usize,
    ) -> usize {
        let chunk = stream
            .next()
            .expect("stream yields at least one chunk")
            .expect("first chunk synthesizes without error");
        wave_len(&chunk)
    }

    #[divan::bench(threads = 4)]
    fn bench_lazy_stream(bencher: Bencher) {
        bencher
            .with_inputs(|| {
                dev_utils::gen_params("std")
                    .expect("failed to load std voice fixture")
                    .expect("std voice fixture missing; see CONTRIBUTING.md#benchmarks")
            })
            .bench_local_refs(|(synth, text, output_config)| {
                let stream = synth
                    .synthesize_lazy(text.clone(), output_config.clone())
                    .unwrap()
                    .map(|result| result.map(|chunk| chunk.samples));
                dev_utils::iterate_stream(black_box(stream)).unwrap();
            });
    }

    #[divan::bench]
    fn bench_parallel_stream(bencher: Bencher) {
        bencher
            .with_inputs(|| {
                dev_utils::gen_params("std")
                    .expect("failed to load std voice fixture")
                    .expect("std voice fixture missing; see CONTRIBUTING.md#benchmarks")
            })
            .bench_local_refs(|(synth, text, output_config)| {
                let stream = synth
                    .synthesize_parallel(text.clone(), output_config.clone())
                    .unwrap()
                    .map(|result| result.map(|chunk| chunk.samples));
                dev_utils::iterate_stream(black_box(stream)).unwrap();
            });
    }

    #[divan::bench]
    fn bench_realtime_stream(bencher: Bencher) {
        bencher
            .with_inputs(|| {
                dev_utils::gen_params("rt")
                    .expect("failed to load rt voice fixture")
                    .expect("rt voice fixture missing; see CONTRIBUTING.md#benchmarks")
            })
            .bench_local_refs(|(synth, text, output_config)| {
                let stream = synth
                    .synthesize_streamed(
                        text.clone(),
                        output_config.clone(),
                        72,
                        3,
                        CancellationToken::new(),
                    )
                    .unwrap();
                dev_utils::iterate_stream(black_box(stream)).unwrap();
            });
    }

    #[divan::bench]
    fn bench_lazy_stream_latency(bencher: Bencher) {
        bencher
            .with_inputs(|| {
                dev_utils::gen_params("std")
                    .expect("failed to load std voice fixture")
                    .expect("std voice fixture missing; see CONTRIBUTING.md#benchmarks")
            })
            .bench_local_refs(|(synth, text, output_config)| {
                let stream = synth
                    .synthesize_lazy(text.clone(), output_config.clone())
                    .unwrap();
                let _ =
                    first_chunk_wave_len(black_box(stream), |audio| audio.as_wave_bytes().len());
            });
    }

    #[divan::bench]
    fn bench_parallel_stream_latency(bencher: Bencher) {
        bencher
            .with_inputs(|| {
                dev_utils::gen_params("std")
                    .expect("failed to load std voice fixture")
                    .expect("std voice fixture missing; see CONTRIBUTING.md#benchmarks")
            })
            .bench_local_refs(|(synth, text, output_config)| {
                let stream = synth
                    .synthesize_parallel(text.clone(), output_config.clone())
                    .unwrap();
                let _ =
                    first_chunk_wave_len(black_box(stream), |audio| audio.as_wave_bytes().len());
            });
    }

    #[divan::bench]
    fn bench_realtime_stream_latency(bencher: Bencher) {
        bencher
            .with_inputs(|| {
                dev_utils::gen_params("rt")
                    .expect("failed to load rt voice fixture")
                    .expect("rt voice fixture missing; see CONTRIBUTING.md#benchmarks")
            })
            .bench_local_refs(|(synth, text, output_config)| {
                let stream = synth
                    .synthesize_streamed(
                        text.clone(),
                        output_config.clone(),
                        72,
                        3,
                        CancellationToken::new(),
                    )
                    .unwrap();
                let _ = first_chunk_wave_len(black_box(stream), |samples| {
                    samples.as_wave_bytes().len()
                });
            });
    }

    #[divan::bench]
    fn bench_kokoro_lazy_stream(bencher: Bencher) {
        bencher
            .with_inputs(|| {
                dev_utils::gen_params("kokoro")
                    .expect("failed to load kokoro voice fixture")
                    .expect("kokoro voice fixture missing; see CONTRIBUTING.md#benchmarks")
            })
            .bench_local_refs(|(synth, text, output_config)| {
                let stream = synth
                    .synthesize_lazy(text.clone(), output_config.clone())
                    .unwrap()
                    .map(|result| result.map(|chunk| chunk.samples));
                dev_utils::iterate_stream(black_box(stream)).unwrap();
            });
    }

    #[divan::bench]
    fn bench_kokoro_lazy_stream_latency(bencher: Bencher) {
        bencher
            .with_inputs(|| {
                dev_utils::gen_params("kokoro")
                    .expect("failed to load kokoro voice fixture")
                    .expect("kokoro voice fixture missing; see CONTRIBUTING.md#benchmarks")
            })
            .bench_local_refs(|(synth, text, output_config)| {
                let stream = synth
                    .synthesize_lazy(text.clone(), output_config.clone())
                    .unwrap();
                let _ =
                    first_chunk_wave_len(black_box(stream), |audio| audio.as_wave_bytes().len());
            });
    }
}
