#![forbid(unsafe_code)]

use dengjen_tts::{
    default_batch_size, AudioOutputConfig, DengjenSpeechStreamBatched, DengjenSpeechStreamLazy,
    DengjenSpeechStreamParallel, DengjenSpeechSynthesizer,
};
use dengjen_tts_core::{
    CancellationToken, DengjenAudioResult, DengjenError, DengjenModel, DengjenResult,
    SynthesisConfig,
};
use grpc::dengjen_grpc_server::{DengjenGrpc, DengjenGrpcServer};
use std::collections::HashMap;
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_stream::wrappers::{ReceiverStream, TcpListenerStream};
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use xxhash_rust::xxh3::xxh3_64;

const DEFAULT_DENGJEN_GRPC_SERVER_PORT: u16 = 49314;

type DengjenGrpcResult<T> = Result<T, DengjenGrpcError>;

pub mod grpc {
    tonic::include_proto!("dengjen_grpc");
}

#[derive(Debug)]
enum DengjenGrpcError {
    DengjenError(DengjenError),
    VoiceNotFound(String),
}

impl std::error::Error for DengjenGrpcError {}

impl std::fmt::Display for DengjenGrpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DengjenError(e) => write!(f, "{}", e),
            Self::VoiceNotFound(msg) => write!(f, "{}", msg),
        }
    }
}

impl From<DengjenError> for DengjenGrpcError {
    fn from(err: DengjenError) -> Self {
        Self::DengjenError(err)
    }
}

impl From<DengjenGrpcError> for Status {
    fn from(err: DengjenGrpcError) -> Self {
        match err {
            DengjenGrpcError::DengjenError(e) => match e {
                DengjenError::FailedToLoadResource(msg) => Status::aborted(msg),
                DengjenError::PhonemizationError(msg) => Status::aborted(msg),
                DengjenError::InferenceError(msg) => Status::internal(msg),
                DengjenError::InvalidConfiguration(msg) => Status::invalid_argument(msg),
                DengjenError::UnsupportedOperation(msg) => Status::unimplemented(msg),
                DengjenError::OperationError(msg) => Status::unknown(msg),
            },
            DengjenGrpcError::VoiceNotFound(msg) => Status::not_found(msg),
        }
    }
}

/// A loaded voice, backed by a speech synthesizer wrapping some `DengjenModel`.
struct Voice(Arc<DengjenSpeechSynthesizer>);

impl Voice {
    fn new(model: Arc<dyn DengjenModel + Send + Sync>) -> DengjenResult<Self> {
        Ok(Self(Arc::new(DengjenSpeechSynthesizer::new(model)?)))
    }

    /// `DengjenSpeechSynthesizer` implements `DengjenModel` by delegating to the
    /// wrapped model, so the synthesizer itself can stand in as the model reference.
    fn model_ref(&self) -> &dyn DengjenModel {
        self.synth_ref()
    }

    fn synth_ref(&self) -> &DengjenSpeechSynthesizer {
        &self.0
    }
}

/// Registry of loaded voices, keyed by voice key.
struct DengjenGrpcService(RwLock<HashMap<String, Voice>>);

impl DengjenGrpcService {
    fn new() -> Self {
        Self(RwLock::new(HashMap::new()))
    }

    /// Derives a stable voice key from a canonicalized config path.
    fn voice_key_for_path(config_path: &std::path::Path) -> DengjenGrpcResult<String> {
        let canonical_path = config_path
            .canonicalize()
            .map_err(|source| {
                DengjenGrpcError::VoiceNotFound(format!(
                    "Could not resolve config path `{}`: {source}",
                    config_path.display()
                ))
            })?
            .to_string_lossy()
            .into_owned();
        Ok(format!("{:016x}", xxh3_64(canonical_path.as_bytes())))
    }

    fn load_voice_from_config(
        &self,
        config_path: PathBuf,
    ) -> DengjenGrpcResult<grpc::VoiceDescriptor> {
        if !config_path.is_file() {
            return Err(DengjenGrpcError::VoiceNotFound(format!(
                "Config file does not exists: `{}`",
                config_path.display()
            )));
        }
        let voice_key = Self::voice_key_for_path(&config_path)?;
        if let Some(voice) = self.0.read().unwrap().get(&voice_key) {
            return self.build_voice_info(voice_key, voice.model_ref());
        }

        let model = load_voice(&config_path)?;
        log::info!(
            "Loaded Vits voice from: `{}`. Voice key: {}",
            config_path.display(),
            voice_key
        );
        let voice = Voice::new(model)?;
        let voice_info = self.build_voice_info(voice_key.clone(), voice.model_ref())?;
        self.0.write().unwrap().insert(voice_key, voice);
        Ok(voice_info)
    }

    fn lookup_synth(&self, voice_key: &str) -> DengjenGrpcResult<Arc<DengjenSpeechSynthesizer>> {
        let voices = self.0.read().unwrap();
        let voice = voices.get(voice_key).ok_or_else(|| {
            DengjenGrpcError::VoiceNotFound(format!(
                "A voice with the key `{}` has not been loaded",
                voice_key
            ))
        })?;
        Ok(Arc::clone(&voice.0))
    }

    fn build_voice_info(
        &self,
        voice_key: String,
        model: &(impl DengjenModel + ?Sized),
    ) -> DengjenGrpcResult<grpc::VoiceDescriptor> {
        let audio_info = model.audio_output_info()?;
        let speakers = model.get_speakers()?.cloned().unwrap_or_default();
        let language = model.get_language()?;
        let synth_options = Self::synth_options_from_default_config(model)?;
        Ok(grpc::VoiceDescriptor {
            voice_key,
            synthesis_options: Some(synth_options),
            language,
            speakers,
            audio: Some(grpc::AudioFormat {
                sample_rate: audio_info.sample_rate as u32,
                num_channels: audio_info.num_channels as u32,
                sample_width: audio_info.sample_width as u32,
            }),
            supports_streaming_output: Some(model.supports_streaming_output()),
            quality: None,
        })
    }

    fn synth_options_from_synthesis_config(
        model: &(impl DengjenModel + ?Sized),
        config: &SynthesisConfig,
        on_unset_speaker: impl FnOnce() -> DengjenGrpcResult<Option<String>>,
    ) -> DengjenGrpcResult<grpc::SynthesisSettings> {
        let speaker = match config.speaker {
            Some(ref speaker_id) => model.speaker_id_to_name(speaker_id)?,
            None => on_unset_speaker()?,
        };
        let piper_config = dengjen_tts_piper::PiperSynthesisConfig::from(config);
        let named_keys = [
            dengjen_tts_piper::synth_config::LENGTH_SCALE,
            dengjen_tts_piper::synth_config::NOISE_SCALE,
            dengjen_tts_piper::synth_config::NOISE_W,
        ];
        let has_piper_scales = named_keys
            .iter()
            .any(|key| config.parameters.contains_key(*key));
        let parameters = config
            .parameters
            .iter()
            .filter(|(key, _)| !named_keys.contains(&key.as_str()))
            .map(|(key, value)| (key.clone(), *value))
            .collect();
        Ok(grpc::SynthesisSettings {
            speaker,
            length_scale: has_piper_scales.then_some(piper_config.length_scale),
            noise_scale: has_piper_scales.then_some(piper_config.noise_scale),
            noise_w: has_piper_scales.then_some(piper_config.noise_w),
            parameters,
        })
    }

