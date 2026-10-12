#![allow(non_local_definitions)]
#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)
)]

#[cfg(feature = "tashkeel")]
use dengjen_tashkeel::{
    create_inference_engine, do_tashkeel, DengjenTashkeelResult,
    DynamicInferenceEngine as TashkeelInferenceEngine,
};
use dengjen_tts::{
    default_batch_size, AudioOutputConfig, DengjenSpeechStreamBatched, DengjenSpeechStreamLazy,
    DengjenSpeechStreamParallel, DengjenSpeechSynthesizer, RealtimeSpeechStream,
};
use dengjen_tts_core::{
    Audio, AudioInfo, CancellationToken, DengjenError, DengjenModel, DengjenResult, SynthesisConfig,
};
#[cfg(feature = "tashkeel")]
use once_cell::sync::Lazy;
use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use pyo3::types::PyBytes;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

#[cfg(feature = "tashkeel")]
static LIBTASHKEEL_ENGINE: Lazy<DengjenTashkeelResult<TashkeelInferenceEngine>> =
    Lazy::new(|| create_inference_engine(None));

#[cfg(feature = "tashkeel")]
fn should_diacritize(language: &str, use_tashkeel: Option<bool>) -> bool {
    if language != "ar" {
        return false;
    }
    use_tashkeel.unwrap_or(true)
}

#[cfg(not(feature = "tashkeel"))]
#[cfg_attr(not(feature = "espeak"), allow(dead_code))]
fn should_diacritize(_language: &str, _use_tashkeel: Option<bool>) -> bool {
    false
}

type PyDengjenResult<T> = Result<T, PyDengjenError>;

create_exception!(
    pydengjen,
    DengjenException,
    PyException,
    "Base Exception for all exceptions raised by pydengjen."
);

create_exception!(
    pydengjen,
    ResourceLoadError,
    DengjenException,
    "A model, config or other resource could not be loaded."
);
create_exception!(
    pydengjen,
    PhonemizationError,
    DengjenException,
    "Text could not be converted to phonemes."
);
create_exception!(
    pydengjen,
    InferenceError,
    DengjenException,
    "Model inference failed."
);
create_exception!(
    pydengjen,
    InvalidConfigurationError,
    DengjenException,
    "A voice config or an argument was invalid."
);
create_exception!(
    pydengjen,
    UnsupportedOperationError,
    DengjenException,
    "The loaded model does not support the requested operation."
);

#[derive(Debug)]
struct PyDengjenError(DengjenError);

impl From<DengjenError> for PyDengjenError {
    fn from(err: DengjenError) -> Self {
        PyDengjenError(err)
    }
}

impl From<PyDengjenError> for PyErr {
    fn from(py_err: PyDengjenError) -> Self {
        let message = format!("{}{}", py_err.0, py_err.0.cause_chain());
        match py_err.0.kind() {
            DengjenError::FailedToLoadResource(_) => ResourceLoadError::new_err(message),
            DengjenError::PhonemizationError(_) => PhonemizationError::new_err(message),
            DengjenError::InferenceError(_) => InferenceError::new_err(message),
            DengjenError::InvalidConfiguration(_) => InvalidConfigurationError::new_err(message),
            DengjenError::UnsupportedOperation(_) => UnsupportedOperationError::new_err(message),
            _ => DengjenException::new_err(message),
        }
    }
}

#[pyclass(weakref, module = "pydengjen", frozen)]
#[pyo3(name = "AudioInfo")]
struct PyWaveInfo(AudioInfo);

#[pymethods]
impl PyWaveInfo {
    #[getter]
    fn get_sample_rate(&self) -> usize {
        self.0.sample_rate
    }

    #[getter]
    fn get_num_channels(&self) -> usize {
        self.0.num_channels
    }

    #[getter]
    fn get_sample_width(&self) -> usize {
        self.0.sample_width
    }
}

impl From<AudioInfo> for PyWaveInfo {
    fn from(info: AudioInfo) -> Self {
        PyWaveInfo(info)
    }
}

#[pyclass(weakref, module = "pydengjen", frozen, from_py_object)]
#[pyo3(name = "AudioOutputConfig")]
#[derive(Clone)]
struct PyAudioOutputConfig(AudioOutputConfig);

#[pymethods]
impl PyAudioOutputConfig {
    #[new]
    #[pyo3(signature = (rate=None, volume=None, pitch=None, appended_silence_ms=None))]
    fn new(
        rate: Option<u8>,
        volume: Option<u8>,
        pitch: Option<u8>,
        appended_silence_ms: Option<u32>,
    ) -> Self {
        PyAudioOutputConfig(AudioOutputConfig {
            rate,
            volume,
            pitch,
            appended_silence_ms,
        })
    }
}

impl From<PyAudioOutputConfig> for AudioOutputConfig {
    fn from(config: PyAudioOutputConfig) -> Self {
        config.0
    }
}

#[pyclass(weakref, module = "pydengjen", frozen)]
struct WaveSamples(Audio);

#[pymethods]
impl WaveSamples {
    fn get_wave_bytes(&self, py: Python) -> Py<PyAny> {
        let wav_data = py.detach(move || self.0.as_wave_bytes());
        PyBytes::new(py, &wav_data).into()
    }

    fn save_to_file(&self, filename: &str) -> PyDengjenResult<()> {
        self.0
            .save_to_file(&PathBuf::from(filename))
            .map_err(|e| PyDengjenError::from(DengjenError::from(e)))?;
        Ok(())
    }

