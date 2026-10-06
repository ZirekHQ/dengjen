use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::path::Path;
use std::sync::Arc;

use flume::{Receiver, SendError, Sender};
use once_cell::sync::Lazy;
use rayon::{prelude::*, ThreadPool, ThreadPoolBuilder};

mod utils;
pub use dengjen_tts_core::*;

pub fn detect_model_type(config_path: &Path) -> DengjenResult<String> {
    let raw = std::fs::read_to_string(config_path).map_err(|why| {
        DengjenError::FailedToLoadResource(format!(
            "Failed to read model config: `{}`. Caused by: `{}`",
            config_path.display(),
            why
        ))
    })?;
    let parsed: serde_json::Value = serde_json::from_str(&raw).map_err(|why| {
        DengjenError::FailedToLoadResource(format!(
            "Failed to parse model config from file: `{}`. Caused by: `{}`",
            config_path.display(),
            why
        ))
    })?;
    let model_type = parsed
        .get("model_type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("piper");
    Ok(model_type.to_owned())
}

struct ParamRange {
    min: f32,
    max: f32,
}

const SPEED_PARAM_RANGE: ParamRange = ParamRange { min: 0.5, max: 5.5 };
const VOLUME_PARAM_RANGE: ParamRange = ParamRange { min: 0.0, max: 1.0 };
const PITCH_PARAM_RANGE: ParamRange = ParamRange { min: 0.5, max: 1.5 };

/// Amplitude, as a fraction of full scale, below which a vocoder's trailing
/// output is treated as the model's own baked-in silence rather than speech.
const TRAILING_SILENCE_THRESHOLD: f32 = 0.015;
const TAIL_FADE_MS: usize = 5;

pub static SYNTHESIS_THREAD_POOL: Lazy<ThreadPool> = Lazy::new(|| {
    let core_count = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(4);
    ThreadPoolBuilder::new()
        .num_threads(core_count * 4)
        .thread_name(|index| format!("dengjen_synth_{index}"))
        .build()
        .expect("thread pool construction only fails on invalid config, never at runtime")
});

/// Resolves an unset `batch_size` to a sane default. Capped well below the
/// core count because `Mutex<Session>` already serializes the actual
/// inference call regardless of how many callers are ready to make it —
/// spawning more than a handful of queued callers buys nothing.
pub fn default_batch_size() -> NonZeroUsize {
    const MAX_DEFAULT_BATCH_SIZE: NonZeroUsize = NonZeroUsize::new(4).unwrap();
    let cores = std::thread::available_parallelism().unwrap_or(NonZeroUsize::MIN);
    cores.min(MAX_DEFAULT_BATCH_SIZE)
}

#[derive(Clone)]
pub struct AudioOutputConfig {
    pub rate: Option<u8>,
    pub volume: Option<u8>,
    pub pitch: Option<u8>,
    pub appended_silence_ms: Option<u32>,
}

impl AudioOutputConfig {
    fn apply(&self, mut audio: Audio) -> DengjenAudioResult {
        let samples = audio.samples.take();
        let processed = self.apply_to_raw_samples(
            samples.into(),
            audio.info.sample_rate,
            audio.info.num_channels,
        )?;
        audio.samples.merge(processed);
        if let Some(silence_ms) = self.appended_silence_ms {
            let silence = self.generate_silence(
                silence_ms as usize,
                audio.info.sample_rate,
                audio.info.num_channels,
            )?;
            audio.samples.merge(silence);
        }
        Ok(audio)
    }

    fn apply_to_raw_samples(
        &self,
        samples: AudioSamples,
        sample_rate: usize,
        num_channels: usize,
    ) -> DengjenResult<AudioSamples> {
        if samples.is_empty() {
            return Ok(samples);
        }
        let input = samples.into_vec();

        // SAFETY: every libsonic call below targets the `stream` handle this
        // block itself creates, and every exit from the block — the early
        // error return on a zero-sample result included — destroys that
        // same handle first, so no stream ever outlives this block.
        unsafe {
            let stream =
                dengjen_sonic_sys::sonicCreateStream(sample_rate as i32, num_channels as i32);

            if let Some(pct) = self.rate {
                let speed =
                    utils::percent_to_param(pct, SPEED_PARAM_RANGE.min, SPEED_PARAM_RANGE.max);
                dengjen_sonic_sys::sonicSetSpeed(stream, speed);
            }

            if let Some(pct) = self.volume.filter(|&pct| pct != 0) {
                let volume =
                    utils::percent_to_param(pct, VOLUME_PARAM_RANGE.min, VOLUME_PARAM_RANGE.max);
                dengjen_sonic_sys::sonicSetVolume(stream, volume);
            }
            if let Some(pct) = self.pitch {
                let pitch =
                    utils::percent_to_param(pct, PITCH_PARAM_RANGE.min, PITCH_PARAM_RANGE.max);
                dengjen_sonic_sys::sonicSetPitch(stream, pitch);
            }

            let frame_count = input.len() / num_channels;
            dengjen_sonic_sys::sonicWriteFloatToStream(stream, input.as_ptr(), frame_count as i32);
            dengjen_sonic_sys::sonicFlushStream(stream);

            let available_frames = dengjen_sonic_sys::sonicSamplesAvailable(stream);
            if available_frames <= 0 {
                dengjen_sonic_sys::sonicDestroyStream(stream);
                return Err(DengjenError::OperationError(
                    "Sonic Error: failed to apply audio config. Invalid parameter value for rate, volume, or pitch".to_string(),
                ));
            }

            let mut output: Vec<f32> = Vec::with_capacity(available_frames as usize * num_channels);

            let frames_read = dengjen_sonic_sys::sonicReadFloatFromStream(
                stream,
                output.spare_capacity_mut().as_mut_ptr().cast(),
                available_frames,
            );
            output.set_len(frames_read.max(0) as usize * num_channels);

            dengjen_sonic_sys::sonicDestroyStream(stream);

            if self.volume == Some(0) {
                output.fill(0.0);
            }

            Ok(output.into())
        }
    }

    fn generate_silence(
        &self,
        time_ms: usize,
        sample_rate: usize,
        num_channels: usize,
    ) -> DengjenResult<AudioSamples> {
        let sample_count = (time_ms * sample_rate * num_channels) / 1000;
        let silence = vec![0f32; sample_count];
        self.apply_to_raw_samples(silence.into(), sample_rate, num_channels)
    }
}

#[non_exhaustive]
pub enum StreamMode {
    Lazy,
    Parallel,
    Realtime {
        chunk_size: usize,
        chunk_padding: usize,
        cancel_token: CancellationToken,
    },
    Batched {
        batch_size: NonZeroUsize,
    },
}

#[non_exhaustive]
pub enum AudioChunkStream {
    Lazy(DengjenSpeechStreamLazy),
    Parallel(DengjenSpeechStreamParallel),
    Realtime(RealtimeSpeechStream),
    Batched(DengjenSpeechStreamBatched),
}

impl Iterator for AudioChunkStream {
    type Item = DengjenResult<AudioSamples>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Lazy(stream) => stream.next().map(|res| res.map(|audio| audio.samples)),
            Self::Parallel(stream) => stream.next().map(|res| res.map(|audio| audio.samples)),
            Self::Batched(stream) => stream.next().map(|res| res.map(|audio| audio.samples)),
            Self::Realtime(stream) => stream.next(),
        }
    }
}

pub struct DengjenSpeechSynthesizer {
    backend: Arc<dyn DengjenModel + Sync + Send>,
}

impl DengjenSpeechSynthesizer {
    pub fn new(model: Arc<dyn DengjenModel + Sync + Send>) -> DengjenResult<Self> {
        Ok(Self { backend: model })
    }

    #[inline(always)]
    pub fn clone_model(&self) -> Arc<dyn DengjenModel + Send + Sync> {
        Arc::clone(&self.backend)
    }

    fn task_provider(
        &self,
        text: String,
        output_config: Option<AudioOutputConfig>,
    ) -> SpeechSynthesisTaskProvider {
        SpeechSynthesisTaskProvider {
            model: self.clone_model(),
            text,
            output_config,
        }
    }

    pub fn synthesize_lazy(
        &self,
        text: String,
        output_config: Option<AudioOutputConfig>,
    ) -> DengjenResult<DengjenSpeechStreamLazy> {
        DengjenSpeechStreamLazy::new(self.task_provider(text, output_config))
    }

    pub fn synthesize_parallel(
        &self,
        text: String,
        output_config: Option<AudioOutputConfig>,
    ) -> DengjenResult<DengjenSpeechStreamParallel> {
        DengjenSpeechStreamParallel::new(self.task_provider(text, output_config))
    }

    pub fn synthesize_batched(
        &self,
        text: String,
        output_config: Option<AudioOutputConfig>,
        batch_size: NonZeroUsize,
    ) -> DengjenResult<DengjenSpeechStreamBatched> {
        DengjenSpeechStreamBatched::new(self.task_provider(text, output_config), batch_size)
    }

