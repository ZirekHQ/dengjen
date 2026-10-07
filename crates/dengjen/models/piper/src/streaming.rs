use crate::config::ModelConfig;
use crate::inference::{
    build_vits_inputs, create_inference_session, inference_error, reversed_mapping,
    session_init_error, snapshot_scales_and_speaker,
};
use crate::phonemize::{
    create_hebrew_engine, create_pinyin_engine, create_tashkeel_engine, HebrewEngine, PinyinEngine,
    TashkeelEngine,
};
use crate::synth_config::PiperSynthesisConfig;
use crate::VitsModelCommons;
use dengjen_tts_core::{
    lock_ignoring_poison, read_ignoring_poison, Audio, AudioInfo, AudioSamples,
    AudioStreamIterator, CancellationToken, DengjenAudioResult, DengjenError, DengjenModel,
    DengjenResult, Phonemes, SynthesisConfig,
};
use ndarray::{Array, Array1, ArrayView, Axis, Dim, IxDynImpl};
use ort::session::{Session, SessionInputValue, SessionOutputs};
use ort::value::{Tensor, TensorRef};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};

const MIN_CHUNK_SIZE: isize = 44;
const MAX_CHUNK_SIZE: usize = 1024;
const CHUNK_CROSSFADE_SAMPLES: usize = 42;
const DEFAULT_HOP_LENGTH: usize = 256;

pub struct VitsStreamingModel {
    synth_config: RwLock<PiperSynthesisConfig>,
    config: ModelConfig,
    speaker_map: HashMap<i64, String>,
    encoder_model: Mutex<Session>,
    decoder_model: Arc<Mutex<Session>>,
    #[cfg_attr(not(all(feature = "tashkeel", feature = "espeak")), allow(dead_code))]
    tashkeel_engine: Option<TashkeelEngine>,
    #[cfg_attr(not(feature = "hebrew"), allow(dead_code))]
    hebrew_engine: Option<HebrewEngine>,
    #[cfg_attr(not(feature = "pinyin"), allow(dead_code))]
    pinyin_engine: Option<PinyinEngine>,
}

impl VitsStreamingModel {
    pub(crate) fn from_config(
        config: ModelConfig,
        synth_config: PiperSynthesisConfig,
        config_path: &Path,
        encoder_path: &Path,
        decoder_path: &Path,
    ) -> DengjenResult<Self> {
        let encoder_model = create_inference_session(encoder_path).map_err(session_init_error)?;
        let decoder_model = create_inference_session(decoder_path).map_err(session_init_error)?;
        let speaker_map = reversed_mapping(&config.speaker_id_map);
        let tashkeel_engine = create_tashkeel_engine(&config)?;
        let hebrew_engine = create_hebrew_engine(&config, config_path)?;
        let pinyin_engine = create_pinyin_engine(&config, config_path)?;
        Ok(Self {
            synth_config: RwLock::new(synth_config),
            config,
            speaker_map,
            encoder_model: Mutex::new(encoder_model),
            decoder_model: Arc::new(Mutex::new(decoder_model)),
            tashkeel_engine,
            hebrew_engine,
            pinyin_engine,
        })
    }

    fn infer_with_values(&self, input_phonemes: Vec<i64>) -> DengjenAudioResult {
        let started_at = std::time::Instant::now();
        let encoder_outputs = self.infer_encoder(input_phonemes)?;
        let samples = encoder_outputs.infer_decoder(&self.decoder_model)?;
        let inference_ms = started_at.elapsed().as_millis() as f32;
        Ok(Audio::new(
            samples,
            self.config.audio.sample_rate as usize,
            Some(inference_ms),
        ))
    }

    fn infer_encoder(&self, input_phonemes: Vec<i64>) -> DengjenResult<EncoderOutputs> {
        let (scales, speaker) =
            snapshot_scales_and_speaker(&self.synth_config, self.config.num_speakers);
        let inputs = build_vits_inputs(input_phonemes, scales, speaker);

        let mut session = lock_ignoring_poison(&self.encoder_model);
        let outputs = session.run(inputs.as_slice()).map_err(inference_error)?;
        EncoderOutputs::from_values(outputs)
    }
}