    #[getter]
    fn sample_rate(&self) -> usize {
        self.0.info.sample_rate
    }

    #[getter]
    fn num_channels(&self) -> usize {
        self.0.info.num_channels
    }

    #[getter]
    fn sample_width(&self) -> usize {
        self.0.info.sample_width
    }

    #[getter]
    fn inference_ms(&self) -> Option<f32> {
        self.0.inference_ms()
    }

    #[getter]
    fn duration_ms(&self) -> f32 {
        self.0.duration_ms()
    }

    #[getter]
    fn real_time_factor(&self) -> Option<f32> {
        self.0.real_time_factor()
    }
}

/// Advances `stream` with the GIL released; `Ok(None)` ends the Python iteration.
fn next_detached<I, T>(py: Python<'_>, stream: &mut I) -> PyResult<Option<T>>
where
    I: Iterator<Item = DengjenResult<T>> + Send,
    T: Send,
{
    py.detach(|| stream.next())
        .transpose()
        .map_err(|err| PyDengjenError::from(err).into())
}

#[pyclass(weakref, module = "pydengjen")]
struct LazySpeechStream(DengjenSpeechStreamLazy);

impl From<DengjenSpeechStreamLazy> for LazySpeechStream {
    fn from(stream: DengjenSpeechStreamLazy) -> Self {
        Self(stream)
    }
}

#[pymethods]
impl LazySpeechStream {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&mut self, py: Python) -> PyResult<Option<WaveSamples>> {
        Ok(next_detached(py, &mut self.0)?.map(WaveSamples))
    }
}

#[pyclass(weakref, module = "pydengjen")]
struct ParallelSpeechStream(DengjenSpeechStreamParallel);

impl From<DengjenSpeechStreamParallel> for ParallelSpeechStream {
    fn from(stream: DengjenSpeechStreamParallel) -> Self {
        Self(stream)
    }
}

#[pymethods]
impl ParallelSpeechStream {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&mut self, py: Python) -> PyResult<Option<WaveSamples>> {
        Ok(next_detached(py, &mut self.0)?.map(WaveSamples))
    }
}

#[pyclass(weakref, module = "pydengjen")]
struct BatchedSpeechStream(DengjenSpeechStreamBatched);

impl From<DengjenSpeechStreamBatched> for BatchedSpeechStream {
    fn from(stream: DengjenSpeechStreamBatched) -> Self {
        Self(stream)
    }
}

#[pymethods]
impl BatchedSpeechStream {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&mut self, py: Python) -> PyResult<Option<WaveSamples>> {
        Ok(next_detached(py, &mut self.0)?.map(WaveSamples))
    }
}

#[pyclass(weakref, module = "pydengjen")]
struct PyRealtimeSpeechStream(RealtimeSpeechStream);

#[pymethods]
impl PyRealtimeSpeechStream {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&mut self, py: Python) -> PyResult<Option<Py<PyAny>>> {
        Ok(next_detached(py, &mut self.0)?
            .map(|samples| PyBytes::new(py, &samples.as_wave_bytes()).into()))
    }
}

#[pyclass(weakref, module = "pydengjen")]
struct PiperScales {
    #[pyo3(get)]
    length_scale: f32,
    #[pyo3(get)]
    noise_scale: f32,
    #[pyo3(get)]
    noise_w: f32,
}

#[pymethods]
impl PiperScales {
    #[new]
    fn new(length_scale: f32, noise_scale: f32, noise_w: f32) -> Self {
        PiperScales {
            length_scale,
            noise_scale,
            noise_w,
        }
    }
}

#[cfg(test)]
mod value_type_tests {
    use super::*;
    use dengjen_tts_core::AudioSamples;
    use std::io::Read;

    #[test]
    fn py_wave_info_exposes_the_wrapped_audio_info_fields() {
        let info = AudioInfo {
            sample_rate: 22050,
            num_channels: 1,
            sample_width: 2,
        };
        let py_info = PyWaveInfo::from(info);
        assert_eq!(py_info.get_sample_rate(), 22050);
        assert_eq!(py_info.get_num_channels(), 1);
        assert_eq!(py_info.get_sample_width(), 2);
    }

    #[test]
    fn py_audio_output_config_new_stores_every_field() {
        let config = PyAudioOutputConfig::new(Some(150), Some(80), Some(50), Some(200));
        let inner: AudioOutputConfig = config.into();
        assert_eq!(inner.rate, Some(150));
        assert_eq!(inner.volume, Some(80));
        assert_eq!(inner.pitch, Some(50));
        assert_eq!(inner.appended_silence_ms, Some(200));
    }

    #[test]
    fn py_audio_output_config_new_accepts_all_none() {
        let config = PyAudioOutputConfig::new(None, None, None, None);
        let inner: AudioOutputConfig = config.into();
        assert_eq!(inner.rate, None);
        assert_eq!(inner.volume, None);
        assert_eq!(inner.pitch, None);
        assert_eq!(inner.appended_silence_ms, None);
    }

    fn sample_wave_samples() -> WaveSamples {
        WaveSamples(Audio::new(
            AudioSamples::new(vec![0.0, 0.25, -0.25, 0.5]),
            22050,
            Some(3.5),
        ))
    }