    pub fn synthesize_streamed(
        &self,
        text: String,
        output_config: Option<AudioOutputConfig>,
        chunk_size: usize,
        chunk_padding: usize,
        cancel_token: CancellationToken,
    ) -> DengjenResult<RealtimeSpeechStream> {
        let info = self.backend.audio_output_info()?;
        RealtimeSpeechStream::new(
            self.task_provider(text, output_config),
            chunk_size,
            chunk_padding,
            info.sample_rate,
            info.num_channels,
            cancel_token,
        )
    }

    pub fn synthesize_samples(
        &self,
        text: String,
        output_config: Option<AudioOutputConfig>,
        mode: StreamMode,
    ) -> DengjenResult<AudioChunkStream> {
        match mode {
            StreamMode::Lazy => Ok(AudioChunkStream::Lazy(
                self.synthesize_lazy(text, output_config)?,
            )),
            StreamMode::Parallel => Ok(AudioChunkStream::Parallel(
                self.synthesize_parallel(text, output_config)?,
            )),
            StreamMode::Batched { batch_size } => Ok(AudioChunkStream::Batched(
                self.synthesize_batched(text, output_config, batch_size)?,
            )),
            StreamMode::Realtime {
                chunk_size,
                chunk_padding,
                cancel_token,
            } => Ok(AudioChunkStream::Realtime(self.synthesize_streamed(
                text,
                output_config,
                chunk_size,
                chunk_padding,
                cancel_token,
            )?)),
        }
    }

    pub fn synthesize_to_file(
        &self,
        filename: &Path,
        text: String,
        output_config: Option<AudioOutputConfig>,
    ) -> DengjenResult<()> {
        let mut collected: Vec<f32> = Vec::new();
        for chunk in self.synthesize_parallel(text, output_config)? {
            collected.extend(chunk?.into_vec());
        }
        if collected.is_empty() {
            return Err(DengjenError::OperationError(
                "No speech data to write".to_string(),
            ));
        }

        let info = self.backend.audio_output_info()?;
        let samples = AudioSamples::from(collected);
        dengjen_audio_ops::write_wave_samples_to_file(
            filename,
            samples.to_i16_vec().iter(),
            info.sample_rate as u32,
            info.num_channels.try_into().unwrap(),
            info.sample_width.try_into().unwrap(),
        )?;
        Ok(())
    }
}

impl DengjenModel for DengjenSpeechSynthesizer {
    fn audio_output_info(&self) -> DengjenResult<AudioInfo> {
        self.backend.audio_output_info()
    }
    fn phonemize_text(&self, text: &str) -> DengjenResult<Phonemes> {
        self.backend.phonemize_text(text)
    }
    fn speak_batch(&self, phoneme_batches: Vec<String>) -> DengjenResult<Vec<Audio>> {
        self.backend.speak_batch(phoneme_batches)
    }
    fn speak_one_sentence(&self, phonemes: String) -> DengjenAudioResult {
        self.backend.speak_one_sentence(phonemes)
    }
    fn get_default_synthesis_config(&self) -> DengjenResult<Option<SynthesisConfig>> {
        self.backend.get_default_synthesis_config()
    }
    fn get_fallback_synthesis_config(&self) -> DengjenResult<Option<SynthesisConfig>> {
        self.backend.get_fallback_synthesis_config()
    }
    fn set_fallback_synthesis_config(
        &self,
        synthesis_config: &SynthesisConfig,
    ) -> DengjenResult<()> {
        self.backend.set_fallback_synthesis_config(synthesis_config)
    }
    fn get_language(&self) -> DengjenResult<Option<String>> {
        self.backend.get_language()
    }
    fn get_speakers(&self) -> DengjenResult<Option<&HashMap<i64, String>>> {
        self.backend.get_speakers()
    }
    fn properties(&self) -> DengjenResult<HashMap<String, String>> {
        self.backend.properties()
    }
    fn supports_streaming_output(&self) -> bool {
        self.backend.supports_streaming_output()
    }
    fn stream_synthesis<'a>(
        &'a self,
        phonemes: String,
        chunk_size: usize,
        chunk_padding: usize,
        cancel_token: CancellationToken,
    ) -> DengjenResult<Box<dyn Iterator<Item = DengjenResult<AudioSamples>> + Send + Sync + 'a>>
    {
        self.backend
            .stream_synthesis(phonemes, chunk_size, chunk_padding, cancel_token)
    }
}

/// Groups everything a stream constructor needs to turn input text into
/// audio — the model to synthesize with, the text itself, and the optional
/// post-processing config — so callers pass one value instead of three.
struct SpeechSynthesisTaskProvider {
    model: Arc<dyn DengjenModel + Sync + Send>,
    text: String,
    output_config: Option<AudioOutputConfig>,
}

impl SpeechSynthesisTaskProvider {
    fn get_phonemes(&self) -> DengjenResult<Vec<String>> {
        Ok(self.model.phonemize_text(&self.text)?.to_vec())
    }

    fn shape_output(&self, mut audio: Audio) -> DengjenAudioResult {
        let num_channels = audio.info.num_channels;
        audio
            .samples
            .trim_trailing_silence(TRAILING_SILENCE_THRESHOLD, num_channels);
        audio
            .samples
            .fade_out(audio.info.sample_rate * TAIL_FADE_MS / 1000 * num_channels);
        match &self.output_config {
            Some(config) => config.apply(audio),
            None => Ok(audio),
        }
    }

    fn process_one_sentence(&self, sentence: String) -> DengjenAudioResult {
        let audio = self.model.speak_one_sentence(sentence)?;
        self.shape_output(audio)
    }

    #[allow(dead_code)]
    fn process_batches(&self, sentences: Vec<String>) -> DengjenResult<Vec<Audio>> {
        self.model
            .speak_batch(sentences)?
            .into_iter()
            .map(|audio| self.shape_output(audio))
            .collect()
    }
}

/// Pulls one sentence's audio out of the underlying model on each
/// `Iterator::next` call, rather than synthesizing everything up front.
pub struct DengjenSpeechStreamLazy {
    provider: SpeechSynthesisTaskProvider,
    pending_sentences: std::vec::IntoIter<String>,
    next_chunk_index: usize,
    span: tracing::Span,
}

impl DengjenSpeechStreamLazy {
    fn new(provider: SpeechSynthesisTaskProvider) -> DengjenResult<Self> {
        let span = tracing::info_span!("synthesis_request", mode = "lazy");
        let pending_sentences = span.in_scope(|| provider.get_phonemes())?.into_iter();
        Ok(Self {
            provider,
            pending_sentences,
            next_chunk_index: 0,
            span,
        })
    }
}

impl Iterator for DengjenSpeechStreamLazy {
    type Item = DengjenAudioResult;

    fn next(&mut self) -> Option<Self::Item> {
        let sentence = self.pending_sentences.next()?;
        let chunk_index = self.next_chunk_index;
        self.next_chunk_index += 1;
        let provider = &self.provider;
        Some(self.span.in_scope(|| {
            let chunk_span = tracing::debug_span!("chunk", chunk_index);
            let _enter = chunk_span.enter();
            let result = provider.process_one_sentence(sentence);
            if let Ok(audio) = &result {
                tracing::debug!(sample_count = audio.len(), "chunk_ready");
            }
            result
        }))
    }
}

#[must_use]
pub struct DengjenSpeechStreamParallel {
    finished: std::vec::IntoIter<DengjenAudioResult>,
    // Kept only to hold the span open for the stream's lifetime; `new()` already
    // performs all synthesis eagerly, so no code path reads it back.
    _span: tracing::Span,
}

impl DengjenSpeechStreamParallel {
    fn new(provider: SpeechSynthesisTaskProvider) -> DengjenResult<Self> {
        let span = tracing::info_span!("synthesis_request", mode = "parallel");
        let sentences = span.in_scope(|| provider.get_phonemes())?;
        let worker_span = span.clone();
        let finished: Vec<DengjenAudioResult> = SYNTHESIS_THREAD_POOL.install(|| {
            sentences
                .into_par_iter()
                .enumerate()
                .map(|(chunk_index, sentence)| {
                    worker_span.in_scope(|| {
                        let chunk_span = tracing::debug_span!("chunk", chunk_index);
                        let _enter = chunk_span.enter();
                        let result = provider.process_one_sentence(sentence);
                        if let Ok(audio) = &result {
                            tracing::debug!(sample_count = audio.len(), "chunk_ready");
                        }
                        result
                    })
                })
                .collect()
        });
        Ok(Self {
            finished: finished.into_iter(),
            _span: span,
        })
    }
}

impl Iterator for DengjenSpeechStreamParallel {
    type Item = DengjenAudioResult;

    fn next(&mut self) -> Option<Self::Item> {
        self.finished.next()
    }
}

// tracing::Span holds a Dispatch over `dyn Subscriber`, which strips the
// auto-derived UnwindSafe/RefUnwindSafe impls even though the span itself
// carries no invariant a caller could observe as broken after a panic.
impl std::panic::UnwindSafe for DengjenSpeechStreamParallel {}
impl std::panic::RefUnwindSafe for DengjenSpeechStreamParallel {}