impl VitsModelCommons for VitsStreamingModel {
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

impl DengjenModel for VitsStreamingModel {
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
        chunk_padding: usize,
        cancel_token: CancellationToken,
    ) -> DengjenResult<AudioStreamIterator<'_>> {
        // Skip the encoder pass entirely rather than erroring: a cancellation must stay silent.
        if cancel_token.is_cancelled() {
            return Ok(Box::new(std::iter::empty()));
        }
        let (pad_id, bos_id, eos_id) = self.get_meta_ids()?;
        let encoder_outputs =
            self.infer_encoder(self.phonemes_to_input_ids(&phonemes, pad_id, bos_id, eos_id))?;
        Ok(Box::new(SpeechStreamer::new(
            Arc::clone(&self.decoder_model),
            encoder_outputs,
            chunk_size,
            chunk_padding,
            self.config.hop_length.unwrap_or(DEFAULT_HOP_LENGTH),
            cancel_token,
        )))
    }
}

struct EncoderOutputs {
    z: Array<f32, Dim<IxDynImpl>>,
    y_mask: Array<f32, Dim<IxDynImpl>>,
    #[allow(dead_code)]
    p_duration: Option<Array<f32, Dim<IxDynImpl>>>,
    g: Array<f32, Dim<IxDynImpl>>,
}

fn extract_named_array(
    values: &SessionOutputs,
    name: &str,
) -> DengjenResult<Array<f32, Dim<IxDynImpl>>> {
    let (shape, data) = values[name]
        .try_extract_tensor::<f32>()
        .map_err(inference_error)?;
    Ok(Array::from_shape_vec(shape.to_ixdyn(), data.to_vec()).unwrap())
}

impl EncoderOutputs {
    fn from_values(values: SessionOutputs) -> DengjenResult<Self> {
        let optional = |name: &str| -> DengjenResult<Option<Array<f32, Dim<IxDynImpl>>>> {
            values
                .contains_key(name)
                .then(|| extract_named_array(&values, name))
                .transpose()
        };
        let z = extract_named_array(&values, "z")?;
        let y_mask = extract_named_array(&values, "y_mask")?;
        for (name, array) in [("z", &z), ("y_mask", &y_mask)] {
            if array.ndim() < 3 {
                return Err(DengjenError::with_message(format!(
                    "Encoder output `{name}` must have at least 3 dimensions, got shape {:?}",
                    array.shape()
                )));
            }
        }
        Ok(Self {
            z,
            y_mask,
            p_duration: optional("p_duration")?,

            g: optional("g")?.unwrap_or_else(|| Array1::<f32>::from_iter([]).into_dyn()),
        })
    }

    fn decoder_inputs<'a>(
        &'a self,
        z: ArrayView<'a, f32, Dim<IxDynImpl>>,
        y_mask: ArrayView<'a, f32, Dim<IxDynImpl>>,
    ) -> DengjenResult<Vec<SessionInputValue<'a>>> {
        // Slicing the frame axis yields non-contiguous views, which ort rejects.
        let input =
            |view: ArrayView<'a, f32, Dim<IxDynImpl>>| -> DengjenResult<SessionInputValue<'a>> {
                if view.is_standard_layout() {
                    Ok(TensorRef::from_array_view(view)
                        .map_err(inference_error)?
                        .into())
                } else {
                    Ok(Tensor::from_array(view.as_standard_layout().into_owned())
                        .map_err(inference_error)?
                        .into())
                }
            };
        let mut inputs = vec![input(z)?, input(y_mask)?];
        if !self.g.is_empty() {
            inputs.push(input(self.g.view())?);
        }
        Ok(inputs)
    }

    fn infer_decoder(&self, session: &Mutex<Session>) -> DengjenResult<AudioSamples> {
        let inputs = self.decoder_inputs(self.z.view(), self.y_mask.view())?;
        let mut session = lock_ignoring_poison(session);
        let outputs = session.run(inputs.as_slice()).map_err(inference_error)?;
        let (_, samples) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(inference_error)?;
        Ok(samples.to_vec().into())
    }
}