    #[test]
    fn wave_samples_getters_reflect_the_wrapped_audio() {
        let samples = sample_wave_samples();
        assert_eq!(samples.sample_rate(), 22050);
        assert_eq!(samples.num_channels(), 1);
        assert_eq!(samples.sample_width(), 2);
        assert_eq!(samples.inference_ms(), Some(3.5));
    }

    #[test]
    fn wave_samples_save_to_file_writes_a_readable_wav() {
        let samples = sample_wave_samples();
        let dir = std::env::temp_dir().join(format!("dengjen-python-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("wave_samples_save_to_file_writes_a_readable_wav.wav");
        let path_str = path.to_str().unwrap();

        samples.save_to_file(path_str).unwrap();

        let mut file = std::fs::File::open(&path).unwrap();
        let mut header = [0u8; 4];
        file.read_exact(&mut header).unwrap();
        assert_eq!(&header, b"RIFF");

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn wave_samples_save_to_file_reports_the_underlying_error() {
        let samples = sample_wave_samples();
        let result = samples.save_to_file("/nonexistent-directory-dengjen-test/out.wav");
        assert!(result.is_err());
    }

    #[test]
    fn piper_scales_new_stores_every_field() {
        let scales = PiperScales::new(0.9, 0.5, 0.7);
        assert_eq!(scales.length_scale, 0.9);
        assert_eq!(scales.noise_scale, 0.5);
        assert_eq!(scales.noise_w, 0.7);
    }
}

fn load_voice(config_path: &std::path::Path) -> DengjenResult<Arc<dyn DengjenModel + Send + Sync>> {
    dengjen_tts::load_voice_with(config_path, |model_type, path| match model_type {
        dengjen_tts::ModelType::Piper => dengjen_tts_piper::from_config_path(path),
        dengjen_tts::ModelType::Kokoro => dengjen_tts_kokoro::from_config_path(path),
        dengjen_tts::ModelType::MeloTts => dengjen_tts_melotts::from_config_path(path),
        other => Err(DengjenError::InvalidConfiguration(format!(
            "unsupported model type `{other}`"
        ))),
    })
}

#[pyclass(weakref, module = "pydengjen")]
#[pyo3(name = "PiperModel")]
struct PiperModel(Arc<dyn DengjenModel + Send + Sync>);

#[pymethods]
impl PiperModel {
    #[new]
    fn new(py: Python<'_>, config_path: &str) -> PyDengjenResult<Self> {
        let path = PathBuf::from(config_path);
        let model = py.detach(|| load_voice(&path))?;
        Ok(Self(model))
    }

    #[getter]
    fn get_speaker(&self) -> PyDengjenResult<Option<String>> {
        let Some(speaker_id) = self.current_speaker_id()? else {
            return Ok(None);
        };
        Ok(self.0.speaker_id_to_name(&speaker_id)?)
    }

    #[setter]
    fn set_speaker(&self, name: String) -> PyDengjenResult<()> {
        let Some(speaker_id) = self.0.speaker_name_to_id(&name)? else {
            return Err(PyDengjenError::from(DengjenError::OperationError(format!(
                "no speaker named '{name}'"
            ))));
        };
        let mut config = self.generic_config_or_err()?;
        config.speaker = Some(speaker_id);
        Ok(self.0.set_fallback_synthesis_config(&config)?)
    }

    fn get_scales(&self) -> PyDengjenResult<PiperScales> {
        let config = self.piper_config_or_err()?;
        Ok(PiperScales {
            length_scale: config.length_scale,
            noise_scale: config.noise_scale,
            noise_w: config.noise_w,
        })
    }

    fn set_scales(&self, length_scale: f32, noise_scale: f32, noise_w: f32) -> PyDengjenResult<()> {
        let mut config = self.generic_config_or_err()?;
        config.parameters.insert(
            dengjen_tts_piper::synth_config::LENGTH_SCALE.to_string(),
            length_scale,
        );
        config.parameters.insert(
            dengjen_tts_piper::synth_config::NOISE_SCALE.to_string(),
            noise_scale,
        );
        config.parameters.insert(
            dengjen_tts_piper::synth_config::NOISE_W.to_string(),
            noise_w,
        );
        Ok(self.0.set_fallback_synthesis_config(&config)?)
    }

    fn set_parameters(&self, parameters: HashMap<String, f32>) -> PyDengjenResult<()> {
        let mut config = self.0.get_fallback_synthesis_config()?.unwrap_or_default();
        config.parameters.extend(parameters);
        Ok(self.0.set_fallback_synthesis_config(&config)?)
    }

    fn get_parameters(&self) -> PyDengjenResult<HashMap<String, f32>> {
        Ok(self
            .0
            .get_fallback_synthesis_config()?
            .unwrap_or_default()
            .parameters)
    }
}

impl PiperModel {
    fn current_speaker_id(&self) -> PyDengjenResult<Option<i64>> {
        Ok(self
            .0
            .get_fallback_synthesis_config()?
            .and_then(|config| dengjen_tts_piper::PiperSynthesisConfig::from(&config).speaker))
    }

    fn piper_config_or_err(&self) -> PyDengjenResult<dengjen_tts_piper::PiperSynthesisConfig> {
        self.generic_config_or_err()
            .map(|config| dengjen_tts_piper::PiperSynthesisConfig::from(&config))
    }

    fn generic_config_or_err(&self) -> PyDengjenResult<SynthesisConfig> {
        self.0.get_fallback_synthesis_config()?.ok_or_else(|| {
            PyDengjenError::from(DengjenError::InvalidConfiguration(
                "this model has no Piper synthesis config to read or update".to_string(),
            ))
        })
    }
}

#[pyclass(weakref, module = "pydengjen", frozen)]
struct Dengjen(Arc<DengjenSpeechSynthesizer>);

#[pymethods]
impl Dengjen {
    #[staticmethod]
    #[pyo3(name = "with_piper")]
    fn py_with_piper(py: Python<'_>, vits_model: &PiperModel) -> PyDengjenResult<Self> {
        py.detach(|| Self::with_piper(vits_model))
    }

    #[pyo3(name = "synthesize", signature = (text, audio_output_config=None))]
    fn py_synthesize(
        &self,
        py: Python<'_>,
        text: String,
        audio_output_config: Option<PyAudioOutputConfig>,
    ) -> PyDengjenResult<LazySpeechStream> {
        py.detach(|| self.synthesize(text, audio_output_config))
    }

    #[pyo3(name = "synthesize_lazy", signature = (text, audio_output_config=None))]
    fn py_synthesize_lazy(
        &self,
        py: Python<'_>,
        text: String,
        audio_output_config: Option<PyAudioOutputConfig>,
    ) -> PyDengjenResult<LazySpeechStream> {
        py.detach(|| self.synthesize_lazy(text, audio_output_config))
    }

    #[pyo3(name = "synthesize_parallel", signature = (text, audio_output_config=None))]
    fn py_synthesize_parallel(
        &self,
        py: Python<'_>,
        text: String,
        audio_output_config: Option<PyAudioOutputConfig>,
    ) -> PyDengjenResult<ParallelSpeechStream> {
        py.detach(|| self.synthesize_parallel(text, audio_output_config))
    }

    #[pyo3(name = "synthesize_batched", signature = (text, audio_output_config=None, batch_size=None))]
    fn py_synthesize_batched(
        &self,
        py: Python<'_>,
        text: String,
        audio_output_config: Option<PyAudioOutputConfig>,
        batch_size: Option<usize>,
    ) -> PyDengjenResult<BatchedSpeechStream> {
        py.detach(|| self.synthesize_batched(text, audio_output_config, batch_size))
    }

    #[pyo3(name = "synthesize_streamed", signature = (text, audio_output_config=None, chunk_size=None, chunk_padding=None))]
    fn py_synthesize_streamed(
        &self,
        py: Python<'_>,
        text: String,
        audio_output_config: Option<PyAudioOutputConfig>,
        chunk_size: Option<usize>,
        chunk_padding: Option<usize>,
    ) -> PyDengjenResult<PyRealtimeSpeechStream> {
        py.detach(|| self.synthesize_streamed(text, audio_output_config, chunk_size, chunk_padding))
    }

    #[pyo3(name = "synthesize_to_file", signature = (filename, text, audio_output_config=None))]
    fn py_synthesize_to_file(
        &self,
        py: Python<'_>,
        filename: &str,
        text: String,
        audio_output_config: Option<PyAudioOutputConfig>,
    ) -> PyDengjenResult<()> {
        py.detach(|| self.synthesize_to_file(filename, text, audio_output_config))
    }

    #[getter]
    fn language(&self) -> PyDengjenResult<Option<String>> {
        Ok(self.0.get_language()?)
    }

    #[getter]
    fn speakers(&self) -> PyDengjenResult<Option<HashMap<i64, String>>> {
        Ok(self.0.get_speakers()?.cloned())
    }

    fn get_audio_output_info(&self) -> PyDengjenResult<PyWaveInfo> {
        Ok(self.0.audio_output_info()?.into())
    }
}

impl Dengjen {
    fn with_piper(vits_model: &PiperModel) -> PyDengjenResult<Self> {
        let model = Arc::clone(&vits_model.0);
        Ok(Self(Arc::new(DengjenSpeechSynthesizer::new(model)?)))
    }

    fn synthesize(
        &self,
        text: String,
        audio_output_config: Option<PyAudioOutputConfig>,
    ) -> PyDengjenResult<LazySpeechStream> {
        self.synthesize_lazy(text, audio_output_config)
    }

    fn synthesize_lazy(
        &self,
        text: String,
        audio_output_config: Option<PyAudioOutputConfig>,
    ) -> PyDengjenResult<LazySpeechStream> {
        let stream = self
            .0
            .synthesize_lazy(text, audio_output_config.map(|o| o.into()))?;
        Ok(stream.into())
    }

    fn synthesize_parallel(
        &self,
        text: String,
        audio_output_config: Option<PyAudioOutputConfig>,
    ) -> PyDengjenResult<ParallelSpeechStream> {
        let stream = self
            .0
            .synthesize_parallel(text, audio_output_config.map(|o| o.into()))?;
        Ok(stream.into())
    }

    fn synthesize_batched(
        &self,
        text: String,
        audio_output_config: Option<PyAudioOutputConfig>,
        batch_size: Option<usize>,
    ) -> PyDengjenResult<BatchedSpeechStream> {
        let batch_size = match batch_size {
            None => default_batch_size(),
            Some(n) => std::num::NonZeroUsize::new(n).ok_or_else(|| {
                PyDengjenError::from(DengjenError::InvalidConfiguration(
                    "batch_size must be greater than zero".to_string(),
                ))
            })?,
        };
        let stream =
            self.0
                .synthesize_batched(text, audio_output_config.map(|o| o.into()), batch_size)?;
        Ok(stream.into())
    }

    fn synthesize_streamed(
        &self,
        text: String,
        audio_output_config: Option<PyAudioOutputConfig>,
        chunk_size: Option<usize>,
        chunk_padding: Option<usize>,
    ) -> PyDengjenResult<PyRealtimeSpeechStream> {
        let stream = self.0.synthesize_streamed(
            text,
            audio_output_config.map(|o| o.into()),
            chunk_size.unwrap_or(45),
            chunk_padding.unwrap_or(3),
            CancellationToken::new(),
        )?;
        Ok(PyRealtimeSpeechStream(stream))
    }

    fn synthesize_to_file(
        &self,
        filename: &str,
        text: String,
        audio_output_config: Option<PyAudioOutputConfig>,
    ) -> PyDengjenResult<()> {
        self.0.synthesize_to_file(
            &PathBuf::from(filename),
            text,
            audio_output_config.map(|o| o.into()),
        )?;
        Ok(())
    }
}

#[cfg(feature = "tashkeel")]
fn diacritize_text(text: &str) -> PyResult<std::borrow::Cow<'_, str>> {
    let engine = LIBTASHKEEL_ENGINE
        .as_ref()
        .map_err(|err| DengjenException::new_err(err.to_string()))?;
    do_tashkeel(engine, text, None, false)
        .map(std::borrow::Cow::from)
        .map_err(|err| DengjenException::new_err(err.to_string()))
}
// should_diacritize() is always false without this feature, so this is unreachable.
#[cfg(not(feature = "tashkeel"))]
#[cfg_attr(not(feature = "espeak"), allow(dead_code))]
fn diacritize_text(_text: &str) -> PyResult<std::borrow::Cow<'_, str>> {
    unreachable!("diacritize_text called with the `tashkeel` feature disabled")
}

#[cfg(feature = "espeak")]
#[pyfunction]
#[pyo3(signature = (text, language, phoneme_separator=None, remove_lang_switch_flags=None, remove_stress=None, use_tashkeel=None))]
pub fn phonemize_text(
    py: Python<'_>,
    text: &str,
    language: &str,
    phoneme_separator: Option<char>,
    remove_lang_switch_flags: Option<bool>,
    remove_stress: Option<bool>,
    use_tashkeel: Option<bool>,
) -> PyResult<Vec<String>> {
    py.detach(|| {
        let text = if should_diacritize(language, use_tashkeel) {
            diacritize_text(text)?
        } else {
            std::borrow::Cow::from(text)
        };
        dengjen_espeak_phonemizer::text_to_phonemes(
            &text,
            language,
            phoneme_separator,
            remove_lang_switch_flags.unwrap_or(true),
            remove_stress.unwrap_or(false),
        )
        .map_err(|err| {
            PyErr::from(PyDengjenError::from(DengjenError::PhonemizationError(
                err.to_string(),
            )))
        })
    })
}

#[cfg(test)]
mod error_plumbing_tests {
    use super::*;