/// Synthesizes sentences in bounded-size concurrent groups: parallel within
/// a group of `batch_size`, sequential across groups. Unlike
/// `DengjenSpeechStreamParallel`, which computes the whole request eagerly
/// in `new()`, this stream computes lazily — one group at a time, only when
/// `next()` needs more — so building it is cheap and the first group's
/// audio can stream out before later groups are even started.
pub struct DengjenSpeechStreamBatched {
    provider: SpeechSynthesisTaskProvider,
    pending: std::vec::IntoIter<String>,
    batch_size: NonZeroUsize,
    buffered: std::vec::IntoIter<DengjenAudioResult>,
    next_chunk_index: usize,
    span: tracing::Span,
}

impl DengjenSpeechStreamBatched {
    fn new(provider: SpeechSynthesisTaskProvider, batch_size: NonZeroUsize) -> DengjenResult<Self> {
        let span = tracing::info_span!("synthesis_request", mode = "batched");
        let pending = span.in_scope(|| provider.get_phonemes())?.into_iter();
        Ok(Self {
            provider,
            pending,
            batch_size,
            buffered: Vec::new().into_iter(),
            next_chunk_index: 0,
            span,
        })
    }

    fn compute_next_group(&mut self) -> Option<Vec<DengjenAudioResult>> {
        let group: Vec<String> = (&mut self.pending).take(self.batch_size.get()).collect();
        if group.is_empty() {
            return None;
        }
        let provider = &self.provider;
        let start_index = self.next_chunk_index;
        self.next_chunk_index += group.len();
        let span = &self.span;
        Some(SYNTHESIS_THREAD_POOL.install(|| {
            span.in_scope(|| {
                group
                    .into_par_iter()
                    .enumerate()
                    .map(|(offset, sentence)| {
                        let chunk_index = start_index + offset;
                        let chunk_span = tracing::debug_span!("chunk", chunk_index);
                        let _enter = chunk_span.enter();
                        let result = provider.process_one_sentence(sentence);
                        if let Ok(audio) = &result {
                            tracing::debug!(sample_count = audio.len(), "chunk_ready");
                        }
                        result
                    })
                    .collect()
            })
        }))
    }
}

impl Iterator for DengjenSpeechStreamBatched {
    type Item = DengjenAudioResult;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(result) = self.buffered.next() {
            return Some(result);
        }
        let group = self.compute_next_group()?;
        self.buffered = group.into_iter();
        self.buffered.next()
    }
}

// Same rationale as `DengjenSpeechStreamParallel`'s impls above.
impl std::panic::UnwindSafe for DengjenSpeechStreamBatched {}
impl std::panic::RefUnwindSafe for DengjenSpeechStreamBatched {}

const MAX_STREAM_CHUNK_SIZE: usize = 1_000_000;

pub struct RealtimeSpeechStream {
    rx: Receiver<DengjenResult<AudioSamples>>,
    cancel_token: CancellationToken,
    span: tracing::Span,
    next_chunk_index: usize,
}

impl RealtimeSpeechStream {
    fn next_chunk_size(base_chunk_size: usize, sentences_seen: usize) -> usize {
        let extra_multiples = sentences_seen.min(4);
        let growth = base_chunk_size.saturating_mul(extra_multiples);
        base_chunk_size
            .saturating_add(growth)
            .min(MAX_STREAM_CHUNK_SIZE)
    }

    fn new(
        provider: SpeechSynthesisTaskProvider,
        chunk_size: usize,
        chunk_padding: usize,
        sample_rate: usize,
        num_channels: usize,
        cancel_token: CancellationToken,
    ) -> DengjenResult<Self> {
        let span = tracing::info_span!("synthesis_request", mode = "realtime");
        let sentences = span.in_scope(|| provider.get_phonemes())?;
        let (tx, rx) = flume::unbounded();
        let producer_cancel_token = cancel_token.clone();
        let producer_span = span.clone();

        // An unwinding job would abort the whole process, and dropping `tx` on
        // unwind would end the stream as if synthesis had succeeded.
        SYNTHESIS_THREAD_POOL.spawn(move || {
            let _enter = producer_span.enter();
            let cancel_token = producer_cancel_token;
            let produced = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                for (sentence_index, phonemes) in sentences.into_iter().enumerate() {
                    if cancel_token.is_cancelled() {
                        return;
                    }

                    let this_chunk_size = Self::next_chunk_size(chunk_size, sentence_index);
                    let stream = match provider.model.stream_synthesis(
                        phonemes,
                        this_chunk_size,
                        chunk_padding,
                        cancel_token.clone(),
                    ) {
                        Ok(stream) => stream,
                        Err(err) => {
                            let _ = tx.send(Err(err));
                            return;
                        }
                    };

                    let stream_result = Self::process_rt_stream(
                        stream,
                        &tx,
                        provider.output_config.as_ref(),
                        sample_rate,
                        num_channels,
                        &cancel_token,
                    );
                    if stream_result.is_err() {
                        return;
                    }
                }
            }));
            if let Err(payload) = produced {
                let message = panic_message(payload.as_ref());
                tracing::error!(%message, "synthesis worker panicked");
                let _ = tx.send(Err(DengjenError::InferenceError(format!(
                    "synthesis worker panicked: {message}"
                ))));
            }
        });

        Ok(Self {
            rx,
            cancel_token,
            span,
            next_chunk_index: 0,
        })
    }

    fn process_rt_stream(
        stream: AudioStreamIterator,
        tx: &Sender<DengjenResult<AudioSamples>>,
        output_config: Option<&AudioOutputConfig>,
        sample_rate: usize,
        num_channels: usize,
        cancel_token: &CancellationToken,
    ) -> Result<(), SendError<DengjenResult<AudioSamples>>> {
        for chunk in stream {
            if cancel_token.is_cancelled() {
                return Ok(());
            }
            let shaped = match (chunk, output_config) {
                (Ok(samples), Some(config)) => {
                    config.apply_to_raw_samples(samples, sample_rate, num_channels)
                }
                (Ok(samples), None) => Ok(samples),
                (Err(err), _) => Err(err),
            };
            tx.send(shaped)?;
        }

        if cancel_token.is_cancelled() {
            return Ok(());
        }
        let Some(config) = output_config else {
            return Ok(());
        };
        let Some(silence_ms) = config.appended_silence_ms else {
            return Ok(());
        };
        let silence = config.generate_silence(silence_ms as usize, sample_rate, num_channels);
        tx.send(silence)
    }
}

impl Iterator for RealtimeSpeechStream {
    type Item = DengjenResult<AudioSamples>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.cancel_token.is_cancelled() {
            return None;
        }
        let result = self.rx.recv().ok()?;
        let chunk_index = self.next_chunk_index;
        self.next_chunk_index += 1;
        self.span.in_scope(|| {
            let chunk_span = tracing::debug_span!("chunk", chunk_index);
            let _enter = chunk_span.enter();
            if let Ok(samples) = &result {
                tracing::debug!(sample_count = samples.len(), "chunk_ready");
            }
        });
        Some(result)
    }
}

// See the same impls on `DengjenSpeechStreamParallel` above for why this is semver-safe.
impl std::panic::UnwindSafe for RealtimeSpeechStream {}
impl std::panic::RefUnwindSafe for RealtimeSpeechStream {}

#[cfg(test)]
mod chunk_size_growth_tests {
    use super::*;

    #[test]
    fn first_sentence_uses_base_chunk_size_unmodified() {
        assert_eq!(RealtimeSpeechStream::next_chunk_size(72, 0), 72);
    }

    #[test]
    fn ramps_additively_up_to_the_cap() {
        assert_eq!(RealtimeSpeechStream::next_chunk_size(72, 0), 72);
        assert_eq!(RealtimeSpeechStream::next_chunk_size(72, 1), 144);
        assert_eq!(RealtimeSpeechStream::next_chunk_size(72, 2), 216);
        assert_eq!(RealtimeSpeechStream::next_chunk_size(72, 3), 288);
        assert_eq!(RealtimeSpeechStream::next_chunk_size(72, 4), 360);
    }

    #[test]
    fn plateaus_after_the_ramp_cap_instead_of_continuing_to_grow() {
        let at_cap = RealtimeSpeechStream::next_chunk_size(72, 4);
        let well_past_cap = RealtimeSpeechStream::next_chunk_size(72, 50);
        assert_eq!(
            at_cap, well_past_cap,
            "growth must stop increasing once sentences_seen exceeds the ramp cap"
        );
    }

    #[test]
    fn result_is_clamped_to_max_chunk_size() {
        assert_eq!(
            RealtimeSpeechStream::next_chunk_size(300_000, 4),
            MAX_STREAM_CHUNK_SIZE,
            "300_000 * 5 = 1_500_000 must be clamped down to MAX_STREAM_CHUNK_SIZE"
        );
    }

    #[test]
    fn never_overflows_or_panics_on_pathological_inputs() {
        let result = RealtimeSpeechStream::next_chunk_size(usize::MAX, usize::MAX);
        assert_eq!(result, MAX_STREAM_CHUNK_SIZE);
    }

