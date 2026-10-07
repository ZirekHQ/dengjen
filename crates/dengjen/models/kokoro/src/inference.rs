use crate::config::KokoroVoiceConfig;
#[cfg(feature = "japanese")]
use crate::ja::JapaneseG2p;
use crate::phonemize::{espeak_language_for_voice, text_to_kokoro_phonemes};
use crate::vocab::Vocab;
use crate::voice_style::VoiceStyles;
use dengjen_tts_core::{
    lock_ignoring_poison, read_ignoring_poison, write_ignoring_poison, Audio, AudioInfo,
    AudioSamples, AudioStreamIterator, CancellationToken, DengjenAudioResult, DengjenError,
    DengjenModel, DengjenResult, Phonemes, SynthesisConfig,
};
use ndarray::{Array1, Array2};
use ort::session::Session;
use ort::value::Tensor;
use std::collections::HashMap;
use std::sync::{Mutex, RwLock};

const KOKORO_CHUNK_SIZE_SCALE: usize = 256;

fn resolve_voice_name<'a>(
    speaker_map: &'a HashMap<i64, String>,
    default_voice: &'a str,
    synth_config: &SynthesisConfig,
) -> &'a str {
    synth_config
        .speaker
        .and_then(|id| speaker_map.get(&id))
        .map(String::as_str)
        .unwrap_or(default_voice)
}

pub struct KokoroModel {
    session: Mutex<Session>,
    vocab: Vocab,
    voice_styles: VoiceStyles,
    sample_rate: u32,
    default_voice: String,
    speaker_map: HashMap<i64, String>,
    synth_config: RwLock<SynthesisConfig>,
    #[cfg(feature = "japanese")]
    japanese: Option<JapaneseG2p>,
}

#[allow(clippy::vec_init_then_push)]
fn execution_providers() -> Vec<ort::ep::ExecutionProviderDispatch> {
    #[allow(unused_mut)]
    let mut providers = Vec::new();
    #[cfg(feature = "cuda")]
    providers.push(ort::ep::CUDA::default().build());
    #[cfg(feature = "directml")]
    providers.push(ort::ep::DirectML::default().build());
    #[cfg(feature = "coreml")]
    providers.push(ort::ep::CoreML::default().build());
    providers
}

impl KokoroModel {
    pub fn from_config(config: KokoroVoiceConfig) -> DengjenResult<Self> {
        let session = Session::builder()
            .map_err(|e| DengjenError::FailedToLoadResource(e.to_string()))?
            .with_execution_providers(execution_providers())
            .map_err(|e| DengjenError::FailedToLoadResource(e.to_string()))?
            .commit_from_file(&config.model_path)
            .map_err(|e| {
                DengjenError::FailedToLoadResource(format!(
                    "Failed to load Kokoro ONNX model at `{}`: {}",
                    config.model_path.display(),
                    e
                ))
            })?;
        let vocab = Vocab::load(&config.vocab_path)?;
        let voice_styles = VoiceStyles::load(&config.voices_dir, &config.voices)?;
        let default_voice =
            config.voices.first().cloned().ok_or_else(|| {
                DengjenError::FailedToLoadResource("No voices in config".to_string())
            })?;
        let speaker_map = config
            .voices
            .iter()
            .enumerate()
            .map(|(index, name)| (index as i64, name.clone()))
            .collect();
        #[cfg(feature = "japanese")]
        let japanese = config
            .voices
            .iter()
            .any(|voice| voice.starts_with('j'))
            .then(JapaneseG2p::from_env)
            .transpose()?;
        Ok(Self {
            session: Mutex::new(session),
            vocab,
            voice_styles,
            sample_rate: config.sample_rate,
            default_voice,
            speaker_map,
            synth_config: RwLock::new(SynthesisConfig::default()),
            #[cfg(feature = "japanese")]
            japanese,
        })
    }

    fn synthesize_phonemes(&self, phonemes: &str) -> DengjenAudioResult {
        let mut token_ids = vec![self.vocab.bos_id()];
        token_ids.extend(self.vocab.tokenize(phonemes));
        token_ids.push(self.vocab.eos_id());

        let input_ids = Array2::from_shape_vec((1, token_ids.len()), token_ids.clone())
            .map_err(|e| DengjenError::with_message(e.to_string()))?;
        let voice_name = {
            let synth_config = read_ignoring_poison(&self.synth_config);
            resolve_voice_name(&self.speaker_map, &self.default_voice, &synth_config).to_string()
        };
        let style = self.voice_styles.style_for(&voice_name, token_ids.len())?;
        let speed = Array1::from_vec(vec![1.0f32]);

        let mut session = lock_ignoring_poison(&self.session);
        let outputs = session
            .run(ort::inputs![
                Tensor::from_array(input_ids)
                    .map_err(|e| DengjenError::with_message(e.to_string()))?,
                Tensor::from_array(style).map_err(|e| DengjenError::with_message(e.to_string()))?,
                Tensor::from_array(speed).map_err(|e| DengjenError::with_message(e.to_string()))?,
            ])
            .map_err(|e| DengjenError::InferenceError(format!("Kokoro inference failed: {}", e)))?;
        let (_, data) = outputs[0].try_extract_tensor::<f32>().map_err(|e| {
            DengjenError::InferenceError(format!("Failed to extract Kokoro output: {}", e))
        })?;
        Ok(Audio::new(
            data.to_vec().into(),
            self.sample_rate as usize,
            None,
        ))
    }
}