    fn synth_options_for_configless_backend(
        on_unset_speaker: impl FnOnce() -> DengjenGrpcResult<Option<String>>,
    ) -> DengjenGrpcResult<grpc::SynthesisSettings> {
        Ok(grpc::SynthesisSettings {
            speaker: on_unset_speaker()?,
            length_scale: None,
            noise_scale: None,
            noise_w: None,
            parameters: std::collections::HashMap::new(),
        })
    }

    fn synth_options_from_default_config(
        model: &(impl DengjenModel + ?Sized),
    ) -> DengjenGrpcResult<grpc::SynthesisSettings> {
        match model.get_default_synthesis_config()? {
            Some(config) => Self::synth_options_from_synthesis_config(model, &config, || {
                Ok(Some("Default".to_string()))
            }),
            None => Self::synth_options_for_configless_backend(|| Ok(Some("Default".to_string()))),
        }
    }

    fn synth_options_from_model(
        model: &(impl DengjenModel + ?Sized),
    ) -> DengjenGrpcResult<grpc::SynthesisSettings> {
        match model.get_fallback_synthesis_config()? {
            Some(config) => Self::synth_options_from_synthesis_config(model, &config, || {
                Ok(model.speaker_id_to_name(&0)?)
            }),
            None => {
                Self::synth_options_for_configless_backend(|| Ok(model.speaker_id_to_name(&0)?))
            }
        }
    }

    fn lookup_synth_options(&self, voice_key: &str) -> DengjenGrpcResult<grpc::SynthesisSettings> {
        let voices = self.0.read().unwrap();
        let voice = voices.get(voice_key).ok_or_else(|| {
            DengjenGrpcError::VoiceNotFound(format!(
                "A voice with the key `{}` has not been loaded",
                voice_key
            ))
        })?;
        Self::synth_options_from_model(voice.model_ref())
    }

    fn apply_synth_options(
        &self,
        voice_key: &str,
        synth_opts: grpc::SynthesisSettings,
    ) -> DengjenGrpcResult<grpc::SynthesisSettings> {
        let voices = self.0.read().unwrap();
        let voice = voices.get(voice_key).ok_or_else(|| {
            DengjenGrpcError::VoiceNotFound(format!(
                "A voice with the key `{}` has not been loaded",
                voice_key
            ))
        })?;
        let model = voice.model_ref();
        let mut new_config = model.get_fallback_synthesis_config()?.unwrap_or_default();
        if let Some(speaker_name) = &synth_opts.speaker {
            if let Some(speaker_id) = model.speaker_name_to_id(speaker_name)? {
                new_config.speaker = Some(speaker_id);
            }
        }
        new_config.parameters.extend(synth_opts.parameters);
        if let Some(length_scale) = synth_opts.length_scale {
            new_config.parameters.insert(
                dengjen_tts_piper::synth_config::LENGTH_SCALE.to_string(),
                length_scale,
            );
        }
        if let Some(noise_scale) = synth_opts.noise_scale {
            new_config.parameters.insert(
                dengjen_tts_piper::synth_config::NOISE_SCALE.to_string(),
                noise_scale,
            );
        }
        if let Some(noise_w) = synth_opts.noise_w {
            new_config.parameters.insert(
                dengjen_tts_piper::synth_config::NOISE_W.to_string(),
                noise_w,
            );
        }
        model.set_fallback_synthesis_config(&new_config)?;
        Self::synth_options_from_model(model)
    }
}

fn load_voice(config_path: &std::path::Path) -> DengjenResult<Arc<dyn DengjenModel + Send + Sync>> {
    let model_type = dengjen_tts::detect_model_type(config_path)?;
    if model_type == "kokoro" {
        return dengjen_tts_kokoro::from_config_path(config_path);
    }
    if model_type == "melotts" {
        return dengjen_tts_melotts::from_config_path(config_path);
    }
    dengjen_tts_piper::from_config_path(config_path)
}

fn narrow_prosody_value(name: &str, value: u32) -> Result<u8, String> {
    u8::try_from(value)
        .ok()
        .filter(|&v| v <= 100)
        .ok_or_else(|| format!("`{name}` must be in the range 0-100, got {value}"))
}

fn output_config_from_prosody(
    args: Option<grpc::ProsodyControls>,
) -> Result<Option<AudioOutputConfig>, String> {
    let Some(args) = args else {
        return Ok(None);
    };
    Ok(Some(AudioOutputConfig {
        rate: args
            .rate
            .map(|v| narrow_prosody_value("rate", v))
            .transpose()?,
        volume: args
            .volume
            .map(|v| narrow_prosody_value("volume", v))
            .transpose()?,
        pitch: args
            .pitch
            .map(|v| narrow_prosody_value("pitch", v))
            .transpose()?,
        appended_silence_ms: args.appended_silence_ms,
    }))
}

#[cfg(test)]
mod prosody_tests {
    use super::*;

    #[test]
    fn output_config_from_prosody_is_none_when_no_prosody_was_sent() {
        assert!(output_config_from_prosody(None).unwrap().is_none());
    }

    #[test]
    fn output_config_from_prosody_narrows_in_range_values() {
        let config = output_config_from_prosody(Some(grpc::ProsodyControls {
            rate: Some(50),
            volume: Some(100),
            pitch: Some(0),
            appended_silence_ms: Some(200),
        }))
        .unwrap()
        .unwrap();
        assert_eq!(config.rate, Some(50));
        assert_eq!(config.volume, Some(100));
        assert_eq!(config.pitch, Some(0));
        assert_eq!(config.appended_silence_ms, Some(200));
    }

    #[test]
    fn output_config_from_prosody_rejects_a_value_above_u8_range_instead_of_wrapping() {
        let result = output_config_from_prosody(Some(grpc::ProsodyControls {
            rate: Some(300),
            volume: None,
            pitch: None,
            appended_silence_ms: None,
        }));
        assert!(result.is_err());
    }

    #[test]
    fn output_config_from_prosody_rejects_a_value_within_u8_range_but_above_the_documented_100() {
        let result = output_config_from_prosody(Some(grpc::ProsodyControls {
            rate: Some(101),
            volume: None,
            pitch: None,
            appended_silence_ms: None,
        }));
        assert!(result.is_err());
    }
}

fn drain_stream_into_channel<Chunk, Message>(
    stream: impl Iterator<Item = DengjenResult<Chunk>>,
    tx: mpsc::Sender<Result<Message, Status>>,
    to_message: impl Fn(Chunk) -> Message,
) {
    for chunk_result in stream {
        let chunk = match chunk_result {
            Ok(chunk) => chunk,
            Err(e) => {
                tx.blocking_send(Err(DengjenGrpcError::from(e).into())).ok();
                return;
            }
        };
        if tx.blocking_send(Ok(to_message(chunk))).is_err() {
            return;
        }
    }
}