    #[test]
    fn growth_never_decreases_across_a_multi_sentence_stream_issue_28_regression() {
        let sizes: Vec<usize> = (0..20)
            .map(|sentences_seen| RealtimeSpeechStream::next_chunk_size(72, sentences_seen))
            .collect();
        for window in sizes.windows(2) {
            assert!(
                window[1] >= window[0],
                "chunk_size must never decrease between sentences, got {sizes:?}"
            );
        }
    }
}

#[cfg(test)]
mod cancellation_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    struct CountingStreamModel {
        chunks_per_sentence: usize,
        sentences: usize,
        produced: Arc<AtomicUsize>,
        delay: Option<std::time::Duration>,
        chunk_sizes_seen: Arc<Mutex<Vec<usize>>>,
    }

    impl DengjenModel for CountingStreamModel {
        fn audio_output_info(&self) -> DengjenResult<AudioInfo> {
            Ok(AudioInfo {
                sample_rate: 16000,
                num_channels: 1,
                sample_width: 2,
            })
        }
        fn phonemize_text(&self, _text: &str) -> DengjenResult<Phonemes> {
            Ok(Phonemes::from(vec!["sentence".to_string(); self.sentences]))
        }
        fn speak_batch(&self, _phoneme_batches: Vec<String>) -> DengjenResult<Vec<Audio>> {
            Ok(Vec::new())
        }
        fn speak_one_sentence(&self, _phonemes: String) -> DengjenAudioResult {
            Err(DengjenError::OperationError(
                "not used by this test".to_string(),
            ))
        }
        fn get_default_synthesis_config(&self) -> DengjenResult<Option<SynthesisConfig>> {
            Ok(None)
        }
        fn get_fallback_synthesis_config(&self) -> DengjenResult<Option<SynthesisConfig>> {
            Ok(None)
        }
        fn set_fallback_synthesis_config(&self, _c: &SynthesisConfig) -> DengjenResult<()> {
            Ok(())
        }
        fn supports_streaming_output(&self) -> bool {
            true
        }
        fn stream_synthesis(
            &self,
            _phonemes: String,
            chunk_size: usize,
            _chunk_padding: usize,
            cancel_token: CancellationToken,
        ) -> DengjenResult<AudioStreamIterator<'_>> {
            self.chunk_sizes_seen.lock().unwrap().push(chunk_size);
            let produced = Arc::clone(&self.produced);
            let n = self.chunks_per_sentence;
            let delay = self.delay;
            let iter = (0..n).map_while(move |_| {
                if let Some(delay) = delay {
                    std::thread::sleep(delay);
                }
                if cancel_token.is_cancelled() {
                    None
                } else {
                    produced.fetch_add(1, Ordering::SeqCst);
                    Some(Ok(AudioSamples::from(vec![0.0f32; 4])))
                }
            });
            Ok(Box::new(iter))
        }
    }

    #[test]
    fn cancelling_mid_stream_stops_further_chunks() {
        let produced = Arc::new(AtomicUsize::new(0));
        let model: Arc<dyn DengjenModel + Send + Sync> = Arc::new(CountingStreamModel {
            chunks_per_sentence: 1000,
            sentences: 5,
            produced: Arc::clone(&produced),
            delay: Some(std::time::Duration::from_millis(1)),
            chunk_sizes_seen: Arc::new(Mutex::new(Vec::new())),
        });
        let synth = DengjenSpeechSynthesizer::new(model).unwrap();
        let cancel_token = CancellationToken::new();
        let stream = synth
            .synthesize_streamed("irrelevant".to_string(), None, 10, 0, cancel_token.clone())
            .unwrap();

        let mut received = 0;
        for result in stream {
            let _ = result.unwrap();
            received += 1;
            if received == 3 {
                cancel_token.cancel();
            }
        }

        assert!(
            received < 100,
            "expected cancellation to truncate the stream promptly, got {received} chunks"
        );
        assert!(
            produced.load(Ordering::SeqCst) < 100,
            "expected production to stop promptly, produced {} chunks",
            produced.load(Ordering::SeqCst)
        );
    }

    #[test]
    fn cancelling_stops_delivery_of_already_buffered_chunks() {
        // The producer runs ahead into an unbounded channel; without a consumer-side
        // cancellation check the iterator would keep draining everything already buffered.
        let produced = Arc::new(AtomicUsize::new(0));
        let model: Arc<dyn DengjenModel + Send + Sync> = Arc::new(CountingStreamModel {
            chunks_per_sentence: 500,
            sentences: 3,
            produced: Arc::clone(&produced),
            delay: None,
            chunk_sizes_seen: Arc::new(Mutex::new(Vec::new())),
        });
        let synth = DengjenSpeechSynthesizer::new(model).unwrap();
        let cancel_token = CancellationToken::new();
        let mut stream = synth
            .synthesize_streamed("irrelevant".to_string(), None, 10, 0, cancel_token.clone())
            .unwrap();

        let mut received = 0;
        while received < 3 {
            let _ = stream
                .next()
                .expect("stream ended before 3 chunks")
                .unwrap();
            received += 1;
        }

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while produced.load(Ordering::SeqCst) < 200 && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let buffered_at_cancel = produced.load(Ordering::SeqCst);
        assert!(
            buffered_at_cancel >= 200,
            "test setup failed: producer only made {buffered_at_cancel} chunks, so nothing was buffered"
        );

        cancel_token.cancel();

        assert!(
            stream.next().is_none(),
            "consumer kept delivering buffered chunks after cancellation"
        );
        assert_eq!(received, 3);
    }

    #[test]
    fn chunk_size_growth_stays_bounded_and_never_oscillates_across_many_sentences() {
        let produced = Arc::new(AtomicUsize::new(0));
        let chunk_sizes_seen = Arc::new(Mutex::new(Vec::new()));
        let model: Arc<dyn DengjenModel + Send + Sync> = Arc::new(CountingStreamModel {
            chunks_per_sentence: 300,
            sentences: 20,
            produced: Arc::clone(&produced),
            delay: None,
            chunk_sizes_seen: Arc::clone(&chunk_sizes_seen),
        });
        let synth = DengjenSpeechSynthesizer::new(model).unwrap();
        let cancel_token = CancellationToken::new();
        let stream = synth
            .synthesize_streamed("irrelevant".to_string(), None, 72, 0, cancel_token)
            .unwrap();

        let mut received = 0;
        for result in stream {
            let _ = result.expect("stream must not error under bounded chunk_size growth");
            received += 1;
        }

        assert_eq!(
            received,
            300 * 20,
            "expected every produced chunk to be delivered without the stream aborting"
        );

        let seen = chunk_sizes_seen.lock().unwrap();
        assert_eq!(
            seen.len(),
            20,
            "expected one stream_synthesis call per sentence"
        );
        for &size in seen.iter() {
            assert!(
                size <= MAX_STREAM_CHUNK_SIZE,
                "chunk_size {size} exceeded MAX_STREAM_CHUNK_SIZE, growth is not bounded"
            );
        }
        assert_eq!(
            &seen[0..5],
            &[72, 144, 216, 288, 360],
            "expected the additive ramp for the first 5 sentences"
        );
        for &size in &seen[5..] {
            assert_eq!(
                size, 360,
                "growth must plateau at 5x base after the ramp cap, not continue growing or drop back down"
            );
        }
    }

    #[test]
    fn some_output_config_processes_chunks_and_appends_silence_per_sentence() {
        let chunks_per_sentence = 3;
        let sentences = 2;
        let produced = Arc::new(AtomicUsize::new(0));
        let model: Arc<dyn DengjenModel + Send + Sync> = Arc::new(CountingStreamModel {
            chunks_per_sentence,
            sentences,
            produced: Arc::clone(&produced),
            delay: None,
            chunk_sizes_seen: Arc::new(Mutex::new(Vec::new())),
        });
        let synth = DengjenSpeechSynthesizer::new(model).unwrap();
        let cancel_token = CancellationToken::new();
        let output_config = AudioOutputConfig {
            rate: None,
            volume: None,
            pitch: None,
            appended_silence_ms: Some(1000),
        };
        let stream = synth
            .synthesize_streamed(
                "irrelevant".to_string(),
                Some(output_config),
                10,
                0,
                cancel_token,
            )
            .unwrap();

        let chunks: Vec<AudioSamples> = stream.map(|result| result.unwrap()).collect();

        let group_size = chunks_per_sentence + 1;
        assert_eq!(
            chunks.len(),
            group_size * sentences,
            "expected {chunks_per_sentence} real chunks plus 1 appended-silence chunk per sentence"
        );

        for sentence_index in 0..sentences {
            let silence_position = sentence_index * group_size + chunks_per_sentence;
            let silence_chunk = &chunks[silence_position];
            assert_eq!(
                silence_chunk.len(),
                16000,
                "1000ms of appended silence @ 16000Hz must be 16000 samples"
            );
            let max_abs = silence_chunk
                .as_vec()
                .iter()
                .fold(0f32, |a, &b| a.max(b.abs()));
            assert_eq!(
                max_abs, 0.0,
                "appended silence chunk must contain only zeros"
            );
        }
    }
}

