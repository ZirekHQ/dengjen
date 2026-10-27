use dengjen_stt::{Recognizer, RecognizerConfig, SttError, MODEL_FILE, TOKENS_FILE};
use std::path::PathBuf;

const CLIP_SAMPLE_RATE: i32 = 24_000;
// The 300M model spells Kurmanji phonetically, so only these stable words of the clip's sentence are asserted.
const CLIP_WORDS: [&str; 3] = ["şêr", "komên mezin", "dijîn"];

fn fixture(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(PathBuf::from)
}

fn read_f32_le(path: PathBuf) -> Vec<f32> {
    let bytes = std::fs::read(path).unwrap();
    let (chunks, _) = bytes.as_chunks::<4>();
    chunks.iter().map(|b| f32::from_le_bytes(*b)).collect()
}

#[test]
fn a_corrupt_model_file_is_a_load_error() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(MODEL_FILE), b"not an onnx model").unwrap();
    std::fs::write(dir.path().join(TOKENS_FILE), b"<blk> 0\n").unwrap();
    let result = Recognizer::new(dir.path(), RecognizerConfig::default());
    assert!(matches!(result, Err(SttError::ModelLoad(_))));
}

// Self-skips without a model: DENGJEN_STT_MODEL_DIR holds the files named by MODEL_FILE and TOKENS_FILE,
// DENGJEN_STT_CLIP is raw little-endian f32 mono samples at 24 kHz.
#[test]
fn transcribes_a_kurmanji_clip() {
    let (Some(model_dir), Some(clip)) = (
        fixture("DENGJEN_STT_MODEL_DIR"),
        fixture("DENGJEN_STT_CLIP"),
    ) else {
        eprintln!("skipping: DENGJEN_STT_MODEL_DIR and DENGJEN_STT_CLIP not set");
        return;
    };
    let recognizer = Recognizer::new(&model_dir, RecognizerConfig::default()).unwrap();
    let text = recognizer
        .recognize(&read_f32_le(clip), CLIP_SAMPLE_RATE)
        .unwrap()
        .to_lowercase();
    for word in CLIP_WORDS {
        assert!(text.contains(word), "{word:?} missing from {text:?}");
    }
}