    #[cfg(feature = "tashkeel")]
    #[test]
    fn should_diacritize_true_for_arabic_language_by_default() {
        assert!(should_diacritize("ar", None));
    }

    #[cfg(feature = "tashkeel")]
    #[test]
    fn should_diacritize_false_for_non_arabic_language() {
        assert!(!should_diacritize("en-us", None));
    }

    #[cfg(feature = "tashkeel")]
    #[test]
    fn should_diacritize_false_for_arabic_language_when_explicitly_disabled() {
        assert!(!should_diacritize("ar", Some(false)));
    }

    #[cfg(feature = "tashkeel")]
    #[test]
    fn should_diacritize_true_for_arabic_language_when_explicitly_enabled() {
        assert!(should_diacritize("ar", Some(true)));
    }

    #[cfg(not(feature = "tashkeel"))]
    #[test]
    fn should_diacritize_always_false_when_tashkeel_disabled() {
        assert!(!should_diacritize("ar", None));
        assert!(!should_diacritize("ar", Some(true)));
    }

    #[test]
    fn py_dengjen_error_wraps_the_original_error_unchanged() {
        let original = DengjenError::OperationError("boom".to_string());
        let wrapped = PyDengjenError::from(original);
        assert!(matches!(wrapped.0, DengjenError::OperationError(ref s) if s == "boom"));
    }

