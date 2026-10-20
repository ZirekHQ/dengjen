use crate::config::{load_model_config, resolve_default_speaker_id, ModelConfig};
use crate::phonemize::{
    create_hebrew_engine, create_pinyin_engine, create_tashkeel_engine, HebrewEngine, PinyinEngine,
    TashkeelEngine,
};
use crate::synth_config::PiperSynthesisConfig;
use crate::VitsModelCommons;
use dengjen_tts_core::{
    lock_ignoring_poison, read_ignoring_poison, Audio, AudioInfo, DengjenAudioResult, DengjenError,
    DengjenModel, DengjenResult, Phonemes, SynthesisConfig,
};
use ndarray::{Array1, Array2};
use ort::session::{Session, SessionInputValue};
use ort::value::Tensor;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};

pub(crate) fn reversed_mapping<K, V>(input: &HashMap<K, V>) -> HashMap<V, K>
where
    K: ToOwned<Owned = K>,
    V: ToOwned<Owned = V> + std::hash::Hash + std::cmp::Eq,
{
    input
        .iter()
        .map(|(key, value)| (value.to_owned(), key.to_owned()))
        .collect()
}

pub(crate) fn create_inference_session(model_path: &Path) -> Result<Session, ort::Error> {
    dengjen_tts_core::session_builder()?.commit_from_file(model_path)
}

pub(crate) fn session_init_error(cause: ort::Error) -> DengjenError {
    DengjenError::InferenceError(format!(
        "Failed to initialize onnxruntime inference session: `{}`",
        cause
    ))
}

pub(crate) fn inference_error(cause: impl std::fmt::Display) -> DengjenError {
    DengjenError::InferenceError(format!("Failed to run model inference. Error: {}", cause))
}

pub(crate) fn first_output<'a>(
    outputs: &'a ort::session::SessionOutputs<'_>,
) -> DengjenResult<ort::value::ValueRef<'a>> {
    outputs
        .values()
        .next()
        .ok_or_else(|| inference_error("model returned no outputs"))
}

pub(crate) fn build_vits_inputs(
    phoneme_ids: Vec<i64>,
    scales: [f32; 3],
    speaker: Option<i64>,
) -> DengjenResult<Vec<SessionInputValue<'static>>> {
    let phoneme_count = phoneme_ids.len();
    let ids =
        Array2::<i64>::from_shape_vec((1, phoneme_count), phoneme_ids).map_err(inference_error)?;
    let lengths = Array1::<i64>::from_iter([phoneme_count as i64]);
    let scales = Array1::<f32>::from_iter(scales);

    let mut inputs: Vec<SessionInputValue<'static>> = vec![
        Tensor::from_array(ids).map_err(inference_error)?.into(),
        Tensor::from_array(lengths).map_err(inference_error)?.into(),
        Tensor::from_array(scales).map_err(inference_error)?.into(),
    ];
    if let Some(speaker_id) = speaker {
        let speaker_tensor =
            Tensor::from_array(Array1::<i64>::from_iter([speaker_id])).map_err(inference_error)?;
        inputs.push(speaker_tensor.into());
    }
    Ok(inputs)
}

pub(crate) fn snapshot_scales_and_speaker(
    synth_config: &RwLock<PiperSynthesisConfig>,
    num_speakers: u32,
    default_speaker_id: i64,
) -> ([f32; 3], Option<i64>) {
    let synth_config = read_ignoring_poison(synth_config);
    let scales = [
        synth_config.noise_scale,
        synth_config.length_scale,
        synth_config.noise_w,
    ];
    let speaker = (num_speakers > 1).then(|| synth_config.speaker.unwrap_or(default_speaker_id));
    (scales, speaker)
}

pub struct VitsModel {
    synth_config: RwLock<PiperSynthesisConfig>,
    config: ModelConfig,
    speaker_map: HashMap<i64, String>,
    session: Mutex<Session>,
    #[cfg_attr(not(all(feature = "tashkeel", feature = "espeak")), allow(dead_code))]
    tashkeel_engine: Option<TashkeelEngine>,
    #[cfg_attr(not(feature = "hebrew"), allow(dead_code))]
    hebrew_engine: Option<HebrewEngine>,
    #[cfg_attr(not(feature = "pinyin"), allow(dead_code))]
    pinyin_engine: Option<PinyinEngine>,
}

impl VitsModel {
    pub fn new(config_path: PathBuf, onnx_path: &Path) -> DengjenResult<Self> {
        let (config, synth_config) = load_model_config(&config_path)?;
        Self::from_config(config, synth_config, &config_path, onnx_path)
    }
    pub(crate) fn from_config(
        config: ModelConfig,
        synth_config: PiperSynthesisConfig,
        config_path: &Path,
        onnx_path: &Path,
    ) -> DengjenResult<Self> {
        let session = create_inference_session(onnx_path).map_err(session_init_error)?;
        let speaker_map = reversed_mapping(&config.speaker_id_map);
        let tashkeel_engine = create_tashkeel_engine(&config)?;
        let hebrew_engine = create_hebrew_engine(&config, config_path)?;
        let pinyin_engine = create_pinyin_engine(&config, config_path)?;
        Ok(Self {
            synth_config: RwLock::new(synth_config),
            config,
            speaker_map,
            session: Mutex::new(session),
            tashkeel_engine,
            hebrew_engine,
            pinyin_engine,
        })
    }
    fn infer_with_values(&self, input_phonemes: Vec<i64>) -> DengjenAudioResult {
        let (scales, speaker) = snapshot_scales_and_speaker(
            &self.synth_config,
            self.config.num_speakers,
            resolve_default_speaker_id(&self.config),
        );
        let inputs = build_vits_inputs(input_phonemes, scales, speaker)?;

        let mut session = lock_ignoring_poison(&self.session);
        let started_at = std::time::Instant::now();
        let outputs = session.run(inputs.as_slice()).map_err(inference_error)?;
        let inference_ms = started_at.elapsed().as_millis() as f32;

        let first = first_output(&outputs)?;
        let (_, samples) = first.try_extract_tensor::<f32>().map_err(inference_error)?;
        Ok(Audio::new(
            samples.to_vec().into(),
            self.config.audio.sample_rate as usize,
            Some(inference_ms),
        ))
    }
}