enum SynthesisStream {
    Lazy(DengjenSpeechStreamLazy),
    Parallel(DengjenSpeechStreamParallel),
    Batched(DengjenSpeechStreamBatched),
}

impl Iterator for SynthesisStream {
    type Item = DengjenAudioResult;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Lazy(stream) => stream.next(),
            Self::Parallel(stream) => stream.next(),
            Self::Batched(stream) => stream.next(),
        }
    }
}

fn resolve_batch_size(batch_size: Option<u32>) -> DengjenGrpcResult<std::num::NonZeroUsize> {
    match batch_size {
        None => Ok(default_batch_size()),
        // `usize` is >= 32 bits on every target this crate builds for, so this never truncates.
        Some(n) => std::num::NonZeroUsize::new(n as usize).ok_or_else(|| {
            DengjenError::InvalidConfiguration("batch_size must be greater than zero".to_string())
                .into()
        }),
    }
}

fn build_synthesis_stream(
    synth: &DengjenSpeechSynthesizer,
    text: String,
    output_config: Option<AudioOutputConfig>,
    mode: grpc::SynthesisMode,
    batch_size: Option<u32>,
) -> DengjenGrpcResult<SynthesisStream> {
    match mode {
        grpc::SynthesisMode::ModeUnspecified | grpc::SynthesisMode::ModeLazy => Ok(
            SynthesisStream::Lazy(synth.synthesize_lazy(text, output_config)?),
        ),
        grpc::SynthesisMode::ModeParallel => Ok(SynthesisStream::Parallel(
            synth.synthesize_parallel(text, output_config)?,
        )),
        grpc::SynthesisMode::ModeBatched => {
            let batch_size = resolve_batch_size(batch_size)?;
            Ok(SynthesisStream::Batched(synth.synthesize_batched(
                text,
                output_config,
                batch_size,
            )?))
        }
    }
}

#[tonic::async_trait]
impl DengjenGrpc for DengjenGrpcService {
    async fn get_dengjen_version(
        &self,
        _request: Request<grpc::Empty>,
    ) -> Result<Response<grpc::Version>, Status> {
        Ok(Response::new(grpc::Version {
            version: env!("CARGO_PKG_VERSION").into(),
        }))
    }

    async fn load_voice(
        &self,
        request: Request<grpc::VoiceConfigLocation>,
    ) -> Result<Response<grpc::VoiceDescriptor>, Status> {
        let config_path = PathBuf::from(request.into_inner().path);
        let voice_info = self.load_voice_from_config(config_path)?;
        Ok(Response::new(voice_info))
    }

    async fn get_voice_info(
        &self,
        request: Request<grpc::VoiceRef>,
    ) -> Result<Response<grpc::VoiceDescriptor>, Status> {
        let voice_key = request.into_inner().voice_key;
        let voices = self.0.read().unwrap();
        let voice = voices.get(&voice_key).ok_or_else(|| {
            DengjenGrpcError::VoiceNotFound(format!(
                "A voice with the key `{}` has not been loaded",
                voice_key
            ))
        })?;
        let voice_info = self.build_voice_info(voice_key, voice.model_ref())?;
        Ok(Response::new(voice_info))
    }

    async fn get_synthesis_options(
        &self,
        request: Request<grpc::VoiceRef>,
    ) -> Result<Response<grpc::SynthesisSettings>, Status> {
        let voice_key = request.into_inner().voice_key;
        let synth_opts = self.lookup_synth_options(&voice_key)?;
        Ok(Response::new(synth_opts))
    }

    async fn set_synthesis_options(
        &self,
        request: Request<grpc::VoiceSynthesisSettings>,
    ) -> Result<Response<grpc::SynthesisSettings>, Status> {
        let req = request.into_inner();
        let synth_opts = req
            .synthesis_options
            .ok_or_else(|| Status::invalid_argument("No synthesis options provided"))?;
        let new_synth_opts = self.apply_synth_options(&req.voice_key, synth_opts)?;
        Ok(Response::new(new_synth_opts))
    }

    type SynthesizeUtteranceStream = ReceiverStream<Result<grpc::SynthesisChunk, Status>>;
    async fn synthesize_utterance(
        &self,
        request: Request<grpc::SynthesisRequest>,
    ) -> Result<Response<Self::SynthesizeUtteranceStream>, Status> {
        let req = request.into_inner();
        let output_config =
            output_config_from_prosody(req.prosody).map_err(Status::invalid_argument)?;
        let mode = grpc::SynthesisMode::try_from(req.synthesis_mode).map_err(|_| {
            Status::invalid_argument(format!(
                "Unrecognized synthesis_mode value: {}",
                req.synthesis_mode
            ))
        })?;
        let batch_size = req.batch_size;
        // Validate eagerly for `ModeBatched` so a `batch_size: Some(0)` request fails before a
        // `Response`/blocking-pool thread is ever handed out; other modes ignore `batch_size`
        // and stay unvalidated, matching `build_synthesis_stream`'s dispatch below.
        if mode == grpc::SynthesisMode::ModeBatched {
            resolve_batch_size(batch_size)?;
        }
        let synth = self.lookup_synth(&req.voice_key)?;

        let (tx, rx) = mpsc::channel(512);
        tokio::task::spawn_blocking(move || {
            let dengjen_stream =
                match build_synthesis_stream(&synth, req.text, output_config, mode, batch_size) {
                    Ok(stream) => stream,
                    Err(e) => {
                        tx.blocking_send(Err(e.into())).ok();
                        return;
                    }
                };
            drain_stream_into_channel(dengjen_stream, tx, |wav| grpc::SynthesisChunk {
                audio_bytes: wav.as_wave_bytes(),
                real_time_factor: wav.real_time_factor().unwrap_or_default(),
            });
        });
        Ok(Response::new(ReceiverStream::new(rx)))
    }

    type SynthesizeUtteranceRealtimeStream =
        ReceiverStream<Result<grpc::RealtimeAudioChunk, Status>>;
    async fn synthesize_utterance_realtime(
        &self,
        request: Request<grpc::SynthesisRequest>,
    ) -> Result<Response<Self::SynthesizeUtteranceRealtimeStream>, Status> {
        let req = request.into_inner();
        let output_config =
            output_config_from_prosody(req.prosody).map_err(Status::invalid_argument)?;
        let synth = {
            let voices = self.0.read().unwrap();
            let voice = voices.get(&req.voice_key).ok_or_else(|| {
                DengjenGrpcError::VoiceNotFound(format!(
                    "A voice with the key `{}` has not been loaded",
                    req.voice_key
                ))
            })?;
            Arc::clone(&voice.0)
        };

        let (tx, rx) = mpsc::channel(512);
        tokio::task::spawn_blocking(move || {
            let realtime_speech_stream = match synth.synthesize_streamed(
                req.text,
                output_config,
                55,
                3,
                CancellationToken::new(),
            ) {
                Ok(stream) => stream,
                Err(e) => {
                    tx.blocking_send(Err(DengjenGrpcError::from(e).into())).ok();
                    return;
                }
            };
            drain_stream_into_channel(realtime_speech_stream, tx, |wav| grpc::RealtimeAudioChunk {
                audio_bytes: wav.as_wave_bytes(),
            });
        });
        Ok(Response::new(ReceiverStream::new(rx)))
    }
}