    #[test]
    fn py_dengjen_error_preserves_variant_across_each_error_kind() {
        let cases = [
            DengjenError::FailedToLoadResource("a".to_string()),
            DengjenError::PhonemizationError("b".to_string()),
            DengjenError::InferenceError("c".to_string()),
            DengjenError::InvalidConfiguration("d".to_string()),
            DengjenError::UnsupportedOperation("e".to_string()),
            DengjenError::OperationError("f".to_string()),
        ];
        for original in cases {
            let expected = original.to_string();
            let wrapped = PyDengjenError::from(original);
            assert_eq!(wrapped.0.to_string(), expected);
        }
    }
}

#[pymodule]
fn pydengjen(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Dengjen>()?;
    m.add_class::<PiperModel>()?;
    m.add_class::<PiperScales>()?;
    m.add_class::<PyAudioOutputConfig>()?;
    m.add_class::<WaveSamples>()?;

    m.add_class::<LazySpeechStream>()?;
    m.add_class::<ParallelSpeechStream>()?;
    m.add_class::<BatchedSpeechStream>()?;
    m.add_class::<PyRealtimeSpeechStream>()?;

    m.add("DengjenException", m.py().get_type::<DengjenException>())?;
    m.add("ResourceLoadError", m.py().get_type::<ResourceLoadError>())?;
    m.add(
        "PhonemizationError",
        m.py().get_type::<PhonemizationError>(),
    )?;
    m.add("InferenceError", m.py().get_type::<InferenceError>())?;
    m.add(
        "InvalidConfigurationError",
        m.py().get_type::<InvalidConfigurationError>(),
    )?;
    m.add(
        "UnsupportedOperationError",
        m.py().get_type::<UnsupportedOperationError>(),
    )?;

    #[cfg(feature = "espeak")]
    m.add_function(wrap_pyfunction!(phonemize_text, m)?)?;

    Ok(())
}

#[cfg(test)]
mod model_and_synthesizer_tests {
    use super::*;
    use dengjen_tts_core::{AudioSamples, DengjenAudioResult, DengjenResult, Phonemes};
    use dengjen_tts_piper::PiperSynthesisConfig;
    use std::collections::HashMap as StdHashMap;
    use std::sync::Mutex;

