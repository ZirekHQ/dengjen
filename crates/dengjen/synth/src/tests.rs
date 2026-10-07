mod dev_utils;

use dengjen_tts::DengjenResult;
use std::{path::PathBuf, sync::Arc};
use tracing_test::traced_test;

fn parse_chunk_index(line: &str) -> Option<usize> {
    let after = line.split("chunk_index=").nth(1)?;
    after
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .ok()
}

/// Checks that every expected chunk index has a captured log line proving its
/// `chunk_ready` fired at `DEBUG`, nested under a `synthesis_request` span
/// carrying the given `mode`, followed by a `chunk` span — not just that both
/// names appear somewhere in the logs.
fn assert_chunk_ready_nested_under_mode(
    lines: &[&str],
    mode: &str,
    expected_chunk_count: usize,
) -> Result<(), String> {
    let span_context = format!(":synthesis_request{{mode=\"{mode}\"}}:chunk");
    let indices: std::collections::BTreeSet<usize> = lines
        .iter()
        .filter(|line| {
            line.contains("DEBUG") && line.contains(&span_context) && line.contains("chunk_ready")
        })
        .filter_map(|line| parse_chunk_index(line))
        .collect();
    let expected: std::collections::BTreeSet<usize> = (0..expected_chunk_count).collect();
    if indices == expected {
        Ok(())
    } else {
        Err(format!(
            "expected DEBUG chunk_ready events nested under `{span_context}` carrying chunk_index \
             values {expected:?}, got {indices:?}; log lines: {lines:?}"
        ))
    }
}

#[traced_test]
#[test]
fn lazy_stream_emits_nested_synthesis_request_and_chunk_spans() {
    let (model, _fixture_dir) = build_synthetic_kokoro_model();
    let model: Arc<dyn dengjen_tts_core::DengjenModel + Send + Sync> = Arc::new(model);
    let synthesizer = dengjen_tts::DengjenSpeechSynthesizer::new(model).unwrap();

    let stream = synthesizer.synthesize_lazy("t\u{025b}st".to_string(), None);
    let stream = match stream {
        Ok(stream) => stream,
        Err(dengjen_tts_core::DengjenError::PhonemizationError(msg))
            if msg.contains(dengjen_espeak_phonemizer::ESPEAKNG_INIT_FAILURE_MARKER) =>
        {
            eprintln!(
                "skipping lazy_stream_emits_nested_synthesis_request_and_chunk_spans: espeak-ng data unavailable"
            );
            return;
        }
        Err(e) => panic!("synthesize_lazy failed unexpectedly: {e:?}"),
    };

    let chunks: Vec<_> = stream.collect();

    logs_assert(|lines| assert_chunk_ready_nested_under_mode(lines, "lazy", chunks.len()));
}

#[traced_test]
#[test]
fn parallel_stream_emits_nested_synthesis_request_and_chunk_spans() {
    let (model, _fixture_dir) = build_synthetic_kokoro_model();
    let model: Arc<dyn dengjen_tts_core::DengjenModel + Send + Sync> = Arc::new(model);
    let synthesizer = dengjen_tts::DengjenSpeechSynthesizer::new(model).unwrap();

    let stream = synthesizer.synthesize_parallel("t\u{025b}st".to_string(), None);
    let stream = match stream {
        Ok(stream) => stream,
        Err(dengjen_tts_core::DengjenError::PhonemizationError(msg))
            if msg.contains(dengjen_espeak_phonemizer::ESPEAKNG_INIT_FAILURE_MARKER) =>
        {
            eprintln!(
                "skipping parallel_stream_emits_nested_synthesis_request_and_chunk_spans: espeak-ng data unavailable"
            );
            return;
        }
        Err(e) => panic!("synthesize_parallel failed unexpectedly: {e:?}"),
    };

    let chunks: Vec<_> = stream.collect();

    logs_assert(|lines| assert_chunk_ready_nested_under_mode(lines, "parallel", chunks.len()));
}

#[traced_test]
#[test]
fn realtime_stream_emits_nested_synthesis_request_and_chunk_spans() {
    let (model, _fixture_dir) = build_synthetic_kokoro_model();
    let model: Arc<dyn dengjen_tts_core::DengjenModel + Send + Sync> = Arc::new(model);
    let synthesizer = dengjen_tts::DengjenSpeechSynthesizer::new(model).unwrap();

    let stream = synthesizer.synthesize_streamed(
        "t\u{025b}st".to_string(),
        None,
        72,
        3,
        dengjen_tts_core::CancellationToken::new(),
    );
    let stream = match stream {
        Ok(stream) => stream,
        Err(dengjen_tts_core::DengjenError::PhonemizationError(msg))
            if msg.contains(dengjen_espeak_phonemizer::ESPEAKNG_INIT_FAILURE_MARKER) =>
        {
            eprintln!(
                "skipping realtime_stream_emits_nested_synthesis_request_and_chunk_spans: espeak-ng data unavailable"
            );
            return;
        }
        Err(e) => panic!("synthesize_streamed failed unexpectedly: {e:?}"),
    };

    let chunks: Vec<_> = stream.map(|c| c.expect("chunk synthesis failed")).collect();

    logs_assert(|lines| assert_chunk_ready_nested_under_mode(lines, "realtime", chunks.len()));
}