fn setup_logging() -> Result<(), Box<dyn std::error::Error>> {
    // `fmt().init()` installs the LogTracer bridge itself (tracing-subscriber's
    // default "tracing-log" feature) — a separate explicit `LogTracer::init()` call
    // here would double-install the `log` backend and panic with `SetLoggerError`.
    let filter = match std::env::var("DENGJEN_GRPC") {
        Ok(directives) => {
            tracing_subscriber::EnvFilter::try_new(&directives).unwrap_or_else(|err| {
                eprintln!(
                    "warning: ignoring malformed DENGJEN_GRPC ({err}); defaulting to \"info\""
                );
                tracing_subscriber::EnvFilter::new("info")
            })
        }
        Err(_) => tracing_subscriber::EnvFilter::new("info"),
    };
    // stdout carries the listening-port announcement (see `listening_announcement`); logs
    // must go to stderr, matching the `env_logger` default this replaces, or they corrupt
    // that line for anything parsing it.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
        .with_env_filter(filter)
        .try_init()
        .map_err(|err| -> Box<dyn std::error::Error> { err })?;
    Ok(())
}

fn init_ort_environment() -> bool {
    let cpu_provider = ort::execution_providers::CPU::default().build();
    ort::init()
        .with_name("dengjen")
        .with_execution_providers([cpu_provider])
        .commit()
}

fn resolve_listen_port() -> u16 {
    std::env::var("DENGJEN_GRPC_SERVER_PORT")
        .ok()
        .and_then(|raw_port| raw_port.parse().ok())
        .unwrap_or(DEFAULT_DENGJEN_GRPC_SERVER_PORT)
}

fn listening_announcement(port: u16) -> String {
    format!("DENGJEN_GRPC_LISTENING port={port}")
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    setup_logging()?;

    if !init_ort_environment() {
        log::error!("Could not initialize onnxruntime environment");
    }

    let addr = SocketAddr::new(
        IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
        resolve_listen_port(),
    );
    let listener = TcpListener::bind(addr).await?;
    let bound_port = listener.local_addr()?.port();
    println!("{}", listening_announcement(bound_port));
    std::io::stdout().flush()?;
    log::info!("Dengjen GRPC server listening on 127.0.0.1:{bound_port}");

    let server = DengjenGrpcServer::new(DengjenGrpcService::new());
    Server::builder()
        .add_service(server)
        .serve_with_incoming(TcpListenerStream::new(listener))
        .await?;

    Ok(())
}

#[cfg(test)]
mod error_mapping_tests {
    use super::*;

    #[test]
    fn voice_not_found_displays_its_message_verbatim() {
        let err = DengjenGrpcError::VoiceNotFound("no such voice".to_string());
        assert_eq!(err.to_string(), "no such voice");
    }

    #[test]
    fn dengjen_error_display_delegates_to_the_wrapped_error() {
        let inner = DengjenError::OperationError("boom".to_string());
        let err = DengjenGrpcError::from(inner);
        assert_eq!(
            err.to_string(),
            DengjenError::OperationError("boom".to_string()).to_string()
        );
    }

    #[test]
    fn voice_not_found_maps_to_status_not_found() {
        let status: Status = DengjenGrpcError::VoiceNotFound("x".to_string()).into();
        assert_eq!(status.code(), tonic::Code::NotFound);
        assert_eq!(status.message(), "x");
    }

    #[test]
    fn failed_to_load_resource_maps_to_status_aborted() {
        let status: Status =
            DengjenGrpcError::from(DengjenError::FailedToLoadResource("x".into())).into();
        assert_eq!(status.code(), tonic::Code::Aborted);
    }

    #[test]
    fn phonemization_error_maps_to_status_aborted() {
        let status: Status =
            DengjenGrpcError::from(DengjenError::PhonemizationError("x".into())).into();
        assert_eq!(status.code(), tonic::Code::Aborted);
    }

    #[test]
    fn inference_error_maps_to_status_internal() {
        let status: Status =
            DengjenGrpcError::from(DengjenError::InferenceError("x".into())).into();
        assert_eq!(status.code(), tonic::Code::Internal);
    }

    #[test]
    fn invalid_configuration_maps_to_status_invalid_argument() {
        let status: Status =
            DengjenGrpcError::from(DengjenError::InvalidConfiguration("x".into())).into();
        assert_eq!(status.code(), tonic::Code::InvalidArgument);
    }

    #[test]
    fn unsupported_operation_maps_to_status_unimplemented() {
        let status: Status =
            DengjenGrpcError::from(DengjenError::UnsupportedOperation("x".into())).into();
        assert_eq!(status.code(), tonic::Code::Unimplemented);
    }

    #[test]
    fn operation_error_maps_to_status_unknown() {
        let status: Status =
            DengjenGrpcError::from(DengjenError::OperationError("x".into())).into();
        assert_eq!(status.code(), tonic::Code::Unknown);
    }
}

#[cfg(test)]
mod listening_announcement_tests {
    use super::*;

    #[test]
    fn listening_announcement_reports_the_given_port() {
        assert_eq!(
            listening_announcement(49314),
            "DENGJEN_GRPC_LISTENING port=49314"
        );
    }

    #[test]
    fn listening_announcement_reports_an_ephemeral_high_port() {
        assert_eq!(
            listening_announcement(65535),
            "DENGJEN_GRPC_LISTENING port=65535"
        );
    }
}

#[cfg(test)]
mod synthesis_request_proto_tests {
    use super::*;

    #[test]
    fn batch_size_is_unset_by_default() {
        let req = grpc::SynthesisRequest::default();
        assert_eq!(req.batch_size, None);
    }
}

#[cfg(test)]
mod voice_loading_tests {
    use super::*;
    use dengjen_tts_core::{Audio, AudioInfo as CoreAudioInfo, DengjenAudioResult, Phonemes};
    use dengjen_tts_piper::PiperSynthesisConfig;
    use std::collections::HashMap as StdHashMap;

    struct FakeModel {
        speakers: StdHashMap<i64, String>,
    }

