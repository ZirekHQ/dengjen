#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)
)]

mod config;
mod inference;
mod phonemize;
mod synth_config;

pub use config::{AudioConfig, InferenceConfig, MeloVoiceConfig, PhonemizerConfig};
pub use synth_config::MeloSynthesisConfig;

use dengjen_tts_core::{
    lock_ignoring_poison, Audio, AudioInfo, DengjenAudioResult, DengjenError, DengjenModel,
    DengjenResult, Phonemes, SynthesisConfig,
};
use inference::MeloTTSModel as InnerModel;
use phonemize::{create_backend, phone_tone_pairs, PhonemizerBackend};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

fn reversed_speaker_map(speaker_id_map: &HashMap<String, i64>) -> HashMap<i64, String> {
    speaker_id_map
        .iter()
        .map(|(name, id)| (*id, name.clone()))
        .collect()
}

fn ensure_known_speaker(
    speaker_map: &HashMap<i64, String>,
    config: &SynthesisConfig,
) -> DengjenResult<()> {
    match config.speaker {
        Some(speaker) if !speaker_map.contains_key(&speaker) => {
            Err(DengjenError::InvalidConfiguration(format!(
                "No speaker was found with the given id `{speaker}`"
            )))
        }
        _ => Ok(()),
    }
}

struct MeloTTSModel {
    inner: InnerModel,
    backend: PhonemizerBackend,
    config: MeloVoiceConfig,
    speaker_map: HashMap<i64, String>,
    fallback_config: Mutex<Option<SynthesisConfig>>,
}

impl DengjenModel for MeloTTSModel {
    fn audio_output_info(&self) -> DengjenResult<AudioInfo> {
        Ok(AudioInfo {
            sample_rate: self.config.audio.sample_rate as usize,
            num_channels: 1,
            sample_width: 2,
        })
    }

    #[tracing::instrument(skip(self, text), fields(text_len = text.len()), err)]
    fn phonemize_text(&self, text: &str) -> DengjenResult<Phonemes> {
        let sentences = phone_tone_pairs(&self.backend, text)?;
        Ok(Phonemes::from(
            sentences
                .into_iter()
                .map(|pairs| {
                    pairs
                        .into_iter()
                        .map(|(phone, tone)| format!("{phone}:{tone}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .collect::<Vec<String>>(),
        ))
    }

    // No `err`: `speak_one_sentence` is itself `#[instrument(err)]`, so it already
    // logs the failure — adding `err` here would emit a second ERROR event per error.
    #[tracing::instrument(skip(self, phoneme_batches), fields(batch_len = phoneme_batches.len()))]
    fn speak_batch(&self, phoneme_batches: Vec<String>) -> DengjenResult<Vec<Audio>> {
        phoneme_batches
            .into_iter()
            .map(|sentence| self.speak_one_sentence(sentence))
            .collect()
    }

    #[tracing::instrument(skip(self, phonemes), fields(phonemes_len = phonemes.len()), err)]
    fn speak_one_sentence(&self, phonemes: String) -> DengjenAudioResult {
        let pairs: Vec<(String, String)> = phonemes
            .split('\n')
            .filter(|s| !s.is_empty())
            .map(|token| {
                token
                    .rsplit_once(':')
                    .map(|(phone, tone)| (phone.to_string(), tone.to_string()))
                    .unwrap_or_else(|| (token.to_string(), "_".to_string()))
            })
            .collect();
        let fallback = lock_ignoring_poison(&self.fallback_config);
        let speaker = fallback
            .as_ref()
            .and_then(|c| c.speaker)
            .or(self.config.default_speaker_id);
        let synth = MeloSynthesisConfig {
            speaker,
            noise_scale: fallback
                .as_ref()
                .and_then(|c| c.parameters.get(synth_config::NOISE_SCALE).copied())
                .unwrap_or(self.config.inference.noise_scale),
            length_scale: fallback
                .as_ref()
                .and_then(|c| c.parameters.get(synth_config::LENGTH_SCALE).copied())
                .unwrap_or(self.config.inference.length_scale),
            noise_scale_w: fallback
                .as_ref()
                .and_then(|c| c.parameters.get(synth_config::NOISE_SCALE_W).copied())
                .unwrap_or(self.config.inference.noise_scale_w),
        };
        drop(fallback);
        self.inner.synthesize_phone_tone_pairs(&pairs, &synth)
    }

    fn get_default_synthesis_config(&self) -> DengjenResult<Option<SynthesisConfig>> {
        Ok(Some(
            (&MeloSynthesisConfig {
                speaker: self.config.default_speaker_id,
                noise_scale: self.config.inference.noise_scale,
                length_scale: self.config.inference.length_scale,
                noise_scale_w: self.config.inference.noise_scale_w,
            })
                .into(),
        ))
    }

    fn get_fallback_synthesis_config(&self) -> DengjenResult<Option<SynthesisConfig>> {
        Ok(lock_ignoring_poison(&self.fallback_config).clone())
    }

    fn set_fallback_synthesis_config(
        &self,
        synthesis_config: &SynthesisConfig,
    ) -> DengjenResult<()> {
        ensure_known_speaker(&self.speaker_map, synthesis_config)?;
        *lock_ignoring_poison(&self.fallback_config) = Some(synthesis_config.clone());
        Ok(())
    }

    fn get_speakers(&self) -> DengjenResult<Option<&HashMap<i64, String>>> {
        Ok(Some(&self.speaker_map))
    }
}

#[tracing::instrument(skip_all, fields(config_path = %config_path.display()), err)]
pub fn from_config_path(config_path: &Path) -> DengjenResult<Arc<dyn DengjenModel + Send + Sync>> {
    let config = config::load_config(config_path)?;
    let backend = create_backend(&config.phonemizer)?;
    let inner = InnerModel::from_config(MeloVoiceConfig {
        audio: config.audio.clone(),
        phonemizer: config.phonemizer.clone(),
        phone_id_map: config.phone_id_map.clone(),
        tone_id_map: config.tone_id_map.clone(),
        speaker_id_map: config.speaker_id_map.clone(),
        default_speaker_id: config.default_speaker_id,
        inference: config.inference.clone(),
        model_path: config.model_path.clone(),
    })?;
    let speaker_map = reversed_speaker_map(&config.speaker_id_map);
    Ok(Arc::new(MeloTTSModel {
        inner,
        backend,
        config,
        speaker_map,
        fallback_config: Mutex::new(None),
    }))
}

#[cfg(test)]
mod speaker_validation_tests {
    use super::*;

    fn config_with_speaker(speaker: Option<i64>) -> SynthesisConfig {
        SynthesisConfig {
            speaker,
            ..Default::default()
        }
    }

    #[test]
    fn ensure_known_speaker_accepts_a_known_or_unset_speaker() {
        let map = HashMap::from([(1, "EN".to_string())]);
        assert!(ensure_known_speaker(&map, &config_with_speaker(Some(1))).is_ok());
        assert!(ensure_known_speaker(&map, &config_with_speaker(None)).is_ok());
    }

    #[test]
    fn ensure_known_speaker_rejects_an_unknown_speaker() {
        let map = HashMap::from([(1, "EN".to_string())]);
        let result = ensure_known_speaker(&map, &config_with_speaker(Some(9)));
        assert!(matches!(result, Err(DengjenError::InvalidConfiguration(_))));
    }
}