impl DengjenModel for KokoroModel {
    fn audio_output_info(&self) -> DengjenResult<AudioInfo> {
        Ok(AudioInfo {
            sample_rate: self.sample_rate as usize,
            num_channels: 1,
            sample_width: 2,
        })
    }

    #[tracing::instrument(skip(self, text), fields(text_len = text.len()), err)]
    fn phonemize_text(&self, text: &str) -> DengjenResult<Phonemes> {
        let language = {
            let synth_config = read_ignoring_poison(&self.synth_config);
            let voice = resolve_voice_name(&self.speaker_map, &self.default_voice, &synth_config);
            espeak_language_for_voice(voice)?
        };
        #[cfg(feature = "japanese")]
        if let (Some(japanese), "ja") = (&self.japanese, language) {
            return Ok(Phonemes::from(japanese.phonemize(text)?));
        }
        let sentences = text_to_kokoro_phonemes(text, language)?;
        Ok(Phonemes::from(sentences))
    }

    #[tracing::instrument(skip(self, phoneme_batches), fields(batch_len = phoneme_batches.len()), err)]
    fn speak_batch(&self, phoneme_batches: Vec<String>) -> DengjenResult<Vec<Audio>> {
        phoneme_batches
            .into_iter()
            .map(|p| self.synthesize_phonemes(&p))
            .collect()
    }

    #[tracing::instrument(skip(self, phonemes), fields(phonemes_len = phonemes.len()), err)]
    fn speak_one_sentence(&self, phonemes: String) -> DengjenAudioResult {
        self.synthesize_phonemes(&phonemes)
    }

    fn get_default_synthesis_config(&self) -> DengjenResult<Option<SynthesisConfig>> {
        Ok(Some(SynthesisConfig {
            speaker: Some(0),
            parameters: HashMap::new(),
        }))
    }

    fn get_fallback_synthesis_config(&self) -> DengjenResult<Option<SynthesisConfig>> {
        Ok(Some(read_ignoring_poison(&self.synth_config).clone()))
    }

    fn set_fallback_synthesis_config(
        &self,
        synthesis_config: &SynthesisConfig,
    ) -> DengjenResult<()> {
        if let Some(speaker) = synthesis_config.speaker {
            if !self.speaker_map.contains_key(&speaker) {
                return Err(DengjenError::InvalidConfiguration(format!(
                    "No speaker was found with the given id `{}`",
                    speaker
                )));
            }
        }
        *write_ignoring_poison(&self.synth_config) = synthesis_config.clone();
        Ok(())
    }

    fn get_speakers(&self) -> DengjenResult<Option<&HashMap<i64, String>>> {
        Ok(Some(&self.speaker_map))
    }

    fn supports_streaming_output(&self) -> bool {
        true
    }

    #[tracing::instrument(
        skip(self, phonemes, cancel_token),
        fields(phonemes_len = phonemes.len(), chunk_size),
        err
    )]
    fn stream_synthesis(
        &self,
        phonemes: String,
        chunk_size: usize,
        _chunk_padding: usize,
        cancel_token: CancellationToken,
    ) -> DengjenResult<AudioStreamIterator<'_>> {
        if chunk_size == 0 {
            return Err(DengjenError::UnsupportedOperation(
                "chunk_size must be greater than 0".to_string(),
            ));
        }
        // Skip inference entirely rather than erroring: a cancellation must stay silent.
        if cancel_token.is_cancelled() {
            return Ok(Box::new(std::iter::empty()));
        }
        let audio = self.synthesize_phonemes(&phonemes)?;
        let scaled_chunk_size = chunk_size.saturating_mul(KOKORO_CHUNK_SIZE_SCALE);
        Ok(Box::new(KokoroAudioStreamer::new(
            audio.samples,
            scaled_chunk_size,
            cancel_token,
        )))
    }
}

struct KokoroAudioStreamer {
    samples: AudioSamples,
    cursor: usize,
    chunk_size: usize,
    cancel_token: CancellationToken,
}