    // Round-tripping through `PiperSynthesisConfig` (a closed, fixed-field struct) would
    // silently drop anything `set_parameters` adds beyond its three known scale fields, so
    // the fallback config is kept as the generic `SynthesisConfig` instead.
    struct FakeModel {
        speakers: StdHashMap<i64, String>,
        fallback_config: Mutex<Option<SynthesisConfig>>,
    }

    impl FakeModel {
        fn with_one_speaker() -> Self {
            Self::with_speakers(&[(0, "alice")])
        }

        fn with_speakers(speakers: &[(i64, &str)]) -> Self {
            let speakers = speakers
                .iter()
                .map(|(id, name)| (*id, name.to_string()))
                .collect();
            Self {
                speakers,
                fallback_config: Mutex::new(Some(SynthesisConfig::from(&PiperSynthesisConfig {
                    speaker: Some(0),
                    noise_scale: 0.667,
                    length_scale: 1.0,
                    noise_w: 0.8,
                }))),
            }
        }

        /// A model whose fallback config has no speaker id set yet, but is
        /// still a valid `Piper` config — distinct from `with_no_config()`.
        fn with_speaker_unset() -> Self {
            Self {
                speakers: StdHashMap::from([(0, "alice".to_string())]),
                fallback_config: Mutex::new(Some(SynthesisConfig::from(&PiperSynthesisConfig {
                    speaker: None,
                    noise_scale: 0.667,
                    length_scale: 1.0,
                    noise_w: 0.8,
                }))),
            }
        }

        /// A model with no synthesis config at all, as opposed to a `Piper` config
        /// that simply has no speaker set. Keeps a speaker table so `set_speaker`'s
        /// name-to-id lookup succeeds and the test exercises the config-write failure,
        /// not a lookup miss.
        fn with_no_config() -> Self {
            Self {
                speakers: StdHashMap::from([(0, "alice".to_string())]),
                fallback_config: Mutex::new(None),
            }
        }
    }

    impl DengjenModel for FakeModel {
        fn audio_output_info(&self) -> DengjenResult<AudioInfo> {
            Ok(AudioInfo {
                sample_rate: 22050,
                num_channels: 1,
                sample_width: 2,
            })
        }
        fn phonemize_text(&self, text: &str) -> DengjenResult<Phonemes> {
            Ok(Phonemes::from(vec![text.to_string()]))
        }
        fn speak_batch(&self, phoneme_batches: Vec<String>) -> DengjenResult<Vec<Audio>> {
            phoneme_batches
                .into_iter()
                .map(|p| self.speak_one_sentence(p))
                .collect()
        }
        fn speak_one_sentence(&self, _phonemes: String) -> DengjenAudioResult {
            // Above the synth pipeline's trailing-silence trim threshold, else
            // this stub gets trimmed away to nothing.
            Ok(Audio::new(
                AudioSamples::new(vec![0.5; 100]),
                22050,
                Some(1.5),
            ))
        }
        fn get_default_synthesis_config(&self) -> DengjenResult<Option<SynthesisConfig>> {
            Ok(self.fallback_config.lock().unwrap().clone())
        }
        fn get_fallback_synthesis_config(&self) -> DengjenResult<Option<SynthesisConfig>> {
            Ok(self.fallback_config.lock().unwrap().clone())
        }
        fn set_fallback_synthesis_config(&self, c: &SynthesisConfig) -> DengjenResult<()> {
            *self.fallback_config.lock().unwrap() = Some(c.clone());
            Ok(())
        }
        fn get_language(&self) -> DengjenResult<Option<String>> {
            Ok(Some("en-us".to_string()))
        }
        fn get_speakers(&self) -> DengjenResult<Option<&StdHashMap<i64, String>>> {
            Ok(Some(&self.speakers))
        }
    }

    fn fake_piper_model() -> PiperModel {
        PiperModel(Arc::new(FakeModel::with_one_speaker()))
    }

    #[test]
    fn load_voice_errors_on_a_missing_config_path() {
        let path = std::path::Path::new("/nonexistent-dengjen-python-load-voice-test.json");
        assert!(load_voice(path).is_err());
    }

