//! Proves `ort` (through a Piper voice) and sherpa-onnx (through `dengjen_stt`) run in one process.
//!
//! Needs `DENGJEN_COEXIST_VOICE_CONFIG` (a Piper `model.onnx.json`) and `DENGJEN_STT_MODEL_DIR`.
//! Self-skips without them unless `DENGJEN_REQUIRE_COEXIST=1`, which the manual workflow sets.
#![cfg(feature = "stt")]

use dengjen_stt::{Recognizer, RecognizerConfig};
use dengjen_tts::{DengjenModel, DengjenResult, DengjenSpeechSynthesizer};
use std::path::PathBuf;
use std::sync::Arc;

const PHRASE: &str = "The quick brown fox jumps over the lazy dog.";

struct Inputs {
    voice_config: PathBuf,
    stt_model_dir: PathBuf,
}

fn inputs() -> Option<Inputs> {
    let voice_config = std::env::var_os("DENGJEN_COEXIST_VOICE_CONFIG").map(PathBuf::from);
    let stt_model_dir = std::env::var_os("DENGJEN_STT_MODEL_DIR").map(PathBuf::from);
    match (voice_config, stt_model_dir) {
        (Some(voice_config), Some(stt_model_dir)) => Some(Inputs {
            voice_config,
            stt_model_dir,
        }),
        _ if std::env::var_os("DENGJEN_REQUIRE_COEXIST").is_some() => panic!(
            "DENGJEN_REQUIRE_COEXIST is set but DENGJEN_COEXIST_VOICE_CONFIG or DENGJEN_STT_MODEL_DIR is missing"
        ),
        _ => None,
    }
}

struct Voice {
    synthesizer: DengjenSpeechSynthesizer,
    sample_rate: i32,
}

impl Voice {
    fn load(config: &std::path::Path) -> DengjenResult<Self> {
        let model: Arc<dyn DengjenModel + Send + Sync> =
            dengjen_tts_piper::from_config_path(config)?;
        let sample_rate =
            i32::try_from(model.audio_output_info()?.sample_rate).expect("sample rate fits in i32");
        Ok(Self {
            synthesizer: DengjenSpeechSynthesizer::new(model)?,
            sample_rate,
        })
    }

    fn say(&self, text: &str) -> DengjenResult<Vec<f32>> {
        let mut samples = Vec::new();
        for chunk in self.synthesizer.synthesize_lazy(text.to_owned(), None)? {
            samples.extend_from_slice(chunk?.samples.as_slice());
        }
        Ok(samples)
    }
}

// Piper adds noise per run, so equal text yields lengths within a few percent, not identical.
fn similar_length(left: usize, right: usize) -> bool {
    left.abs_diff(right) * 5 <= left.max(right)
}

#[test]
fn ort_and_sherpa_onnx_share_a_process() {
    let Some(inputs) = inputs() else {
        eprintln!("skipping: DENGJEN_COEXIST_VOICE_CONFIG and DENGJEN_STT_MODEL_DIR not set");
        return;
    };

    let sherpa_first = std::env::var("DENGJEN_COEXIST_ORDER").is_ok_and(|o| o == "sherpa-first");
    let load_recognizer = || {
        eprintln!("loading the recognizer");
        Recognizer::new(&inputs.stt_model_dir, RecognizerConfig::default())
            .expect("load the recognizer")
    };
    let load_voice = || {
        eprintln!("loading the Piper voice through ort");
        Voice::load(&inputs.voice_config).expect("load the Piper voice through ort")
    };
    let (voice, recognizer) = if sherpa_first {
        let recognizer = load_recognizer();
        (load_voice(), recognizer)
    } else {
        let voice = load_voice();
        (voice, load_recognizer())
    };

    eprintln!("synthesizing");
    let spoken = voice.say(PHRASE).expect("synthesize");
    assert!(!spoken.is_empty(), "ort produced no audio");

    eprintln!("transcribing");
    let transcript = recognizer
        .recognize(&spoken, voice.sample_rate)
        .expect("transcribe the synthesized audio");
    assert!(
        !transcript.trim().is_empty(),
        "sherpa-onnx returned no text"
    );

    eprintln!("synthesizing again");
    let again = voice
        .say(PHRASE)
        .expect("ort still synthesizes after sherpa-onnx loaded");
    assert!(
        similar_length(again.len(), spoken.len()),
        "ort output changed from {} to {} samples after sherpa-onnx loaded",
        spoken.len(),
        again.len()
    );

    eprintln!("synthesizing and transcribing concurrently");
    std::thread::scope(|scope| {
        let synthesis = scope.spawn(|| voice.say(PHRASE));
        let recognition = scope.spawn(|| recognizer.recognize(&spoken, voice.sample_rate));
        let synthesized = synthesis
            .join()
            .expect("synthesis thread")
            .expect("concurrent synthesis");
        let text = recognition
            .join()
            .expect("recognition thread")
            .expect("concurrent recognition");
        assert!(similar_length(synthesized.len(), spoken.len()));
        assert!(!text.trim().is_empty());
    });
}
