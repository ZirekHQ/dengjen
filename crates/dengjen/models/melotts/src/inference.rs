use crate::config::{map_phone_tone_pairs_to_ids, MeloVoiceConfig};
use dengjen_tts_core::{
    lock_ignoring_poison, Audio, DengjenAudioResult, DengjenError, DengjenResult,
};
use ndarray::{Array1, Array2};
use ort::session::Session;
use ort::value::Tensor;
use std::path::Path;
use std::sync::Mutex;

pub(crate) struct MeloTTSModel {
    session: Mutex<Session>,
    config: MeloVoiceConfig,
}

impl MeloTTSModel {
    pub(crate) fn from_config(config: MeloVoiceConfig) -> DengjenResult<Self> {
        let model_path = config.model_path.clone();
        Self::from_config_with_model_path(config, &model_path)
    }

    fn from_config_with_model_path(
        config: MeloVoiceConfig,
        model_path: &Path,
    ) -> DengjenResult<Self> {
        let session = dengjen_tts_core::session_builder()
            .map_err(|e| DengjenError::FailedToLoadResource(e.to_string()))?
            .commit_from_file(model_path)
            .map_err(|e| {
                DengjenError::FailedToLoadResource(format!(
                    "Failed to load MeloTTS ONNX model at `{}`: {e}",
                    model_path.display()
                ))
            })?;
        Ok(Self {
            session: Mutex::new(session),
            config,
        })
    }

    pub(crate) fn synthesize_phone_tone_pairs(
        &self,
        pairs: &[(String, String)],
        synth: &crate::synth_config::MeloSynthesisConfig,
    ) -> DengjenAudioResult {
        let (phone_ids, tone_ids) =
            map_phone_tone_pairs_to_ids(&self.config.phone_id_map, &self.config.tone_id_map, pairs);
        let seq_len = phone_ids.len();

        let x = Array2::<i64>::from_shape_vec((1, seq_len), phone_ids)
            .map_err(|e| DengjenError::with_message(e.to_string()))?;
        let x_lengths = Array1::<i64>::from_iter([seq_len as i64]);
        let tones = Array2::<i64>::from_shape_vec((1, seq_len), tone_ids)
            .map_err(|e| DengjenError::with_message(e.to_string()))?;
        let sid = Array1::<i64>::from_iter([synth.speaker.unwrap_or(0)]);
        let noise_scale = Array1::<f32>::from_iter([synth.noise_scale]);
        let length_scale = Array1::<f32>::from_iter([synth.length_scale]);
        let noise_scale_w = Array1::<f32>::from_iter([synth.noise_scale_w]);

        let mut session = lock_ignoring_poison(&self.session);
        let outputs = session
            .run(ort::inputs![
                "x" => Tensor::from_array(x).map_err(|e| DengjenError::with_message(e.to_string()))?,
                "x_lengths" => Tensor::from_array(x_lengths).map_err(|e| DengjenError::with_message(e.to_string()))?,
                "tones" => Tensor::from_array(tones).map_err(|e| DengjenError::with_message(e.to_string()))?,
                "sid" => Tensor::from_array(sid).map_err(|e| DengjenError::with_message(e.to_string()))?,
                "noise_scale" => Tensor::from_array(noise_scale).map_err(|e| DengjenError::with_message(e.to_string()))?,
                "length_scale" => Tensor::from_array(length_scale).map_err(|e| DengjenError::with_message(e.to_string()))?,
                "noise_scale_w" => Tensor::from_array(noise_scale_w).map_err(|e| DengjenError::with_message(e.to_string()))?,
            ])
            .map_err(|e| DengjenError::InferenceError(format!("MeloTTS inference failed: {e}")))?;

        let (_, data) = outputs
            .get("y")
            .ok_or_else(|| {
                DengjenError::InferenceError("MeloTTS model has no output named `y`".to_string())
            })?
            .try_extract_tensor::<f32>()
            .map_err(|e| {
                DengjenError::InferenceError(format!("Failed to extract MeloTTS output: {e}"))
            })?;
        Ok(Audio::new(
            data.to_vec().into(),
            self.config.audio.sample_rate as usize,
            None,
        ))
    }
}