impl KokoroAudioStreamer {
    fn new(samples: AudioSamples, chunk_size: usize, cancel_token: CancellationToken) -> Self {
        Self {
            samples,
            cursor: 0,
            chunk_size,
            cancel_token,
        }
    }
}

impl Iterator for KokoroAudioStreamer {
    type Item = DengjenResult<AudioSamples>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.cancel_token.is_cancelled() {
            return None;
        }
        if self.chunk_size == 0 || self.cursor >= self.samples.len() {
            return None;
        }
        let end = (self.cursor + self.chunk_size).min(self.samples.len());
        let chunk = self.samples.as_slice()[self.cursor..end].to_vec();
        self.cursor = end;
        Some(Ok(AudioSamples::new(chunk)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(not(any(feature = "cuda", feature = "directml", feature = "coreml")))]
    fn execution_providers_is_empty_when_no_gpu_feature_is_enabled() {
        assert!(execution_providers().is_empty());
    }

    fn speaker_map() -> HashMap<i64, String> {
        HashMap::from([(0, "voice_a".to_string()), (1, "voice_b".to_string())])
    }

    #[test]
    fn resolve_voice_name_returns_default_voice_when_synth_config_has_no_speaker() {
        let config = SynthesisConfig::default();
        assert_eq!(
            resolve_voice_name(&speaker_map(), "voice_a", &config),
            "voice_a"
        );
    }

    #[test]
    fn resolve_voice_name_returns_the_named_voice_for_a_known_speaker_id() {
        let config = SynthesisConfig {
            speaker: Some(1),
            ..Default::default()
        };
        assert_eq!(
            resolve_voice_name(&speaker_map(), "voice_a", &config),
            "voice_b"
        );
    }

    #[test]
    fn resolve_voice_name_falls_back_to_default_voice_for_an_unknown_speaker_id() {
        let config = SynthesisConfig {
            speaker: Some(99),
            ..Default::default()
        };
        assert_eq!(
            resolve_voice_name(&speaker_map(), "voice_a", &config),
            "voice_a"
        );
    }

    #[test]
    fn yields_equal_size_chunks_when_length_is_an_exact_multiple() {
        let samples = AudioSamples::new(vec![0.0; 9]);
        let mut streamer = KokoroAudioStreamer::new(samples, 3, CancellationToken::new());
        assert_eq!(streamer.next().unwrap().unwrap().len(), 3);
        assert_eq!(streamer.next().unwrap().unwrap().len(), 3);
        assert_eq!(streamer.next().unwrap().unwrap().len(), 3);
        assert!(streamer.next().is_none());
    }

    #[test]
    fn yields_a_shorter_final_chunk_for_a_remainder() {
        let samples = AudioSamples::new(vec![0.0; 7]);
        let mut streamer = KokoroAudioStreamer::new(samples, 3, CancellationToken::new());
        assert_eq!(streamer.next().unwrap().unwrap().len(), 3);
        assert_eq!(streamer.next().unwrap().unwrap().len(), 3);
        assert_eq!(streamer.next().unwrap().unwrap().len(), 1);
        assert!(streamer.next().is_none());
    }

    #[test]
    fn chunk_values_preserve_sample_order() {
        let samples = AudioSamples::new(vec![1.0, 2.0, 3.0, 4.0, 5.0]);
        let mut streamer = KokoroAudioStreamer::new(samples, 2, CancellationToken::new());
        assert_eq!(streamer.next().unwrap().unwrap().as_slice(), &[1.0, 2.0]);
        assert_eq!(streamer.next().unwrap().unwrap().as_slice(), &[3.0, 4.0]);
        assert_eq!(streamer.next().unwrap().unwrap().as_slice(), &[5.0]);
        assert!(streamer.next().is_none());
    }

    #[test]
    fn stops_yielding_once_cancelled_mid_stream() {
        let samples = AudioSamples::new(vec![0.0; 9]);
        let cancel_token = CancellationToken::new();
        let mut streamer = KokoroAudioStreamer::new(samples, 3, cancel_token.clone());
        assert!(streamer.next().is_some());
        cancel_token.cancel();
        assert!(
            streamer.next().is_none(),
            "no further chunks should be yielded once cancelled"
        );
    }

    #[test]
    fn empty_input_yields_no_chunks() {
        let samples = AudioSamples::new(Vec::new());
        let mut streamer = KokoroAudioStreamer::new(samples, 3, CancellationToken::new());
        assert!(streamer.next().is_none());
    }

    #[test]
    fn chunk_size_zero_yields_no_chunks() {
        let samples = AudioSamples::new(vec![0.0; 9]);
        let mut streamer = KokoroAudioStreamer::new(samples, 0, CancellationToken::new());
        assert!(streamer.next().is_none());
        assert!(
            streamer.next().is_none(),
            "should remain exhausted after first call"
        );
    }
}