    fn write_temp_config(dir: &std::path::Path, name: &str, contents: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn load_voice_routes_kokoro_model_type_toward_the_kokoro_loader() {
        let dir = std::env::temp_dir().join("dengjen_python_load_voice_test_kokoro");
        std::fs::create_dir_all(&dir).unwrap();

        let path = write_temp_config(&dir, "config.json", r#"{"model_type": "kokoro"}"#);
        let err = match load_voice(&path) {
            Err(e) => format!("{}{}", e, e.cause_chain()),
            Ok(_) => panic!("expected an error for an incomplete Kokoro config"),
        };
        assert!(
            err.contains("model_path"),
            "expected a Kokoro-loader error naming the missing `model_path` field, got: {err}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_voice_rejects_an_unknown_model_type_instead_of_loading_as_piper() {
        let dir = std::env::temp_dir().join(format!(
            "dengjen_python_load_voice_test_unknown_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = write_temp_config(&dir, "config.json", r#"{"model_type": "bark"}"#);
        let err = match load_voice(&path) {
            Err(e) => e,
            Ok(_) => panic!("expected an error for an unknown model_type"),
        };
        assert!(matches!(err, DengjenError::InvalidConfiguration(_)));
        assert!(err.to_string().contains("bark"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_voice_routes_melotts_model_type_toward_the_melotts_loader() {
        let dir = std::env::temp_dir().join("dengjen_python_load_voice_test_melotts");
        std::fs::create_dir_all(&dir).unwrap();

        let path = write_temp_config(
            &dir,
            "config.json",
            r#"{"model_type": "melotts", "audio": {"sample_rate": 24000}}"#,
        );
        let err = match load_voice(&path) {
            Err(e) => format!("{}{}", e, e.cause_chain()),
            Ok(_) => panic!("expected an error for an incomplete MeloTTS config"),
        };
        assert!(
            err.contains("phonemizer"),
            "expected a MeloTTS-loader error naming the missing `phonemizer` field, got: {err}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_voice_routes_vits_model_type_toward_the_piper_loader() {
        let dir = std::env::temp_dir().join("dengjen_python_load_voice_test_vits");
        std::fs::create_dir_all(&dir).unwrap();

        let path = write_temp_config(&dir, "config.json", r#"{"model_type": "vits"}"#);
        let err = match load_voice(&path) {
            Err(e) => format!("{}{}", e, e.cause_chain()),
            Ok(_) => panic!("expected an error for an incomplete VITS config"),
        };
        assert!(
            err.contains("audio"),
            "expected a Piper-loader error naming the missing `audio` field, got: {err}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn piper_model_get_speaker_resolves_id_to_name() {
        let model = fake_piper_model();
        assert_eq!(model.get_speaker().unwrap(), Some("alice".to_string()));
    }

    #[test]
    fn piper_model_set_speaker_updates_the_fallback_config_by_name() {
        let model = PiperModel(Arc::new(FakeModel::with_speakers(&[
            (0, "alice"),
            (1, "bob"),
        ])));
        assert_eq!(model.get_speaker().unwrap(), Some("alice".to_string()));
        model.set_speaker("bob".to_string()).unwrap();
        assert_eq!(model.get_speaker().unwrap(), Some("bob".to_string()));
    }

    #[test]
    fn piper_model_set_speaker_errors_for_an_unknown_name() {
        let model = fake_piper_model();
        let result = model.set_speaker("nobody".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn piper_model_set_speaker_preserves_generic_parameters_set_earlier() {
        let model = PiperModel(Arc::new(FakeModel::with_speakers(&[
            (0, "alice"),
            (1, "bob"),
        ])));
        let mut parameters = HashMap::new();
        parameters.insert("custom_knob".to_string(), 1.25);
        model.set_parameters(parameters).unwrap();
        model.set_speaker("bob".to_string()).unwrap();
        let result = model.get_parameters().unwrap();
        assert_eq!(result.get("custom_knob"), Some(&1.25));
    }

    #[test]
    fn piper_model_get_and_set_scales_round_trip() {
        let model = fake_piper_model();
        model.set_scales(0.9, 0.5, 0.7).unwrap();
        let scales = model.get_scales().unwrap();
        assert_eq!(scales.length_scale, 0.9);
        assert_eq!(scales.noise_scale, 0.5);
        assert_eq!(scales.noise_w, 0.7);
    }

    #[test]
    fn piper_model_set_scales_preserves_generic_parameters_set_earlier() {
        let model = PiperModel(Arc::new(FakeModel::with_one_speaker()));
        let mut parameters = HashMap::new();
        parameters.insert("custom_knob".to_string(), 1.25);
        model.set_parameters(parameters).unwrap();
        model.set_scales(0.9, 0.5, 0.7).unwrap();
        let result = model.get_parameters().unwrap();
        assert_eq!(result.get("custom_knob"), Some(&1.25));
    }

    #[test]
    fn set_parameters_merges_into_the_fallback_config() {
        let model = PiperModel(Arc::new(FakeModel::with_one_speaker()));
        let mut parameters = HashMap::new();
        parameters.insert("custom_knob".to_string(), 1.25);
        model.set_parameters(parameters).unwrap();
        let raw = model.0.get_fallback_synthesis_config().unwrap().unwrap();
        assert_eq!(raw.parameters.get("custom_knob"), Some(&1.25));
    }

    #[test]
    fn get_parameters_reads_a_value_set_via_set_parameters() {
        let model = PiperModel(Arc::new(FakeModel::with_one_speaker()));
        let mut parameters = HashMap::new();
        parameters.insert("custom_knob".to_string(), 1.25);
        model.set_parameters(parameters).unwrap();
        let result = model.get_parameters().unwrap();
        assert_eq!(result.get("custom_knob"), Some(&1.25));
    }

    #[test]
    fn get_parameters_returns_an_empty_dict_for_a_configless_backend() {
        let model = PiperModel(Arc::new(FakeModel::with_no_config()));
        let result = model.get_parameters().unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn piper_model_get_speaker_returns_none_for_a_piper_config_with_no_speaker_set() {
        let model = PiperModel(Arc::new(FakeModel::with_speaker_unset()));
        assert_eq!(model.get_speaker().unwrap(), None);
    }

    #[test]
    fn piper_model_get_speaker_returns_none_when_there_is_no_synthesis_config() {
        let model = PiperModel(Arc::new(FakeModel::with_no_config()));
        assert_eq!(model.get_speaker().unwrap(), None);
    }

    #[test]
    fn piper_model_get_scales_errors_when_there_is_no_synthesis_config() {
        let model = PiperModel(Arc::new(FakeModel::with_no_config()));
        assert!(model.get_scales().is_err());
    }

    #[test]
    fn piper_model_set_scales_errors_when_there_is_no_synthesis_config() {
        let model = PiperModel(Arc::new(FakeModel::with_no_config()));
        assert!(model.set_scales(0.9, 0.5, 0.7).is_err());
    }

    #[test]
    fn piper_model_set_speaker_errors_when_there_is_no_synthesis_config() {
        let model = PiperModel(Arc::new(FakeModel::with_no_config()));
        assert!(model.set_speaker("alice".to_string()).is_err());
    }

    fn fake_dengjen() -> Dengjen {
        let piper_model = fake_piper_model();
        Dengjen::with_piper(&piper_model).unwrap()
    }

    #[test]
    fn dengjen_with_piper_succeeds_for_a_valid_model() {
        let _ = fake_dengjen();
    }

    #[test]
    fn dengjen_language_delegates_to_the_model() {
        let dengjen = fake_dengjen();
        assert_eq!(dengjen.language().unwrap(), Some("en-us".to_string()));
    }

    #[test]
    fn dengjen_speakers_delegates_to_the_model() {
        let dengjen = fake_dengjen();
        let speakers = dengjen.speakers().unwrap().unwrap();
        assert_eq!(speakers.get(&0), Some(&"alice".to_string()));
    }

    #[test]
    fn dengjen_get_audio_output_info_delegates_to_the_model() {
        let dengjen = fake_dengjen();
        let info = dengjen.get_audio_output_info().unwrap();
        assert_eq!(info.get_sample_rate(), 22050);
    }

    #[test]
    fn dengjen_synthesize_lazy_produces_a_stream_whose_inner_iterator_yields_the_fake_models_audio()
    {
        let dengjen = fake_dengjen();
        let mut stream = dengjen.synthesize_lazy("hello".to_string(), None).unwrap();
        let first = stream.0.next().unwrap().unwrap();
        assert_eq!(first.into_vec().len(), 100);
        assert!(stream.0.next().is_none());
    }

    #[test]
    fn dengjen_synthesize_parallel_produces_a_stream_whose_inner_iterator_yields_the_fake_models_audio(
    ) {
        let dengjen = fake_dengjen();
        let mut stream = dengjen
            .synthesize_parallel("hello".to_string(), None)
            .unwrap();
        let first = stream.0.next().unwrap().unwrap();
        assert_eq!(first.into_vec().len(), 100);
        assert!(stream.0.next().is_none());
    }

    #[test]
    fn dengjen_synthesize_batched_produces_a_stream_whose_inner_iterator_yields_the_fake_models_audio(
    ) {
        let dengjen = fake_dengjen();
        let mut stream = dengjen
            .synthesize_batched("hello".to_string(), None, Some(1))
            .unwrap();
        let first = stream.0.next().unwrap().unwrap();
        assert_eq!(first.into_vec().len(), 100);
        assert!(stream.0.next().is_none());
    }

    #[test]
    fn dengjen_synthesize_batched_rejects_an_explicit_zero_batch_size() {
        let dengjen = fake_dengjen();
        let result = dengjen.synthesize_batched("hello".to_string(), None, Some(0));
        assert!(result.is_err());
    }

    #[test]
    fn dengjen_synthesize_streamed_constructs_successfully() {
        let dengjen = fake_dengjen();
        let result = dengjen.synthesize_streamed("hello".to_string(), None, None, None);
        assert!(result.is_ok());
    }

    #[test]
    fn dengjen_synthesize_defaults_to_the_lazy_strategy() {
        let dengjen = fake_dengjen();
        let mut via_synthesize = dengjen.synthesize("hello".to_string(), None).unwrap();
        let first = via_synthesize.0.next().unwrap().unwrap();
        assert_eq!(first.into_vec().len(), 100);
    }

    #[test]
    fn dengjen_synthesize_to_file_writes_a_readable_wav() {
        let dengjen = fake_dengjen();
        let dir = std::env::temp_dir().join(format!("dengjen-python-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("dengjen_synthesize_to_file_writes_a_readable_wav.wav");

        dengjen
            .synthesize_to_file(path.to_str().unwrap(), "hello".to_string(), None)
            .unwrap();

        assert!(path.exists());
        std::fs::remove_file(&path).ok();
    }
}