#[cfg(test)]
mod realtime_stream_error_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    struct FailingStreamModel {
        sentences: usize,
        fail_on_call: usize,
        calls: Arc<AtomicUsize>,
    }

    impl DengjenModel for FailingStreamModel {
        fn audio_output_info(&self) -> DengjenResult<AudioInfo> {
            Ok(AudioInfo {
                sample_rate: 16000,
                num_channels: 1,
                sample_width: 2,
            })
        }
        fn phonemize_text(&self, _text: &str) -> DengjenResult<Phonemes> {
            Ok(Phonemes::from(vec!["sentence".to_string(); self.sentences]))
        }
        fn speak_batch(&self, _phoneme_batches: Vec<String>) -> DengjenResult<Vec<Audio>> {
            Ok(Vec::new())
        }
        fn speak_one_sentence(&self, _phonemes: String) -> DengjenAudioResult {
            Err(DengjenError::OperationError(
                "not used by this test".to_string(),
            ))
        }
        fn get_default_synthesis_config(&self) -> DengjenResult<Option<SynthesisConfig>> {
            Ok(None)
        }
        fn get_fallback_synthesis_config(&self) -> DengjenResult<Option<SynthesisConfig>> {
            Ok(None)
        }
        fn set_fallback_synthesis_config(&self, _c: &SynthesisConfig) -> DengjenResult<()> {
            Ok(())
        }
        fn supports_streaming_output(&self) -> bool {
            true
        }
        fn stream_synthesis(
            &self,
            _phonemes: String,
            _chunk_size: usize,
            _chunk_padding: usize,
            _cancel_token: CancellationToken,
        ) -> DengjenResult<AudioStreamIterator<'_>> {
            let call_index = self.calls.fetch_add(1, Ordering::SeqCst);
            if call_index == self.fail_on_call {
                return Err(DengjenError::InferenceError(
                    "synthetic stream_synthesis failure".to_string(),
                ));
            }
            Ok(Box::new(std::iter::once(Ok(AudioSamples::from(vec![
                0.0f32; 4
            ])))))
        }
    }

    #[test]
    fn stream_synthesis_failure_on_a_later_sentence_surfaces_and_stops_further_production() {
        let calls = Arc::new(AtomicUsize::new(0));
        let model: Arc<dyn DengjenModel + Send + Sync> = Arc::new(FailingStreamModel {
            sentences: 3,
            fail_on_call: 1,
            calls: Arc::clone(&calls),
        });
        let synth = DengjenSpeechSynthesizer::new(model).unwrap();
        let cancel_token = CancellationToken::new();
        let stream = synth
            .synthesize_streamed("irrelevant".to_string(), None, 10, 0, cancel_token)
            .unwrap();

        let items: Vec<DengjenResult<AudioSamples>> = stream.collect();

        assert_eq!(
            items.len(),
            2,
            "expected the first sentence's one chunk plus the second sentence's error, nothing after"
        );
        assert!(items[0].is_ok(), "first sentence's chunk should succeed");
        match &items[1] {
            Err(DengjenError::InferenceError(msg)) => {
                assert_eq!(msg, "synthetic stream_synthesis failure");
            }
            other => panic!("expected the injected InferenceError, got {other:?}"),
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "producer must stop after the failing sentence, never reaching the third"
        );
    }

    struct MidStreamErrorModel {
        sentences: usize,
        chunks: Mutex<Option<Vec<DengjenResult<AudioSamples>>>>,
    }

    impl DengjenModel for MidStreamErrorModel {
        fn audio_output_info(&self) -> DengjenResult<AudioInfo> {
            Ok(AudioInfo {
                sample_rate: 16000,
                num_channels: 1,
                sample_width: 2,
            })
        }
        fn phonemize_text(&self, _text: &str) -> DengjenResult<Phonemes> {
            Ok(Phonemes::from(vec!["sentence".to_string(); self.sentences]))
        }
        fn speak_batch(&self, _phoneme_batches: Vec<String>) -> DengjenResult<Vec<Audio>> {
            Ok(Vec::new())
        }
        fn speak_one_sentence(&self, _phonemes: String) -> DengjenAudioResult {
            Err(DengjenError::OperationError(
                "not used by this test".to_string(),
            ))
        }
        fn get_default_synthesis_config(&self) -> DengjenResult<Option<SynthesisConfig>> {
            Ok(None)
        }
        fn get_fallback_synthesis_config(&self) -> DengjenResult<Option<SynthesisConfig>> {
            Ok(None)
        }
        fn set_fallback_synthesis_config(&self, _c: &SynthesisConfig) -> DengjenResult<()> {
            Ok(())
        }
        fn supports_streaming_output(&self) -> bool {
            true
        }
        fn stream_synthesis(
            &self,
            _phonemes: String,
            _chunk_size: usize,
            _chunk_padding: usize,
            _cancel_token: CancellationToken,
        ) -> DengjenResult<AudioStreamIterator<'_>> {
            let chunks = self
                .chunks
                .lock()
                .unwrap()
                .take()
                .expect("stream_synthesis called more than once in this test");
            Ok(Box::new(chunks.into_iter()))
        }
    }

    struct PanickingStreamModel;

    impl DengjenModel for PanickingStreamModel {
        fn audio_output_info(&self) -> DengjenResult<AudioInfo> {
            Ok(AudioInfo {
                sample_rate: 16000,
                num_channels: 1,
                sample_width: 2,
            })
        }
        fn phonemize_text(&self, _text: &str) -> DengjenResult<Phonemes> {
            Ok(Phonemes::from(vec!["sentence".to_string()]))
        }
        fn speak_batch(&self, _phoneme_batches: Vec<String>) -> DengjenResult<Vec<Audio>> {
            Ok(Vec::new())
        }
        fn speak_one_sentence(&self, _phonemes: String) -> DengjenAudioResult {
            Err(DengjenError::OperationError(
                "not used by this test".to_string(),
            ))
        }
        fn get_default_synthesis_config(&self) -> DengjenResult<Option<SynthesisConfig>> {
            Ok(None)
        }
        fn get_fallback_synthesis_config(&self) -> DengjenResult<Option<SynthesisConfig>> {
            Ok(None)
        }
        fn set_fallback_synthesis_config(&self, _c: &SynthesisConfig) -> DengjenResult<()> {
            Ok(())
        }
        fn supports_streaming_output(&self) -> bool {
            true
        }
        fn stream_synthesis(
            &self,
            _phonemes: String,
            _chunk_size: usize,
            _chunk_padding: usize,
            _cancel_token: CancellationToken,
        ) -> DengjenResult<AudioStreamIterator<'_>> {
            panic!("synthetic stream_synthesis panic")
        }
    }

    fn collect_stream(
        model: Arc<dyn DengjenModel + Send + Sync>,
    ) -> Vec<DengjenResult<AudioSamples>> {
        DengjenSpeechSynthesizer::new(model)
            .unwrap()
            .synthesize_streamed(
                "irrelevant".to_string(),
                None,
                10,
                0,
                CancellationToken::new(),
            )
            .unwrap()
            .collect()
    }

    #[test]
    fn worker_panic_surfaces_as_an_error_and_the_pool_keeps_serving() {
        let items = collect_stream(Arc::new(PanickingStreamModel));

        assert_eq!(
            items.len(),
            1,
            "the panic must yield exactly one error item"
        );
        match &items[0] {
            Err(DengjenError::InferenceError(msg)) => {
                assert!(
                    msg.contains("synthetic stream_synthesis panic"),
                    "got {msg:?}"
                );
            }
            other => panic!("expected an InferenceError carrying the panic message, got {other:?}"),
        }

        let healthy: Arc<dyn DengjenModel + Send + Sync> = Arc::new(FailingStreamModel {
            sentences: 1,
            fail_on_call: usize::MAX,
            calls: Arc::new(AtomicUsize::new(0)),
        });
        let follow_up = collect_stream(healthy);
        assert_eq!(follow_up.len(), 1);
        assert!(follow_up[0].is_ok(), "a request after a panic must succeed");
    }

    #[test]
    fn mid_stream_chunk_error_is_forwarded_to_the_consumer_not_dropped() {
        let chunks = vec![
            Ok(AudioSamples::from(vec![0.0f32; 4])),
            Err(DengjenError::InferenceError(
                "synthetic mid-stream failure".to_string(),
            )),
            Ok(AudioSamples::from(vec![0.0f32; 4])),
        ];
        let model: Arc<dyn DengjenModel + Send + Sync> = Arc::new(MidStreamErrorModel {
            sentences: 1,
            chunks: Mutex::new(Some(chunks)),
        });
        let synth = DengjenSpeechSynthesizer::new(model).unwrap();
        let cancel_token = CancellationToken::new();
        let stream = synth
            .synthesize_streamed("irrelevant".to_string(), None, 10, 0, cancel_token)
            .unwrap();

        let items: Vec<DengjenResult<AudioSamples>> = stream.collect();

        assert_eq!(
            items.len(),
            3,
            "expected all 3 chunks including the mid-stream error, not truncated"
        );
        assert!(items[0].is_ok());
        match &items[1] {
            Err(DengjenError::InferenceError(msg)) => {
                assert_eq!(msg, "synthetic mid-stream failure");
            }
            other => panic!("expected the injected InferenceError, got {other:?}"),
        }
        assert!(
            items[2].is_ok(),
            "chunks after a mid-stream error must still be forwarded, not dropped"
        );
    }
}