#[test]
fn test_lazy_stream() -> DengjenResult<()> {
    let Some((synthesizer, text, config)) = dev_utils::gen_params("std")? else {
        eprintln!(
            "skipping test_lazy_stream: no std voice fixture at crates/dengjen/synth/models/std/ (see #220)"
        );
        return Ok(());
    };
    let stream = synthesizer
        .synthesize_lazy(text, config)?
        .map(|chunk| chunk.map(|c| c.samples));
    dev_utils::iterate_stream(stream)
}

#[test]
fn test_parallel_stream() -> DengjenResult<()> {
    let Some((synthesizer, text, config)) = dev_utils::gen_params("std")? else {
        eprintln!(
            "skipping test_parallel_stream: no std voice fixture at crates/dengjen/synth/models/std/ (see #220)"
        );
        return Ok(());
    };
    let stream = synthesizer
        .synthesize_parallel(text, config)?
        .map(|chunk| chunk.map(|c| c.samples));
    dev_utils::iterate_stream(stream)
}

#[test]
fn test_realtime_stream() -> DengjenResult<()> {
    let Some((synthesizer, text, config)) = dev_utils::gen_params("rt")? else {
        eprintln!(
            "skipping test_realtime_stream: no rt voice fixture at crates/dengjen/synth/models/rt/ (see #220)"
        );
        return Ok(());
    };
    let cancel = dengjen_tts_core::CancellationToken::new();
    let stream = synthesizer.synthesize_streamed(text, config, 72, 3, cancel)?;
    dev_utils::iterate_stream(stream)
}

const KOKORO_VOICE_STYLE_ROWS: usize = 510;
const KOKORO_VOICE_STYLE_COLS: usize = 256;

fn synthetic_voice_style_bytes() -> Vec<u8> {
    let mut bytes = Vec::with_capacity(KOKORO_VOICE_STYLE_ROWS * KOKORO_VOICE_STYLE_COLS * 4);
    for row in 0..KOKORO_VOICE_STYLE_ROWS {
        let value = (row as f32).to_le_bytes();
        for _ in 0..KOKORO_VOICE_STYLE_COLS {
            bytes.extend_from_slice(&value);
        }
    }
    bytes
}

fn write_synthetic_voice(voices_dir: &std::path::Path, name: &str) {
    std::fs::write(
        voices_dir.join(format!("{name}.bin")),
        synthetic_voice_style_bytes(),
    )
    .expect("failed to write synthetic voice fixture");
}

fn write_minimal_vocab(dir: &std::path::Path) -> PathBuf {
    let path = dir.join("tokenizer.json");
    let vocab = r#"{"model": {"vocab": {"$": 0, "t": 1, "ɛ": 2, "s": 3}}}"#;
    std::fs::write(&path, vocab).expect("failed to write synthetic vocab fixture");
    path
}

fn build_synthetic_kokoro_model() -> (dengjen_tts_kokoro::KokoroModel, tempfile::TempDir) {
    let root = tempfile::tempdir().expect("failed to create fixture temp dir");
    let voices_dir = root.path().join("voices");
    std::fs::create_dir_all(&voices_dir).expect("failed to create fixture voices dir");
    write_synthetic_voice(&voices_dir, "af_test");
    let vocab_path = write_minimal_vocab(root.path());

    let model_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../models/kokoro/tests/fixtures/synthetic_kokoro.onnx");

    let config = dengjen_tts_kokoro::KokoroVoiceConfig {
        model_path,
        voices_dir,
        vocab_path,
        sample_rate: 24000,
        voices: vec!["af_test".to_string()],
    };
    let model = dengjen_tts_kokoro::KokoroModel::from_config(config)
        .expect("failed to build synthetic Kokoro model");
    (model, root)
}

#[test]
fn kokoro_realtime_stream_uses_realistic_chunk_duration_for_capi_default_chunk_size() {
    let (model, _fixture_dir) = build_synthetic_kokoro_model();
    let model: Arc<dyn dengjen_tts_core::DengjenModel + Send + Sync> = Arc::new(model);
    let synthesizer = dengjen_tts::DengjenSpeechSynthesizer::new(model).unwrap();

    let stream = synthesizer.synthesize_streamed(
        "t\u{025b}st".to_string(),
        None,
        72,
        3,
        dengjen_tts_core::CancellationToken::new(),
    );
    let stream = match stream {
        Ok(stream) => stream,
        Err(dengjen_tts_core::DengjenError::PhonemizationError(msg))
            if msg.contains(dengjen_espeak_phonemizer::ESPEAKNG_INIT_FAILURE_MARKER) =>
        {
            eprintln!(
                "skipping kokoro_realtime_stream_uses_realistic_chunk_duration_for_capi_default_chunk_size: espeak-ng data unavailable on this machine"
            );
            return;
        }
        Err(e) => panic!("synthesize_streamed failed unexpectedly: {e:?}"),
    };

    let chunks: Vec<Vec<f32>> = stream
        .map(|chunk| chunk.expect("chunk synthesis failed").into_vec())
        .collect();

    assert_eq!(
        chunks.len(),
        1,
        "expected the whole sentence to land in a single chunk"
    );
    assert_eq!(chunks[0].len(), 16000);
}