    impl DengjenModel for FakeModel {
        fn audio_output_info(&self) -> DengjenResult<CoreAudioInfo> {
            Ok(CoreAudioInfo {
                sample_rate: 22050,
                num_channels: 1,
                sample_width: 2,
            })
        }
        fn phonemize_text(&self, _text: &str) -> DengjenResult<Phonemes> {
            Ok(Phonemes::from(vec!["fake".to_string()]))
        }
        fn speak_batch(&self, _phoneme_batches: Vec<String>) -> DengjenResult<Vec<Audio>> {
            Ok(Vec::new())
        }
        fn speak_one_sentence(&self, _phonemes: String) -> DengjenAudioResult {
            Ok(Audio::new(Default::default(), 22050, None))
        }
        fn get_default_synthesis_config(&self) -> DengjenResult<Option<SynthesisConfig>> {
            Ok(Some(
                SynthesisConfig::from(&PiperSynthesisConfig::default()),
            ))
        }
        fn get_fallback_synthesis_config(&self) -> DengjenResult<Option<SynthesisConfig>> {
            Ok(Some(
                SynthesisConfig::from(&PiperSynthesisConfig::default()),
            ))
        }
        fn set_fallback_synthesis_config(&self, _c: &SynthesisConfig) -> DengjenResult<()> {
            Ok(())
        }
        fn get_speakers(&self) -> DengjenResult<Option<&StdHashMap<i64, String>>> {
            Ok(Some(&self.speakers))
        }
        fn get_language(&self) -> DengjenResult<Option<String>> {
            Ok(Some("en-US".to_string()))
        }
        fn supports_streaming_output(&self) -> bool {
            true
        }
    }

    fn service_with_voice(voice_key: &str, model: FakeModel) -> DengjenGrpcService {
        let service = DengjenGrpcService::new();
        let voice = Voice::new(Arc::new(model)).unwrap();
        service
            .0
            .write()
            .unwrap()
            .insert(voice_key.to_string(), voice);
        service
    }

    #[test]
    fn voice_key_for_path_errors_instead_of_panicking_on_an_unresolvable_path() {
        let result = DengjenGrpcService::voice_key_for_path(std::path::Path::new(
            "/does/not/exist-dengjen-test.json",
        ));
        assert!(matches!(result, Err(DengjenGrpcError::VoiceNotFound(_))));
    }