struct SpeechStreamer {
    decoder_model: Arc<Mutex<Session>>,
    encoder_outputs: EncoderOutputs,
    mel_chunker: AdaptiveMelChunker,
    one_shot: bool,
    cancel_token: CancellationToken,
}

impl SpeechStreamer {
    fn new(
        decoder_model: Arc<Mutex<Session>>,
        encoder_outputs: EncoderOutputs,
        chunk_size: usize,
        chunk_padding: usize,
        hop_length: usize,
        cancel_token: CancellationToken,
    ) -> Self {
        let num_frames = encoder_outputs.z.shape()[2];

        let one_shot = num_frames <= (chunk_size * 2 + chunk_padding * 2);
        Self {
            mel_chunker: AdaptiveMelChunker::new(
                num_frames as isize,
                chunk_size as isize,
                chunk_padding as isize,
                hop_length as isize,
            ),
            decoder_model,
            encoder_outputs,
            one_shot,
            cancel_token,
        }
    }

    fn synthesize_chunk(
        &self,
        mel_index: ndarray::Slice,
        audio_index: ndarray::Slice,
    ) -> DengjenResult<AudioSamples> {
        let z = self.encoder_outputs.z.view();
        let y_mask = self.encoder_outputs.y_mask.view();
        let inputs = self.encoder_outputs.decoder_inputs(
            z.slice_axis(Axis(2), mel_index),
            y_mask.slice_axis(Axis(2), mel_index),
        )?;

        let mut session = lock_ignoring_poison(&self.decoder_model);
        let outputs = session.run(inputs.as_slice()).map_err(inference_error)?;
        let (shape, data) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(inference_error)?;
        let waveform = ArrayView::from_shape(shape.to_ixdyn(), data).map_err(|e| {
            DengjenError::with_message(format!("Invalid model audio output shape: {}", e))
        })?;
        trim_chunk_padding(waveform, audio_index)
    }
}

fn trim_chunk_padding(
    waveform: ArrayView<f32, Dim<IxDynImpl>>,
    audio_index: ndarray::Slice,
) -> DengjenResult<AudioSamples> {
    let mut samples: AudioSamples = waveform
        .slice_axis(Axis(2), audio_index)
        .as_slice()
        .ok_or_else(|| DengjenError::with_message("Invalid model audio output"))?
        .to_vec()
        .into();
    samples.crossfade(CHUNK_CROSSFADE_SAMPLES);
    Ok(samples)
}

impl Iterator for SpeechStreamer {
    type Item = DengjenResult<AudioSamples>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.cancel_token.is_cancelled() {
            return None;
        }
        let (mel_index, audio_index) = self.mel_chunker.next()?;
        if self.one_shot {
            self.mel_chunker.consume();
            return Some(self.encoder_outputs.infer_decoder(&self.decoder_model));
        }
        Some(self.synthesize_chunk(mel_index, audio_index))
    }
}

struct AdaptiveMelChunker {
    num_frames: isize,
    base_chunk_size: usize,
    padding: isize,
    hop_length: isize,

    cursor: Option<isize>,
    chunks_emitted: usize,
}

impl AdaptiveMelChunker {
    fn new(num_frames: isize, chunk_size: isize, chunk_padding: isize, hop_length: isize) -> Self {
        Self {
            num_frames,
            base_chunk_size: chunk_size as usize,
            padding: chunk_padding,
            hop_length,
            cursor: Some(0),
            chunks_emitted: 0,
        }
    }

    fn consume(&mut self) {
        self.cursor = None;
    }

    fn current_chunk_width(&self) -> isize {
        self.base_chunk_size
            .max(1)
            .saturating_mul(self.chunks_emitted + 1)
            .min(MAX_CHUNK_SIZE) as isize
    }
}

impl Iterator for AdaptiveMelChunker {
    type Item = (ndarray::Slice, ndarray::Slice);

