use dengjen_stt::{Recognizer, RecognizerConfig};
use std::path::PathBuf;

fn fixture(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(PathBuf::from)
}

fn read_f32_le(path: PathBuf) -> Vec<f32> {
    let bytes = std::fs::read(path).unwrap();
    let (chunks, _) = bytes.as_chunks::<4>();
    chunks.iter().map(|b| f32::from_le_bytes(*b)).collect()
}

// Self-skips without a model: DENGJEN_STT_MODEL_DIR holds the files named by MODEL_FILE and TOKENS_FILE,
// DENGJEN_STT_CLIP is raw little-endian f32 mono samples at 16 kHz.
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
    let text = recognizer.recognize(&read_f32_le(clip), 16_000).unwrap();
    assert!(!text.trim().is_empty());
}