    #[test]
    fn voice_key_for_path_derives_distinct_keys_for_distinct_files() {
        let dir = std::env::temp_dir().join(format!(
            "dengjen-grpc-voice-key-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let first = dir.join("first.json");
        let second = dir.join("second.json");
        std::fs::write(&first, "{}").unwrap();
        std::fs::write(&second, "{}").unwrap();

        let first_key = DengjenGrpcService::voice_key_for_path(&first).unwrap();
        let second_key = DengjenGrpcService::voice_key_for_path(&second).unwrap();

        let _ = std::fs::remove_dir_all(&dir);
        assert_ne!(first_key, second_key);

        assert_eq!(first_key.len(), 16);
    }

    #[test]
    fn load_voice_from_config_reports_voice_not_found_for_a_missing_config_path() {
        let service = DengjenGrpcService::new();
        let result = service.load_voice_from_config(PathBuf::from("/does/not/exist.json"));
        match result {
            Err(DengjenGrpcError::VoiceNotFound(msg)) => {
                assert!(msg.contains("/does/not/exist.json"), "message was: {msg}");
            }
            other => panic!("expected VoiceNotFound, got {other:?}"),
        }
    }

    #[test]
    fn lookup_synth_reports_voice_not_found_for_an_unloaded_voice() {
        let service = DengjenGrpcService::new();
        let result = service.lookup_synth("missing");
        assert!(matches!(result, Err(DengjenGrpcError::VoiceNotFound(_))));
    }

    #[test]
    fn lookup_synth_succeeds_for_a_loaded_voice() {
        let model = FakeModel {
            speakers: StdHashMap::new(),
        };
        let service = service_with_voice("v1", model);
        assert!(service.lookup_synth("v1").is_ok());
    }

    #[test]
    fn build_synthesis_stream_dispatches_lazy_by_default_and_when_unspecified() {
        let model = FakeModel {
            speakers: StdHashMap::new(),
        };
        let service = service_with_voice("v1", model);
        let synth = service.lookup_synth("v1").unwrap();
        for mode in [
            grpc::SynthesisMode::ModeUnspecified,
            grpc::SynthesisMode::ModeLazy,
        ] {
            let stream =
                build_synthesis_stream(&synth, "hi".to_string(), None, mode, None).unwrap();
            assert!(matches!(stream, SynthesisStream::Lazy(_)));
        }
    }

    #[test]
    fn build_synthesis_stream_dispatches_parallel() {
        let model = FakeModel {
            speakers: StdHashMap::new(),
        };
        let service = service_with_voice("v1", model);
        let synth = service.lookup_synth("v1").unwrap();
        let stream = build_synthesis_stream(
            &synth,
            "hi".to_string(),
            None,
            grpc::SynthesisMode::ModeParallel,
            None,
        )
        .unwrap();
        assert!(matches!(stream, SynthesisStream::Parallel(_)));
    }

    #[test]
    fn build_synthesis_stream_dispatches_batched_with_default_batch_size_when_unset() {
        let model = FakeModel {
            speakers: StdHashMap::new(),
        };
        let service = service_with_voice("v1", model);
        let synth = service.lookup_synth("v1").unwrap();
        let stream = build_synthesis_stream(
            &synth,
            "hi".to_string(),
            None,
            grpc::SynthesisMode::ModeBatched,
            None,
        )
        .unwrap();
        assert!(matches!(stream, SynthesisStream::Batched(_)));
    }

    #[test]
    fn build_synthesis_stream_rejects_an_explicit_zero_batch_size() {
        let model = FakeModel {
            speakers: StdHashMap::new(),
        };
        let service = service_with_voice("v1", model);
        let synth = service.lookup_synth("v1").unwrap();
        let result = build_synthesis_stream(
            &synth,
            "hi".to_string(),
            None,
            grpc::SynthesisMode::ModeBatched,
            Some(0),
        );
        assert!(matches!(
            result,
            Err(DengjenGrpcError::DengjenError(
                DengjenError::InvalidConfiguration(_)
            ))
        ));
    }

    #[tokio::test]
    async fn synthesize_utterance_rejects_an_out_of_range_synthesis_mode() {
        let model = FakeModel {
            speakers: StdHashMap::new(),
        };
        let service = service_with_voice("v1", model);
        let request = Request::new(grpc::SynthesisRequest {
            voice_key: "v1".to_string(),
            text: "hi".to_string(),
            prosody: None,
            synthesis_mode: 99,
            batch_size: None,
        });
        let status = service
            .synthesize_utterance(request)
            .await
            .expect_err("an out-of-range synthesis_mode must be rejected");
        assert_eq!(status.code(), tonic::Code::InvalidArgument);
    }

    #[test]
    fn get_voice_info_reports_speakers_audio_and_language() {
        let mut speakers = StdHashMap::new();
        speakers.insert(1i64, "Alice".to_string());
        let model = FakeModel { speakers };
        let service = service_with_voice("v1", model);
        let voices = service.0.read().unwrap();
        let voice = voices.get("v1").unwrap();
        let info = service
            .build_voice_info("v1".to_string(), voice.model_ref())
            .unwrap();
        assert_eq!(info.voice_key, "v1");
        assert_eq!(info.language.as_deref(), Some("en-US"));
        assert_eq!(info.speakers.get(&1), Some(&"Alice".to_string()));
        assert_eq!(info.supports_streaming_output, Some(true));
        let audio = info.audio.unwrap();
        assert_eq!(audio.sample_rate, 22050);
        assert_eq!(audio.num_channels, 1);
        assert_eq!(audio.sample_width, 2);
    }

    #[test]
    fn get_voice_info_defaults_speaker_name_to_default_when_none_configured() {
        let model = FakeModel {
            speakers: StdHashMap::new(),
        };
        let service = service_with_voice("v1", model);
        let voices = service.0.read().unwrap();
        let voice = voices.get("v1").unwrap();
        let info = service
            .build_voice_info("v1".to_string(), voice.model_ref())
            .unwrap();
        assert_eq!(
            info.synthesis_options.unwrap().speaker.as_deref(),
            Some("Default")
        );
    }

    #[tokio::test]
    async fn synthesize_utterance_does_not_panic_when_the_receiver_is_dropped_mid_stream() {
        // `spawn_blocking`'s `JoinHandle` is never exposed, so a panic there is otherwise
        // undetectable; this hooks the global panic handler instead. Risk: a concurrent test's
        // own panic during the ~50ms window below could misattribute here.
        use std::panic::{set_hook, take_hook};
        use std::sync::atomic::{AtomicBool, Ordering};

        let panicked = std::sync::Arc::new(AtomicBool::new(false));
        let panicked_flag = panicked.clone();
        let previous_hook = take_hook();
        set_hook(Box::new(move |info| {
            panicked_flag.store(true, Ordering::SeqCst);
            previous_hook(info);
        }));

        let model = FakeModel {
            speakers: StdHashMap::new(),
        };
        let service = service_with_voice("v1", model);
        let request = Request::new(grpc::SynthesisRequest {
            voice_key: "v1".to_string(),
            text: "hi".to_string(),
            prosody: None,
            synthesis_mode: grpc::SynthesisMode::ModeBatched as i32,
            batch_size: Some(1),
        });

        let response = service
            .synthesize_utterance(request)
            .await
            .expect("dispatch must succeed");
        // Drop before polling any items: races the mpsc receiver against the still-possibly-
        // mid-synthesis blocking task, exercising `drain_stream_into_channel`'s "receiver went
        // away" path.
        drop(response);

        // Give the spawned blocking task a moment to observe the dropped
        // receiver and return; if it panicked, the hook above recorded it.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // `take_hook()` alone uninstalls the current hook and reinstalls the default,
        // returning what it removed; calling `set_hook` on that return value would
        // just reinstall the very hook we're trying to discard.
        let _ = take_hook();
        assert!(
            !panicked.load(Ordering::SeqCst),
            "spawned blocking task panicked during mid-stream drop"
        );
    }

    #[tokio::test]
    async fn synthesize_utterance_rejects_an_explicit_zero_batch_size() {
        let model = FakeModel {
            speakers: StdHashMap::new(),
        };
        let service = service_with_voice("v1", model);
        let request = Request::new(grpc::SynthesisRequest {
            voice_key: "v1".to_string(),
            text: "hi".to_string(),
            prosody: None,
            synthesis_mode: grpc::SynthesisMode::ModeBatched as i32,
            batch_size: Some(0),
        });

        let status = service
            .synthesize_utterance(request)
            .await
            .expect_err("an explicit zero batch_size must be rejected for ModeBatched");
        assert_eq!(status.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn synthesize_utterance_ignores_batch_size_for_non_batched_modes() {
        let model = FakeModel {
            speakers: StdHashMap::new(),
        };
        let service = service_with_voice("v1", model);
        let request = Request::new(grpc::SynthesisRequest {
            voice_key: "v1".to_string(),
            text: "hi".to_string(),
            prosody: None,
            synthesis_mode: grpc::SynthesisMode::ModeLazy as i32,
            batch_size: Some(0),
        });

        let result = service.synthesize_utterance(request).await;
        assert!(
            result.is_ok(),
            "non-batched modes must ignore batch_size entirely, zero or otherwise"
        );
    }
}

#[cfg(test)]
mod load_voice_dispatch_tests {
    use super::*;
    use std::io::Write;

    fn write_temp_config(dir: &std::path::Path, name: &str, contents: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(contents.as_bytes()).unwrap();
        path
    }

    #[test]
    fn load_voice_errors_on_a_missing_config_path() {
        let path = std::path::Path::new("/nonexistent-dengjen-grpc-load-voice-test.json");
        assert!(load_voice(path).is_err());
    }

    #[test]
    fn load_voice_routes_kokoro_model_type_toward_the_kokoro_loader() {
        let dir = std::env::temp_dir().join("dengjen_grpc_load_voice_test_kokoro");
        std::fs::create_dir_all(&dir).unwrap();

        let path = write_temp_config(&dir, "config.json", r#"{"model_type": "kokoro"}"#);

        let err = match load_voice(&path) {
            Err(e) => format!("{}", e),
            Ok(_) => panic!("expected an error for an incomplete Kokoro config"),
        };
        assert!(
            err.contains("model_path"),
            "expected a Kokoro-loader error naming the missing `model_path` field, got: {err}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_voice_routes_melotts_model_type_toward_the_melotts_loader() {
        let dir = std::env::temp_dir().join("dengjen_grpc_load_voice_test_melotts");
        std::fs::create_dir_all(&dir).unwrap();

        let path = write_temp_config(
            &dir,
            "config.json",
            r#"{"model_type": "melotts", "audio": {"sample_rate": 24000}}"#,
        );
        let err = match load_voice(&path) {
            Err(e) => format!("{}", e),
            Ok(_) => panic!("expected an error for an incomplete MeloTTS config"),
        };
        assert!(
            err.contains("phonemizer"),
            "expected a MeloTTS-loader error naming the missing `phonemizer` field, got: {err}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}

#[cfg(test)]
mod synth_options_tests {
    use super::*;
    use dengjen_tts_core::{Audio, AudioInfo as CoreAudioInfo, DengjenAudioResult, Phonemes};
    use dengjen_tts_piper::PiperSynthesisConfig;
    use std::collections::HashMap as StdHashMap;
    use std::sync::Mutex as StdMutex;

    struct ConfigurableModel {
        speakers: StdHashMap<i64, String>,
        config: StdMutex<SynthesisConfig>,
    }

    impl DengjenModel for ConfigurableModel {
        fn audio_output_info(&self) -> DengjenResult<CoreAudioInfo> {
            Ok(CoreAudioInfo {
                sample_rate: 22050,
                num_channels: 1,
                sample_width: 2,
            })
        }
        fn phonemize_text(&self, _text: &str) -> DengjenResult<Phonemes> {
            Ok(Phonemes::from(vec!["fake".to_string()]))
        }
        fn speak_batch(&self, _phoneme_batches: Vec<String>) -> DengjenResult<Vec<Audio>> {
            Ok(Vec::new())
        }
        fn speak_one_sentence(&self, _phonemes: String) -> DengjenAudioResult {
            Ok(Audio::new(Default::default(), 22050, None))
        }
        fn get_default_synthesis_config(&self) -> DengjenResult<Option<SynthesisConfig>> {
            Ok(Some(self.config.lock().unwrap().clone()))
        }
        fn get_fallback_synthesis_config(&self) -> DengjenResult<Option<SynthesisConfig>> {
            Ok(Some(self.config.lock().unwrap().clone()))
        }
        fn set_fallback_synthesis_config(&self, c: &SynthesisConfig) -> DengjenResult<()> {
            *self.config.lock().unwrap() = c.clone();
            Ok(())
        }
        fn get_speakers(&self) -> DengjenResult<Option<&StdHashMap<i64, String>>> {
            Ok(Some(&self.speakers))
        }
    }

    fn service_with_voice(voice_key: &str, model: ConfigurableModel) -> DengjenGrpcService {
        let service = DengjenGrpcService::new();
        let voice = Voice::new(Arc::new(model)).unwrap();
        service
            .0
            .write()
            .unwrap()
            .insert(voice_key.to_string(), voice);
        service
    }

    struct ConfiglessModel {
        speakers: StdHashMap<i64, String>,
    }

    impl DengjenModel for ConfiglessModel {
        fn audio_output_info(&self) -> DengjenResult<CoreAudioInfo> {
            Ok(CoreAudioInfo {
                sample_rate: 24000,
                num_channels: 1,
                sample_width: 2,
            })
        }
        fn phonemize_text(&self, _text: &str) -> DengjenResult<Phonemes> {
            Ok(Phonemes::from(vec!["fake".to_string()]))
        }
        fn speak_batch(&self, _phoneme_batches: Vec<String>) -> DengjenResult<Vec<Audio>> {
            Ok(Vec::new())
        }
        fn speak_one_sentence(&self, _phonemes: String) -> DengjenAudioResult {
            Ok(Audio::new(Default::default(), 24000, None))
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
        fn get_speakers(&self) -> DengjenResult<Option<&StdHashMap<i64, String>>> {
            Ok(Some(&self.speakers))
        }
    }

    #[test]
    fn synth_options_from_default_config_tolerates_a_backend_with_no_synthesis_config() {
        let model = ConfiglessModel {
            speakers: StdHashMap::new(),
        };
        let opts = DengjenGrpcService::synth_options_from_default_config(&model).unwrap();
        assert_eq!(opts.speaker.as_deref(), Some("Default"));
        assert_eq!(opts.length_scale, None);
        assert_eq!(opts.noise_scale, None);
        assert_eq!(opts.noise_w, None);
        assert!(opts.parameters.is_empty());
    }

    #[test]
    fn synth_options_omit_piper_scales_for_a_config_without_them() {
        let model = ConfiglessModel {
            speakers: StdHashMap::from([(0i64, "Narrator".to_string())]),
        };
        let config = SynthesisConfig {
            speaker: Some(0),
            parameters: StdHashMap::new(),
        };
        let opts =
            DengjenGrpcService::synth_options_from_synthesis_config(&model, &config, || Ok(None))
                .unwrap();
        assert_eq!(opts.speaker.as_deref(), Some("Narrator"));
        assert_eq!(opts.length_scale, None);
        assert_eq!(opts.noise_scale, None);
        assert_eq!(opts.noise_w, None);
    }

    #[test]
    fn synth_options_from_model_tolerates_a_backend_with_no_synthesis_config() {
        let mut speakers = StdHashMap::new();
        speakers.insert(0i64, "Narrator".to_string());
        let model = ConfiglessModel { speakers };
        let opts = DengjenGrpcService::synth_options_from_model(&model).unwrap();
        assert_eq!(opts.speaker.as_deref(), Some("Narrator"));
        assert_eq!(opts.length_scale, None);
        assert_eq!(opts.noise_scale, None);
        assert_eq!(opts.noise_w, None);
        assert!(opts.parameters.is_empty());
    }

    #[test]
    fn build_voice_info_tolerates_a_backend_with_no_synthesis_config() {
        let model = ConfiglessModel {
            speakers: StdHashMap::new(),
        };
        let service = DengjenGrpcService::new();
        let voice = Voice::new(Arc::new(model)).unwrap();
        let info = service
            .build_voice_info("v1".to_string(), voice.model_ref())
            .unwrap();
        let opts = info.synthesis_options.unwrap();
        assert_eq!(opts.speaker.as_deref(), Some("Default"));
        assert_eq!(opts.length_scale, None);
    }

    #[test]
    fn lookup_synth_options_reports_voice_not_found_for_an_unloaded_voice() {
        let service = DengjenGrpcService::new();
        assert!(matches!(
            service.lookup_synth_options("missing"),
            Err(DengjenGrpcError::VoiceNotFound(_))
        ));
    }

    #[test]
    fn apply_synth_options_reports_voice_not_found_for_an_unloaded_voice() {
        let service = DengjenGrpcService::new();
        let opts = grpc::SynthesisSettings {
            speaker: None,
            length_scale: None,
            noise_scale: None,
            noise_w: None,
            parameters: StdHashMap::new(),
        };
        assert!(matches!(
            service.apply_synth_options("missing", opts),
            Err(DengjenGrpcError::VoiceNotFound(_))
        ));
    }

    #[test]
    fn lookup_synth_options_reads_the_models_fallback_config_defaulting_speaker_to_id_zero() {
        let mut speakers = StdHashMap::new();
        speakers.insert(0i64, "Narrator".to_string());
        let model = ConfigurableModel {
            speakers,
            config: StdMutex::new(SynthesisConfig::from(&PiperSynthesisConfig {
                speaker: None,
                noise_scale: 0.5,
                length_scale: 1.0,
                noise_w: 0.2,
            })),
        };
        let service = service_with_voice("v1", model);
        let opts = service.lookup_synth_options("v1").unwrap();
        assert_eq!(opts.speaker.as_deref(), Some("Narrator"));
        assert_eq!(opts.noise_scale, Some(0.5));
        assert_eq!(opts.length_scale, Some(1.0));
        assert_eq!(opts.noise_w, Some(0.2));
    }

    #[test]
    fn apply_synth_options_updates_only_the_provided_fields_and_persists_them() {
        let mut speakers = StdHashMap::new();
        speakers.insert(0i64, "Narrator".to_string());
        speakers.insert(7i64, "Robot".to_string());
        let model = ConfigurableModel {
            speakers,
            config: StdMutex::new(SynthesisConfig::from(&PiperSynthesisConfig {
                speaker: Some(0),
                noise_scale: 0.5,
                length_scale: 1.0,
                noise_w: 0.2,
            })),
        };
        let service = service_with_voice("v1", model);

        let update = grpc::SynthesisSettings {
            speaker: Some("Robot".to_string()),
            length_scale: Some(2.0),
            noise_scale: None,
            noise_w: None,
            parameters: StdHashMap::new(),
        };
        let result = service.apply_synth_options("v1", update).unwrap();
        assert_eq!(result.speaker.as_deref(), Some("Robot"));
        assert_eq!(result.length_scale, Some(2.0));
        assert_eq!(
            result.noise_scale,
            Some(0.5),
            "unset field must be left unchanged"
        );
        assert_eq!(
            result.noise_w,
            Some(0.2),
            "unset field must be left unchanged"
        );

        let persisted = service.lookup_synth_options("v1").unwrap();
        assert_eq!(
            persisted.speaker.as_deref(),
            Some("Robot"),
            "change must persist on the model"
        );
    }

    #[test]
    fn apply_synth_options_ignores_an_unknown_speaker_name() {
        let mut speakers = StdHashMap::new();
        speakers.insert(0i64, "Narrator".to_string());
        let model = ConfigurableModel {
            speakers,
            config: StdMutex::new(SynthesisConfig::from(&PiperSynthesisConfig {
                speaker: Some(0),
                noise_scale: 0.5,
                length_scale: 1.0,
                noise_w: 0.2,
            })),
        };
        let service = service_with_voice("v1", model);
        let update = grpc::SynthesisSettings {
            speaker: Some("NoSuchSpeaker".to_string()),
            length_scale: None,
            noise_scale: None,
            noise_w: None,
            parameters: StdHashMap::new(),
        };
        let result = service.apply_synth_options("v1", update).unwrap();
        assert_eq!(
            result.speaker.as_deref(),
            Some("Narrator"),
            "unknown speaker name must leave the current speaker unchanged"
        );
    }

    #[test]
    fn apply_synth_options_merges_generic_parameters_the_named_fields_dont_cover() {
        let speakers = StdHashMap::from([(0i64, "alice".to_string())]);
        let model = ConfigurableModel {
            speakers,
            config: StdMutex::new(SynthesisConfig::from(&PiperSynthesisConfig {
                speaker: Some(0),
                noise_scale: 0.667,
                length_scale: 1.0,
                noise_w: 0.8,
            })),
        };
        let service = service_with_voice("v1", model);
        let mut parameters = StdHashMap::new();
        parameters.insert("length_scale".to_string(), 2.5);
        let update = grpc::SynthesisSettings {
            speaker: None,
            length_scale: None,
            noise_scale: None,
            noise_w: None,
            parameters,
        };
        let result = service.apply_synth_options("v1", update).unwrap();
        assert_eq!(result.length_scale, Some(2.5));
    }

    #[test]
    fn apply_synth_options_prefers_the_named_field_over_a_conflicting_parameters_key() {
        let speakers = StdHashMap::from([(0i64, "alice".to_string())]);
        let model = ConfigurableModel {
            speakers,
            config: StdMutex::new(SynthesisConfig::from(&PiperSynthesisConfig {
                speaker: Some(0),
                noise_scale: 0.667,
                length_scale: 1.0,
                noise_w: 0.8,
            })),
        };
        let service = service_with_voice("v1", model);
        let mut parameters = StdHashMap::new();
        parameters.insert("length_scale".to_string(), 9.9);
        let update = grpc::SynthesisSettings {
            speaker: None,
            length_scale: Some(3.0),
            noise_scale: None,
            noise_w: None,
            parameters,
        };
        let result = service.apply_synth_options("v1", update).unwrap();
        assert_eq!(result.length_scale, Some(3.0));
    }

    #[test]
    fn lookup_synth_options_echoes_back_a_generic_parameter_not_covered_by_a_named_field() {
        let speakers = StdHashMap::from([(0i64, "alice".to_string())]);
        let mut config = SynthesisConfig::from(&PiperSynthesisConfig {
            speaker: Some(0),
            noise_scale: 0.667,
            length_scale: 1.0,
            noise_w: 0.8,
        });
        config.parameters.insert("custom_knob".to_string(), 1.25);
        let model = ConfigurableModel {
            speakers,
            config: StdMutex::new(config),
        };
        let service = service_with_voice("v1", model);
        let opts = service.lookup_synth_options("v1").unwrap();
        assert_eq!(opts.parameters.get("custom_knob"), Some(&1.25));
    }

    #[test]
    fn lookup_synth_options_does_not_duplicate_named_fields_inside_the_parameters_map() {
        let speakers = StdHashMap::from([(0i64, "alice".to_string())]);
        let model = ConfigurableModel {
            speakers,
            config: StdMutex::new(SynthesisConfig::from(&PiperSynthesisConfig {
                speaker: Some(0),
                noise_scale: 0.667,
                length_scale: 1.0,
                noise_w: 0.8,
            })),
        };
        let service = service_with_voice("v1", model);
        let opts = service.lookup_synth_options("v1").unwrap();
        assert!(
            opts.parameters.is_empty(),
            "the 3 named-field keys must not also appear in the generic map: {:?}",
            opts.parameters
        );
    }

    #[test]
    fn apply_synth_options_preserves_a_previously_set_generic_key_across_a_later_named_field_update(
    ) {
        let speakers = StdHashMap::from([(0i64, "alice".to_string())]);
        let model = ConfigurableModel {
            speakers,
            config: StdMutex::new(SynthesisConfig::from(&PiperSynthesisConfig {
                speaker: Some(0),
                noise_scale: 0.667,
                length_scale: 1.0,
                noise_w: 0.8,
            })),
        };
        let service = service_with_voice("v1", model);

        let mut first_parameters = StdHashMap::new();
        first_parameters.insert("custom_knob".to_string(), 1.25);
        service
            .apply_synth_options(
                "v1",
                grpc::SynthesisSettings {
                    speaker: None,
                    length_scale: None,
                    noise_scale: None,
                    noise_w: None,
                    parameters: first_parameters,
                },
            )
            .unwrap();

        let result = service
            .apply_synth_options(
                "v1",
                grpc::SynthesisSettings {
                    speaker: None,
                    length_scale: Some(2.0),
                    noise_scale: None,
                    noise_w: None,
                    parameters: StdHashMap::new(),
                },
            )
            .unwrap();
        assert_eq!(result.length_scale, Some(2.0));
        assert_eq!(
            result.parameters.get("custom_knob"),
            Some(&1.25),
            "a generic key set by a prior call must survive a later call that doesn't touch it"
        );
    }

    #[test]
    fn set_synthesis_options_no_ops_cleanly_for_a_backend_with_no_synthesis_config() {
        let model = ConfiglessModel {
            speakers: StdHashMap::from([(0i64, "Narrator".to_string())]),
        };
        let service = DengjenGrpcService::new();
        let voice = Voice::new(Arc::new(model)).unwrap();
        service.0.write().unwrap().insert("v1".to_string(), voice);
        let result = service.apply_synth_options(
            "v1",
            grpc::SynthesisSettings {
                speaker: None,
                length_scale: None,
                noise_scale: None,
                noise_w: None,
                parameters: StdHashMap::new(),
            },
        );
        assert!(
            result.is_ok(),
            "SetSynthesisOptions on a configless backend must no-op, not error: {result:?}"
        );
    }
}