#[cfg(test)]
mod audio_output_config_tests {
    use super::*;

    fn sine_samples(n: usize) -> Vec<f32> {
        (0..n).map(|i| (i as f32 * 0.01).sin() * 0.5).collect()
    }

    #[test]
    fn apply_to_raw_samples_on_empty_input_is_a_noop() {
        let config = AudioOutputConfig {
            rate: Some(50),
            volume: Some(50),
            pitch: Some(50),
            appended_silence_ms: None,
        };
        let result = config
            .apply_to_raw_samples(AudioSamples::from(Vec::new()), 16000, 1)
            .unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn apply_to_raw_samples_with_no_config_set_preserves_length_and_signal() {
        let config = AudioOutputConfig {
            rate: None,
            volume: None,
            pitch: None,
            appended_silence_ms: None,
        };
        let input = sine_samples(1000);
        let result = config
            .apply_to_raw_samples(AudioSamples::from(input.clone()), 16000, 1)
            .unwrap();
        assert_eq!(result.len(), input.len());
        let max_diff = result
            .as_vec()
            .iter()
            .zip(input.iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0f32, f32::max);
        assert!(
            max_diff < 0.01,
            "expected near-identity passthrough with no config set, max diff was {max_diff}"
        );
    }

    #[test]
    fn apply_to_raw_samples_with_volume_zero_mutes_the_signal() {
        let config = AudioOutputConfig {
            rate: None,
            volume: Some(0),
            pitch: None,
            appended_silence_ms: None,
        };
        let input = sine_samples(1000);
        let result = config
            .apply_to_raw_samples(AudioSamples::from(input), 16000, 1)
            .unwrap();
        let max_abs = result.as_vec().iter().fold(0f32, |a, &b| a.max(b.abs()));
        assert_eq!(max_abs, 0.0, "volume=0 must mute the signal entirely");
    }

    #[test]
    fn apply_to_raw_samples_with_volume_100_preserves_amplitude() {
        let config = AudioOutputConfig {
            rate: None,
            volume: Some(100),
            pitch: None,
            appended_silence_ms: None,
        };
        let input = sine_samples(1000);
        let result = config
            .apply_to_raw_samples(AudioSamples::from(input), 16000, 1)
            .unwrap();
        let max_abs = result.as_vec().iter().fold(0f32, |a, &b| a.max(b.abs()));
        assert!(
            (max_abs - 0.5).abs() < 0.01,
            "expected peak amplitude close to the input's 0.5, got {max_abs}"
        );
    }

    #[test]
    fn generate_silence_produces_the_expected_sample_count_and_is_silent() {
        let config = AudioOutputConfig {
            rate: None,
            volume: Some(50),
            pitch: None,
            appended_silence_ms: None,
        };
        let silence = config.generate_silence(1000, 16000, 1).unwrap();
        assert_eq!(
            silence.len(),
            16000,
            "1000ms @ 16000Hz must be 16000 samples"
        );
        let max_abs = silence.as_vec().iter().fold(0f32, |a, &b| a.max(b.abs()));
        assert_eq!(max_abs, 0.0, "generated silence must contain only zeros");
    }

    #[test]
    fn generate_silence_scales_sample_count_by_channel_count() {
        let config = AudioOutputConfig {
            rate: None,
            volume: None,
            pitch: None,
            appended_silence_ms: None,
        };
        let silence = config.generate_silence(1000, 16000, 2).unwrap();
        assert_eq!(
            silence.len(),
            32000,
            "1000ms @ 16000Hz stereo must be 16000 frames * 2 channels = 32000 samples"
        );
    }

    #[test]
    fn apply_appends_generated_silence_when_configured() {
        let config = AudioOutputConfig {
            rate: None,
            volume: None,
            pitch: None,
            appended_silence_ms: Some(500),
        };
        let samples = AudioSamples::from(sine_samples(100));
        let audio = Audio::new(samples, 16000, None);
        let original_len = audio.len();
        let result = config.apply(audio).unwrap();
        assert_eq!(
            result.len(),
            original_len + 8000,
            "500ms @ 16000Hz of silence (8000 samples) must be appended"
        );
    }

    #[test]
    fn apply_scales_appended_silence_by_rate_exactly_once() {
        let config = AudioOutputConfig {
            rate: Some(100),
            volume: None,
            pitch: None,
            appended_silence_ms: Some(500),
        };
        let raw_samples = sine_samples(100);

        let expected_silence_len = config.generate_silence(500, 16000, 1).unwrap().len();
        let expected_audio_len = config
            .apply_to_raw_samples(AudioSamples::from(raw_samples.clone()), 16000, 1)
            .unwrap()
            .len();

        let audio = Audio::new(AudioSamples::from(raw_samples), 16000, None);
        let result = config.apply(audio).unwrap();

        assert_eq!(result.len(), expected_audio_len + expected_silence_len);
    }
}

#[cfg(test)]
mod stream_mode_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CannedSentenceModel {
        sentences: Vec<&'static str>,
        fail_on: Option<&'static str>,
    }

    impl DengjenModel for CannedSentenceModel {
        fn audio_output_info(&self) -> DengjenResult<AudioInfo> {
            Ok(AudioInfo {
                sample_rate: 16000,
                num_channels: 1,
                sample_width: 2,
            })
        }
        fn phonemize_text(&self, _text: &str) -> DengjenResult<Phonemes> {
            Ok(Phonemes::from(
                self.sentences
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>(),
            ))
        }
        fn speak_batch(&self, phoneme_batches: Vec<String>) -> DengjenResult<Vec<Audio>> {
            phoneme_batches
                .into_iter()
                .map(|ph| self.speak_one_sentence(ph))
                .collect()
        }
        fn speak_one_sentence(&self, phonemes: String) -> DengjenAudioResult {
            if self.fail_on == Some(phonemes.as_str()) {
                return Err(DengjenError::OperationError(format!(
                    "synthesis failed for {phonemes}"
                )));
            }
            let n = phonemes.len();
            let samples = AudioSamples::from(vec![n as f32; n]);
            Ok(Audio::new(samples, 16000, None))
        }
        fn get_default_synthesis_config(&self) -> DengjenResult<Option<SynthesisConfig>> {
            Ok(None)
        }
        fn get_fallback_synthesis_config(&self) -> DengjenResult<Option<SynthesisConfig>> {
            Ok(None)
        }
        fn set_fallback_synthesis_config(&self, _c: &SynthesisConfig) -> DengjenResult<()> {
            Ok(())
        }
    }

    #[test]
    fn shape_output_ramps_a_trimmed_tail_down_to_silence() {
        let provider = SpeechSynthesisTaskProvider {
            model: Arc::new(CannedSentenceModel {
                sentences: vec![],
                fail_on: None,
            }),
            text: String::new(),
            output_config: None,
        };
        let decay: Vec<f32> = (0..4000).map(|i| 0.5 * (1.0 - i as f32 / 4000.0)).collect();
        let shaped = provider
            .shape_output(Audio::new(AudioSamples::from(decay), 16000, None))
            .unwrap()
            .into_vec();
        assert_eq!(shaped.last(), Some(&0.0));
        let seam_step = shaped[shaped.len() - 2].abs();
        assert!(seam_step < 0.001, "tail still ends in a step: {seam_step}");
    }

    #[test]
    fn batched_stream_yields_one_result_per_sentence_in_order() {
        let model: Arc<dyn DengjenModel + Send + Sync> = Arc::new(CannedSentenceModel {
            sentences: vec!["a", "bb", "ccc", "dddd", "e"],
            fail_on: None,
        });
        let synth = DengjenSpeechSynthesizer::new(model).unwrap();
        let results: Vec<_> = synth
            .synthesize_batched(
                "irrelevant".to_string(),
                None,
                std::num::NonZeroUsize::new(2).unwrap(),
            )
            .unwrap()
            .collect();
        let lens: Vec<usize> = results.into_iter().map(|r| r.unwrap().len()).collect();
        assert_eq!(
            lens,
            vec![1, 2, 3, 4, 1],
            "batched stream must preserve sentence order across group boundaries"
        );
    }

    #[test]
    fn batched_stream_with_batch_size_covering_every_sentence_behaves_like_parallel() {
        let model: Arc<dyn DengjenModel + Send + Sync> = Arc::new(CannedSentenceModel {
            sentences: vec!["a", "bb", "ccc"],
            fail_on: None,
        });
        let synth = DengjenSpeechSynthesizer::new(model).unwrap();
        let results: Vec<_> = synth
            .synthesize_batched(
                "irrelevant".to_string(),
                None,
                std::num::NonZeroUsize::new(10).unwrap(),
            )
            .unwrap()
            .collect();
        assert_eq!(results.len(), 3);
        let lens: Vec<usize> = results.into_iter().map(|r| r.unwrap().len()).collect();
        assert_eq!(lens, vec![1, 2, 3]);
    }