impl VitsModelCommons for VitsModel {
    fn get_synth_config(&self) -> &RwLock<PiperSynthesisConfig> {
        &self.synth_config
    }
    fn get_config(&self) -> &ModelConfig {
        &self.config
    }
    fn get_speaker_map(&self) -> &HashMap<i64, String> {
        &self.speaker_map
    }
    fn get_tashkeel_engine(&self) -> Option<&TashkeelEngine> {
        self.tashkeel_engine.as_ref()
    }
    fn get_hebrew_engine(&self) -> Option<&HebrewEngine> {
        self.hebrew_engine.as_ref()
    }
    fn get_pinyin_engine(&self) -> Option<&PinyinEngine> {
        self.pinyin_engine.as_ref()
    }
}

impl DengjenModel for VitsModel {
    #[tracing::instrument(skip(self, text), fields(text_len = text.len()), err)]
    fn phonemize_text(&self, text: &str) -> DengjenResult<Phonemes> {
        self.do_phonemize_text(text)
    }

    #[tracing::instrument(skip(self, phoneme_batches), fields(batch_len = phoneme_batches.len()), err)]
    fn speak_batch(&self, phoneme_batches: Vec<String>) -> DengjenResult<Vec<Audio>> {
        let (pad_id, bos_id, eos_id) = self.get_meta_ids()?;
        phoneme_batches
            .into_iter()
            .map(|phonemes| {
                let ids = self.phonemes_to_input_ids(&phonemes, pad_id, bos_id, eos_id);
                self.infer_with_values(ids)
            })
            .collect()
    }

    #[tracing::instrument(skip(self, phonemes), fields(phonemes_len = phonemes.len()), err)]
    fn speak_one_sentence(&self, phonemes: String) -> DengjenAudioResult {
        let (pad_id, bos_id, eos_id) = self.get_meta_ids()?;
        self.infer_with_values(self.phonemes_to_input_ids(&phonemes, pad_id, bos_id, eos_id))
    }
    fn get_default_synthesis_config(&self) -> DengjenResult<Option<SynthesisConfig>> {
        Ok(Some(SynthesisConfig::from(
            &self.factory_synthesis_config(),
        )))
    }
    fn get_fallback_synthesis_config(&self) -> DengjenResult<Option<SynthesisConfig>> {
        Ok(Some(SynthesisConfig::from(
            &read_ignoring_poison(&self.synth_config).clone(),
        )))
    }
    fn set_fallback_synthesis_config(
        &self,
        synthesis_config: &SynthesisConfig,
    ) -> DengjenResult<()> {
        self._do_set_default_synth_config(&PiperSynthesisConfig::from(synthesis_config))
    }
    fn get_language(&self) -> DengjenResult<Option<String>> {
        Ok(self.language())
    }
    fn get_speakers(&self) -> DengjenResult<Option<&HashMap<i64, String>>> {
        Ok(Some(self.get_speaker_map()))
    }
    fn speaker_name_to_id(&self, name: &str) -> DengjenResult<Option<i64>> {
        Ok(self.config.speaker_id_map.get(name).copied())
    }
    fn properties(&self) -> DengjenResult<HashMap<String, String>> {
        Ok(self.get_properties())
    }
    fn audio_output_info(&self) -> DengjenResult<AudioInfo> {
        self.get_audio_output_info()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_vits_inputs_adds_a_speaker_tensor_only_when_a_speaker_is_given() {
        let scales = [0.667, 1.0, 0.8];
        let single = build_vits_inputs(vec![1, 2, 3], scales, None).unwrap();
        let multi = build_vits_inputs(vec![1, 2, 3], scales, Some(2)).unwrap();
        assert_eq!(single.len(), 3);
        assert_eq!(multi.len(), 4);
    }

    #[test]
    fn unset_speaker_resolves_to_the_voice_default_speaker_for_multi_speaker_voices() {
        let synth_config = RwLock::new(PiperSynthesisConfig::default());
        let (_, speaker) = snapshot_scales_and_speaker(&synth_config, 4, 3);
        assert_eq!(speaker, Some(3));
    }

    #[test]
    fn explicit_speaker_overrides_the_default_and_single_speaker_voices_send_none() {
        let synth_config = RwLock::new(PiperSynthesisConfig {
            speaker: Some(1),
            ..Default::default()
        });
        assert_eq!(snapshot_scales_and_speaker(&synth_config, 4, 3).1, Some(1));
        assert_eq!(snapshot_scales_and_speaker(&synth_config, 1, 3).1, None);
    }
}