    fn next(&mut self) -> Option<Self::Item> {
        let region_start = self.cursor?;

        let (mel_start, lead_trim) = if region_start == 0 {
            (0, 0)
        } else {
            (region_start - self.padding * 2, self.padding)
        };

        let padded_end = region_start + self.current_chunk_width() + self.padding;

        let is_final = self.num_frames - padded_end <= MIN_CHUNK_SIZE;

        let (mel_end, tail_trim) = if is_final {
            (None, None)
        } else {
            (Some(padded_end), Some(-self.padding))
        };

        self.chunks_emitted += 1;
        self.cursor = mel_end;

        Some((
            ndarray::Slice::new(mel_start, mel_end, 1),
            ndarray::Slice::new(
                lead_trim * self.hop_length,
                tail_trim.map(|frames| frames * self.hop_length),
                1,
            ),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::IxDyn;

    fn encoder_outputs(frames: usize) -> EncoderOutputs {
        let filled = |channels: usize| {
            Array::from_shape_vec(
                IxDyn(&[1, channels, frames]),
                (0..channels * frames).map(|i| i as f32).collect(),
            )
            .unwrap()
        };
        EncoderOutputs {
            z: filled(192),
            y_mask: filled(1),
            p_duration: None,
            g: Array1::<f32>::from_iter([0.5, 0.25]).into_dyn(),
        }
    }

    #[test]
    fn decoder_inputs_accepts_mel_chunk_sliced_along_the_frame_axis() {
        let outputs = encoder_outputs(100);
        let slice = ndarray::Slice::new(10, Some(60), 1);
        let z = outputs.z.view();
        let y_mask = outputs.y_mask.view();

        let inputs = outputs.decoder_inputs(
            z.slice_axis(Axis(2), slice),
            y_mask.slice_axis(Axis(2), slice),
        );

        assert_eq!(inputs.unwrap().len(), 3);
    }

    #[test]
    fn adaptive_mel_chunker_scales_audio_index_by_default_hop_length() {
        let mut chunker = AdaptiveMelChunker::new(5000, 100, 10, 256);
        let _first = chunker.next().unwrap();
        let (_, audio_index) = chunker.next().unwrap();
        assert_eq!(audio_index.start, 10 * 256);
        assert_eq!(audio_index.end, Some(-10 * 256));
    }

    #[test]
    fn adaptive_mel_chunker_scales_audio_index_by_custom_hop_length() {
        let mut chunker = AdaptiveMelChunker::new(5000, 100, 10, 100);
        let _first = chunker.next().unwrap();
        let (_, audio_index) = chunker.next().unwrap();
        assert_eq!(audio_index.start, 10 * 100);
        assert_eq!(audio_index.end, Some(-10 * 100));
    }

    #[test]
    fn adaptive_mel_chunker_clamps_chunk_width_at_max() {
        let mut chunker = AdaptiveMelChunker::new(100_000, 600, 10, 256);

        let (first_mel, _) = chunker.next().unwrap();
        assert_eq!(first_mel.end, Some(610));

        let (second_mel, _) = chunker.next().unwrap();

        assert_eq!(second_mel.end, Some(610 + MAX_CHUNK_SIZE as isize + 10));
        assert_eq!(chunker.current_chunk_width(), MAX_CHUNK_SIZE as isize);
    }

    #[test]
    fn adaptive_mel_chunker_terminates_when_remaining_frames_fall_below_minimum() {
        let mut chunker = AdaptiveMelChunker::new(50, 10, 5, 10);
        let (mel_index, audio_index) = chunker.next().unwrap();
        assert_eq!(mel_index.end, None);
        assert_eq!(audio_index.end, None);
        assert!(chunker.next().is_none());
    }

    #[test]
    fn adaptive_mel_chunker_terminates_when_chunk_size_and_padding_are_both_zero() {
        let chunker = AdaptiveMelChunker::new(1000, 0, 0, 10);
        let chunks: Vec<_> = chunker.take(1000).collect();
        assert!(
            chunks.len() < 1000,
            "iterator did not terminate within 1000 chunks"
        );
        assert_eq!(chunks.last().unwrap().0.end, None);
    }
}