    #[test]
    fn batched_stream_drains_cleanly_when_fewer_sentences_than_batch_size() {
        let model: Arc<dyn DengjenModel + Send + Sync> = Arc::new(CannedSentenceModel {
            sentences: vec!["a"],
            fail_on: None,
        });
        let synth = DengjenSpeechSynthesizer::new(model).unwrap();
        let mut stream = synth
            .synthesize_batched(
                "irrelevant".to_string(),
                None,
                std::num::NonZeroUsize::new(4).unwrap(),
            )
            .unwrap();
        let first = stream.next().unwrap().unwrap();
        assert_eq!(first.len(), 1);
        assert!(
            stream.next().is_none(),
            "stream must end after its one sentence, not pad to batch_size"
        );
    }

    #[test]
    fn batched_stream_keeps_yielding_after_a_sentence_level_error_within_a_group() {
        let model: Arc<dyn DengjenModel + Send + Sync> = Arc::new(CannedSentenceModel {
            sentences: vec!["a", "bb", "ccc", "dddd"],
            fail_on: Some("bb"),
        });
        let synth = DengjenSpeechSynthesizer::new(model).unwrap();
        let results: Vec<_> = synth
            .synthesize_batched(
                "irrelevant".to_string(),
                None,
                std::num::NonZeroUsize::new(2).unwrap(),
            )
            .unwrap()
            .collect();
        assert_eq!(results.len(), 4);
        assert!(results[0].is_ok());
        assert!(matches!(results[1], Err(DengjenError::OperationError(_))));
        assert!(
            results[2].is_ok(),
            "the second group must still run after the first group had an error"
        );
        assert!(results[3].is_ok());
    }

    struct CountingCanaryModel {
        sentences: Vec<&'static str>,
        calls: Arc<AtomicUsize>,
    }

    impl DengjenModel for CountingCanaryModel {
        fn audio_output_info(&self) -> DengjenResult<AudioInfo> {
            Ok(AudioInfo {
                sample_rate: 16000,
                num_channels: 1,
                sample_width: 2,
            })
        }
        fn phonemize_text(&self, _text: &str) -> DengjenResult<Phonemes> {
            Ok(Phonemes::from(
                self.sentences
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>(),
            ))
        }
        fn speak_batch(&self, phoneme_batches: Vec<String>) -> DengjenResult<Vec<Audio>> {
            phoneme_batches
                .into_iter()
                .map(|ph| self.speak_one_sentence(ph))
                .collect()
        }
        fn speak_one_sentence(&self, phonemes: String) -> DengjenAudioResult {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let n = phonemes.len();
            Ok(Audio::new(
                AudioSamples::from(vec![n as f32; n]),
                16000,
                None,
            ))
        }
        fn get_default_synthesis_config(&self) -> DengjenResult<Option<SynthesisConfig>> {
            Ok(None)
        }
        fn get_fallback_synthesis_config(&self) -> DengjenResult<Option<SynthesisConfig>> {
            Ok(None)
        }
        fn set_fallback_synthesis_config(&self, _c: &SynthesisConfig) -> DengjenResult<()> {
            Ok(())
        }
    }

    #[test]
    fn batched_stream_does_not_synthesize_the_next_group_before_the_current_one_is_drained() {
        let calls = Arc::new(AtomicUsize::new(0));
        let model: Arc<dyn DengjenModel + Send + Sync> = Arc::new(CountingCanaryModel {
            sentences: vec!["a", "b", "c", "d", "e", "f"],
            calls: calls.clone(),
        });
        let synth = DengjenSpeechSynthesizer::new(model).unwrap();
        let mut stream = synth
            .synthesize_batched(
                "irrelevant".to_string(),
                None,
                std::num::NonZeroUsize::new(2).unwrap(),
            )
            .unwrap();

        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "constructing the stream must not eagerly synthesize"
        );

