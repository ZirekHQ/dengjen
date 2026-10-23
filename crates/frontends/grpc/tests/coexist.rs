//! Checks that `ort` (through a Piper voice) and sherpa-onnx (through `dengjen_stt`) coexist in one process.
//!
//! Needs `DENGJEN_COEXIST_VOICE_CONFIG` (a Piper `model.onnx.json`) and `DENGJEN_STT_MODEL_DIR`.
//! Self-skips without them unless `DENGJEN_REQUIRE_COEXIST` is set, which the manual workflow does.
//! `DENGJEN_COEXIST_TEXT_PHONEMES` skips the transcript checks, for builds without espeak.
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
        Self::wrap(model)
    }

    // Builds without espeak (iOS) cannot phonemize, so the voice reads the text as phonemes and
    // produces unintelligible audio; ort still runs its full inference path.
    fn load_text_phonemes(
        config: &std::path::Path,
        scratch: &std::path::Path,
    ) -> DengjenResult<Self> {
        let original = std::fs::read_to_string(config).expect("read the voice config");
        let patched = original.replace(r#""phoneme_type": "espeak""#, r#""phoneme_type": "text""#);
        assert_ne!(
            patched, original,
            "voice config has no espeak phoneme_type to patch"
        );
        std::fs::copy(config.with_extension(""), scratch.join("voice.onnx"))
            .expect("copy the voice model");
        let patched_config = scratch.join("voice.onnx.json");
        std::fs::write(&patched_config, patched).expect("write the patched voice config");
        Self::load(&patched_config)
    }

    fn wrap(model: Arc<dyn DengjenModel + Send + Sync>) -> DengjenResult<Self> {
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

// Piper adds noise per run, so compare lengths with a tolerance, not equality.
fn similar_length(left: usize, right: usize) -> bool {
    left.abs_diff(right) * 5 <= left.max(right)
}

// The recognizer need not be exact; two distinctive words show it heard the phrase.
fn heard_phrase(transcript: &str) -> bool {
    let lowered = transcript.to_lowercase();
    let heard: Vec<&str> = lowered.split(|c: char| !c.is_alphabetic()).collect();
    ["quick", "brown", "fox", "jumps", "lazy", "dog"]
        .iter()
        .filter(|word| heard.contains(*word))
        .count()
        >= 2
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
    let text_phonemes = std::env::var_os("DENGJEN_COEXIST_TEXT_PHONEMES").is_some();
    let scratch = tempfile::tempdir().expect("create a scratch directory");
    let load_voice = || {
        eprintln!("loading the Piper voice through ort");
        if text_phonemes {
            Voice::load_text_phonemes(&inputs.voice_config, scratch.path())
        } else {
            Voice::load(&inputs.voice_config)
        }
        .expect("load the Piper voice through ort")
    };
    let heard = |transcript: &str| text_phonemes || heard_phrase(transcript);
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
    eprintln!("transcript: {transcript}");
    assert!(
        heard(&transcript),
        "sherpa-onnx did not hear the phrase: {transcript:?}"
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
        assert!(heard(&text), "concurrent transcript: {text:?}");
    });
}