        let _ = stream.next();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "only the first group (batch_size=2) may be synthesized before its first chunk is consumed — not the whole request"
        );

        let _ = stream.next();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "the second chunk of an already-computed group must come from the buffer, not trigger new synthesis"
        );

        let _ = stream.next();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            4,
            "pulling past a drained group must synthesize exactly the next group, not the whole remainder"
        );
    }

    struct ThreadNameCapturingModel {
        names: std::sync::Arc<std::sync::Mutex<Vec<Option<String>>>>,
    }

    impl DengjenModel for ThreadNameCapturingModel {
        fn audio_output_info(&self) -> DengjenResult<AudioInfo> {
            Ok(AudioInfo {
                sample_rate: 16000,
                num_channels: 1,
                sample_width: 2,
            })
        }
        fn phonemize_text(&self, _text: &str) -> DengjenResult<Phonemes> {
            Ok(Phonemes::from(vec!["a".to_string(), "b".to_string()]))
        }
        fn speak_batch(&self, phoneme_batches: Vec<String>) -> DengjenResult<Vec<Audio>> {
            phoneme_batches
                .into_iter()
                .map(|p| self.speak_one_sentence(p))
                .collect()
        }
        fn speak_one_sentence(&self, _phonemes: String) -> DengjenAudioResult {
            self.names
                .lock()
                .unwrap()
                .push(std::thread::current().name().map(str::to_string));
            Ok(Audio::new(AudioSamples::from(vec![0.0f32]), 16000, None))
        }
        fn get_default_synthesis_config(&self) -> DengjenResult<Option<SynthesisConfig>> {
            Ok(None)
        }
        fn get_fallback_synthesis_config(&self) -> DengjenResult<Option<SynthesisConfig>> {
            Ok(None)
        }
        fn set_fallback_synthesis_config(&self, _c: &SynthesisConfig) -> DengjenResult<()> {
            Ok(())
        }
    }

    #[test]
    fn parallel_stream_runs_inference_on_the_dedicated_synthesis_thread_pool() {
        let names = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let model: Arc<dyn DengjenModel + Send + Sync> = Arc::new(ThreadNameCapturingModel {
            names: names.clone(),
        });
        let synth = DengjenSpeechSynthesizer::new(model).unwrap();
        let _: Vec<_> = synth
            .synthesize_parallel("irrelevant".to_string(), None)
            .unwrap()
            .collect();

        let captured = names.lock().unwrap();
        assert_eq!(captured.len(), 2);
        for name in captured.iter() {
            assert!(
                name.as_deref()
                    .is_some_and(|n| n.starts_with("dengjen_synth_")),
                "expected a dengjen_synth_ pool thread, got {name:?}"
            );
        }
    }

    #[test]
    fn lazy_stream_yields_one_result_per_sentence_in_order() {
        let model: Arc<dyn DengjenModel + Send + Sync> = Arc::new(CannedSentenceModel {
            sentences: vec!["a", "bb", "ccc"],
            fail_on: None,
        });
        let synth = DengjenSpeechSynthesizer::new(model).unwrap();
        let results: Vec<_> = synth
            .synthesize_lazy("irrelevant".to_string(), None)
            .unwrap()
            .collect();
        assert_eq!(results.len(), 3);
        let lens: Vec<usize> = results.into_iter().map(|r| r.unwrap().len()).collect();
        assert_eq!(
            lens,
            vec![1, 2, 3],
            "lazy stream must preserve sentence order"
        );
    }

    #[test]
    fn lazy_stream_propagates_a_sentence_level_error() {
        let model: Arc<dyn DengjenModel + Send + Sync> = Arc::new(CannedSentenceModel {
            sentences: vec!["a", "bb", "ccc"],
            fail_on: Some("bb"),
        });
        let synth = DengjenSpeechSynthesizer::new(model).unwrap();
        let results: Vec<_> = synth
            .synthesize_lazy("irrelevant".to_string(), None)
            .unwrap()
            .collect();
        assert_eq!(results.len(), 3);
        assert!(results[0].is_ok());
        assert!(matches!(results[1], Err(DengjenError::OperationError(_))));
        assert!(results[2].is_ok());
    }

    #[test]
    fn parallel_stream_yields_one_result_per_sentence_in_order() {
        let model: Arc<dyn DengjenModel + Send + Sync> = Arc::new(CannedSentenceModel {
            sentences: vec!["a", "bb", "ccc"],
            fail_on: None,
        });
        let synth = DengjenSpeechSynthesizer::new(model).unwrap();
        let results: Vec<_> = synth
            .synthesize_parallel("irrelevant".to_string(), None)
            .unwrap()
            .collect();
        assert_eq!(results.len(), 3);
        let lens: Vec<usize> = results.into_iter().map(|r| r.unwrap().len()).collect();
        assert_eq!(
            lens,
            vec![1, 2, 3],
            "parallel stream must preserve sentence order in its results"
        );
    }

    #[test]
    fn parallel_stream_propagates_a_sentence_level_error() {
        let model: Arc<dyn DengjenModel + Send + Sync> = Arc::new(CannedSentenceModel {
            sentences: vec!["a", "bb", "ccc"],
            fail_on: Some("ccc"),
        });
        let synth = DengjenSpeechSynthesizer::new(model).unwrap();
        let results: Vec<_> = synth
            .synthesize_parallel("irrelevant".to_string(), None)
            .unwrap()
            .collect();
        assert_eq!(results.len(), 3);
        assert_eq!(results.iter().filter(|r| r.is_err()).count(), 1);
    }

    #[test]
    fn synthesize_samples_lazy_strips_audio_down_to_samples() {
        let model: Arc<dyn DengjenModel + Send + Sync> = Arc::new(CannedSentenceModel {
            sentences: vec!["a", "bb"],
            fail_on: None,
        });
        let synth = DengjenSpeechSynthesizer::new(model).unwrap();
        let results: Vec<_> = synth
            .synthesize_samples("irrelevant".to_string(), None, StreamMode::Lazy)
            .unwrap()
            .collect();
        let lens: Vec<usize> = results.into_iter().map(|r| r.unwrap().len()).collect();
        assert_eq!(
            lens,
            vec![1, 2],
            "lazy mode must yield one AudioSamples per sentence, in order"
        );
    }

    #[test]
    fn synthesize_samples_parallel_strips_audio_down_to_samples() {
        let model: Arc<dyn DengjenModel + Send + Sync> = Arc::new(CannedSentenceModel {
            sentences: vec!["a", "bb", "ccc"],
            fail_on: None,
        });
        let synth = DengjenSpeechSynthesizer::new(model).unwrap();
        let results: Vec<_> = synth
            .synthesize_samples("irrelevant".to_string(), None, StreamMode::Parallel)
            .unwrap()
            .collect();
        let lens: Vec<usize> = results.into_iter().map(|r| r.unwrap().len()).collect();
        assert_eq!(
            lens,
            vec![1, 2, 3],
            "parallel mode must preserve sentence order in its output"
        );
    }

    #[test]
    fn synthesize_samples_batched_strips_audio_down_to_samples() {
        let model: Arc<dyn DengjenModel + Send + Sync> = Arc::new(CannedSentenceModel {
            sentences: vec!["a", "bb", "ccc"],
            fail_on: None,
        });
        let synth = DengjenSpeechSynthesizer::new(model).unwrap();
        let results: Vec<_> = synth
            .synthesize_samples(
                "irrelevant".to_string(),
                None,
                StreamMode::Batched {
                    batch_size: std::num::NonZeroUsize::new(2).unwrap(),
                },
            )
            .unwrap()
            .collect();
        let lens: Vec<usize> = results.into_iter().map(|r| r.unwrap().len()).collect();
        assert_eq!(
            lens,
            vec![1, 2, 3],
            "batched mode must preserve sentence order in its output"
        );
    }

    #[test]
    fn synthesize_samples_propagates_a_sentence_level_error() {
        let model: Arc<dyn DengjenModel + Send + Sync> = Arc::new(CannedSentenceModel {
            sentences: vec!["a", "bb"],
            fail_on: Some("bb"),
        });
        let synth = DengjenSpeechSynthesizer::new(model).unwrap();
        let results: Vec<_> = synth
            .synthesize_samples("irrelevant".to_string(), None, StreamMode::Lazy)
            .unwrap()
            .collect();
        assert!(results[0].is_ok());
        assert!(results[1].is_err());
    }

    #[test]
    fn synthesize_samples_realtime_routes_through_synthesize_streamed() {
        let model: Arc<dyn DengjenModel + Send + Sync> = Arc::new(CannedSentenceModel {
            sentences: vec!["a"],
            fail_on: None,
        });
        let synth = DengjenSpeechSynthesizer::new(model).unwrap();
        let mut stream = synth
            .synthesize_samples(
                "irrelevant".to_string(),
                None,
                StreamMode::Realtime {
                    chunk_size: 10,
                    chunk_padding: 0,
                    cancel_token: CancellationToken::new(),
                },
            )
            .unwrap();
        // CannedSentenceModel doesn't implement stream_synthesis, so DengjenModel's default
        // impl surfaces UnsupportedOperation — this is what proves the Realtime arm actually
        // dispatched to synthesize_streamed rather than silently no-op'ing.
        let first = stream.next().unwrap();
        assert!(matches!(first, Err(DengjenError::UnsupportedOperation(_))));
    }

    struct TrailingSilenceModel;

    impl DengjenModel for TrailingSilenceModel {
        fn audio_output_info(&self) -> DengjenResult<AudioInfo> {
            Ok(AudioInfo {
                sample_rate: 16000,
                num_channels: 1,
                sample_width: 2,
            })
        }
        fn phonemize_text(&self, _text: &str) -> DengjenResult<Phonemes> {
            Ok(Phonemes::from(vec!["one".to_string()]))
        }
        fn speak_batch(&self, phoneme_batches: Vec<String>) -> DengjenResult<Vec<Audio>> {
            phoneme_batches
                .into_iter()
                .map(|ph| self.speak_one_sentence(ph))
                .collect()
        }
        fn speak_one_sentence(&self, _phonemes: String) -> DengjenAudioResult {
            let samples = AudioSamples::from(vec![0.8, -0.6, 0.9, 0.01, -0.005, 0.0]);
            Ok(Audio::new(samples, 16000, None))
        }
        fn get_default_synthesis_config(&self) -> DengjenResult<Option<SynthesisConfig>> {
            Ok(None)
        }
        fn get_fallback_synthesis_config(&self) -> DengjenResult<Option<SynthesisConfig>> {
            Ok(None)
        }
        fn set_fallback_synthesis_config(&self, _c: &SynthesisConfig) -> DengjenResult<()> {
            Ok(())
        }
    }

    #[test]
    fn shape_output_trims_the_models_trailing_near_silence_even_without_an_output_config() {
        let model: Arc<dyn DengjenModel + Send + Sync> = Arc::new(TrailingSilenceModel);
        let synth = DengjenSpeechSynthesizer::new(model).unwrap();
        let results: Vec<_> = synth
            .synthesize_lazy("irrelevant".to_string(), None)
            .unwrap()
            .collect();
        assert_eq!(results.len(), 1);
        let audio = results.into_iter().next().unwrap().unwrap();
        let samples = audio.into_vec();
        assert_eq!(samples.len(), 3, "the 0.01/-0.005/0.0 tail is trimmed");
        assert_eq!(samples.last(), Some(&0.0));
    }

    #[test]
    fn shape_output_trims_the_models_tail_before_appending_configured_silence() {
        let model: Arc<dyn DengjenModel + Send + Sync> = Arc::new(TrailingSilenceModel);
        let synth = DengjenSpeechSynthesizer::new(model).unwrap();
        let output_config = Some(AudioOutputConfig {
            rate: None,
            volume: None,
            pitch: None,
            appended_silence_ms: Some(1),
        });
        let results: Vec<_> = synth
            .synthesize_lazy("irrelevant".to_string(), output_config)
            .unwrap()
            .collect();
        let audio = results.into_iter().next().unwrap().unwrap();
        // 3 samples survive trimming; 16 is 1ms of requested silence at 16kHz.
        assert_eq!(audio.len(), 3 + 16);
    }
}

#[cfg(test)]
mod model_type_detection_tests {
    use super::*;
    use std::io::Write;

    fn write_temp_config(dir: &Path, name: &str, contents: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(contents.as_bytes()).unwrap();
        path
    }

    #[test]
    fn detect_model_type_recognizes_kokoro() {
        let dir = std::env::temp_dir().join("dengjen_synth_dispatch_test_kokoro");
        std::fs::create_dir_all(&dir).unwrap();
        let path = write_temp_config(&dir, "config.json", r#"{"model_type": "kokoro"}"#);
        assert_eq!(detect_model_type(&path).unwrap(), "kokoro");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn detect_model_type_recognizes_vits() {
        let dir = std::env::temp_dir().join("dengjen_synth_dispatch_test_vits");
        std::fs::create_dir_all(&dir).unwrap();
        let path = write_temp_config(&dir, "config.json", r#"{"model_type": "vits"}"#);
        assert_eq!(detect_model_type(&path).unwrap(), "vits");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn detect_model_type_defaults_to_piper_when_field_absent() {
        let dir = std::env::temp_dir().join("dengjen_synth_dispatch_test_piper_default");
        std::fs::create_dir_all(&dir).unwrap();
        let path = write_temp_config(&dir, "config.json", r#"{"audio": {"sample_rate": 22050}}"#);
        assert_eq!(detect_model_type(&path).unwrap(), "piper");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn detect_model_type_errors_on_malformed_json() {
        let dir = std::env::temp_dir().join("dengjen_synth_dispatch_test_malformed");
        std::fs::create_dir_all(&dir).unwrap();
        let path = write_temp_config(&dir, "config.json", "{ not valid json");
        assert!(matches!(
            detect_model_type(&path),
            Err(DengjenError::FailedToLoadResource(_))
        ));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn detect_model_type_errors_when_file_does_not_exist() {
        let path = std::path::Path::new("/nonexistent-dengjen-synth-dispatch-test.json");
        assert!(matches!(
            detect_model_type(path),
            Err(DengjenError::FailedToLoadResource(_))
        ));
    }
}

#[cfg(test)]
mod default_batch_size_tests {
    use super::*;

    #[test]
    fn default_batch_size_is_between_one_and_four_inclusive() {
        let n = default_batch_size().get();
        assert!((1..=4).contains(&n), "expected 1..=4, got {n}");
    }
}
