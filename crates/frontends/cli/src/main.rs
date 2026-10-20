#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)
)]

use clap::Parser;
use dengjen_tts::{
    default_batch_size, AudioOutputConfig, AudioSamples, CancellationToken, DengjenModel,
    DengjenResult, DengjenSpeechSynthesizer, StreamMode, SynthesisConfig,
};
#[cfg(test)]
use dengjen_tts_piper::PiperSynthesisConfig;
use serde::Deserialize;
use std::fs::File;
use std::io::{self, prelude::*};
use std::path::PathBuf;

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
enum SynthesisMode {
    #[default]
    Lazy,
    Parallel,
    Batched,
    Realtime,
}

impl std::str::FromStr for SynthesisMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let normalized = s.to_lowercase();
        match normalized.as_str() {
            "lazy" => Ok(Self::Lazy),
            "parallel" => Ok(Self::Parallel),
            "batched" => Ok(Self::Batched),
            "realtime" => Ok(Self::Realtime),
            _ => Err(format!("Unknown synthesis mode: `{}`", s)),
        }
    }
}

fn parse_param(s: &str) -> Result<(String, f32), String> {
    let (key, value) = s
        .split_once('=')
        .ok_or_else(|| format!("expected KEY=VALUE, got `{s}`"))?;
    let value: f32 = value
        .parse()
        .map_err(|_| format!("`{value}` is not a valid float"))?;
    Ok((key.to_string(), value))
}

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Model config
    config: PathBuf,
    /// Input text file (default `stdin`)
    #[arg(short = 'f', long, value_name = "INPUT_FILE")]
    input_file: Option<PathBuf>,
    /// Output file (default `stdout`)
    #[arg(short, long, value_name = "OUTPUT_FILE")]
    output_file: Option<PathBuf>,
    /// Synthesis mode (default `Lazy`)
    #[arg(long)]
    mode: Option<SynthesisMode>,
    /// Speaker ID for multi-speaker models (default: the voice's own default speaker)
    #[arg(long)]
    speaker_id: Option<u32>,
    /// Length scale override, used by Piper and MeloTTS (default: the voice's config value)
    #[arg(long)]
    length_scale: Option<f32>,
    /// Noise scale override, used by Piper and MeloTTS (default: the voice's config value)
    #[arg(long)]
    noise_scale: Option<f32>,
    /// Piper noise width (default `model_default from config file`)
    #[arg(long)]
    noise_w: Option<f32>,
    /// Speaking rate [0 - 100], mapped linearly to 0.5x-5.5x speed; 10 is 1x, 50 is 3x (default: 1x)
    #[arg(long)]
    rate: Option<u8>,
    /// Speech pitch [0 - 100], mapped to 0.5-1.5; 50 is neutral (default: 1.0, no change)
    #[arg(long)]
    pitch: Option<u8>,
    /// Speech volume [0 - 100], mapped to 0-1 gain; 0 mutes (default: 1.0, no change)
    #[arg(long)]
    volume: Option<u8>,
    /// Extra silence (in milliseconds) to append to the end of each sentence (default `0`)
    #[arg(long)]
    silence: Option<u32>,
    /// Chunk granularity to stream (unit is backend-specific; e.g. Piper mel frames)
    #[arg(long)]
    chunk_size: Option<usize>,
    /// Number of mel frames to use for padding current chunk (improves naturalness)
    #[arg(long)]
    chunk_padding: Option<usize>,
    /// Number of sentences to synthesize per concurrent group in `batched`
    /// mode (default: min(available CPU cores, 4))
    #[arg(long)]
    batch_size: Option<usize>,
    /// Named synthesis parameter, repeatable (e.g. --param custom_knob=1.25).
    /// A named flag (e.g. --length-scale) wins over a conflicting key here; an
    /// unrecognized key may be silently ignored by the current backend.
    #[arg(long, value_parser = parse_param)]
    param: Vec<(String, f32)>,
}

#[derive(Deserialize, Default)]
struct SynthesisRequest {
    text: String,
    mode: Option<SynthesisMode>,
    speaker_id: Option<u32>,
    length_scale: Option<f32>,
    noise_scale: Option<f32>,
    noise_w: Option<f32>,
    rate: Option<u8>,
    pitch: Option<u8>,
    volume: Option<u8>,
    appended_silence_ms: Option<u32>,
    chunk_size: Option<usize>,
    chunk_padding: Option<usize>,
    batch_size: Option<usize>,
    #[serde(default)]
    parameters: Vec<(String, f32)>,
}

impl SynthesisRequest {
    fn as_synthesis_config(&self, default_config: &SynthesisConfig) -> SynthesisConfig {
        let mut config = default_config.clone();
        for (key, value) in &self.parameters {
            config.parameters.insert(key.clone(), *value);
        }
        if let Some(length_scale) = self.length_scale {
            config.parameters.insert(
                dengjen_tts_piper::synth_config::LENGTH_SCALE.to_string(),
                length_scale,
            );
        }
        if let Some(noise_scale) = self.noise_scale {
            config.parameters.insert(
                dengjen_tts_piper::synth_config::NOISE_SCALE.to_string(),
                noise_scale,
            );
        }
        if let Some(noise_w) = self.noise_w {
            config.parameters.insert(
                dengjen_tts_piper::synth_config::NOISE_W.to_string(),
                noise_w,
            );
        }
        if let Some(speaker_id) = self.speaker_id {
            config.speaker = Some(i64::from(speaker_id));
        }
        config
    }

    fn as_audio_output_config(&self) -> AudioOutputConfig {
        AudioOutputConfig {
            rate: self.rate,
            pitch: self.pitch,
            volume: self.volume,
            appended_silence_ms: self.appended_silence_ms,
        }
    }
}

fn enable_logging() -> anyhow::Result<()> {
    // `fmt().init()` installs the LogTracer bridge itself (tracing-subscriber's
    // default "tracing-log" feature) — a separate explicit `LogTracer::init()` call
    // here would double-install the `log` backend and panic with `SetLoggerError`.
    let filter = match std::env::var("DENGJEN_LOG") {
        Ok(directives) => {
            tracing_subscriber::EnvFilter::try_new(&directives).unwrap_or_else(|err| {
                eprintln!(
                    "warning: ignoring malformed DENGJEN_LOG ({err}); defaulting to \"info\""
                );
                tracing_subscriber::EnvFilter::new("info")
            })
        }
        Err(_) => tracing_subscriber::EnvFilter::new("info"),
    };
    // stdout carries synthesized PCM/WAV bytes (see `consume_stream`); logs must go to
    // stderr, matching the `env_logger` default this replaces, or they corrupt the audio
    // stream.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
        .with_env_filter(filter)
        .try_init()
        .map_err(anyhow::Error::from_boxed)?;
    Ok(())
}

fn read_synthesis_request<R: BufRead>(reader: &mut R) -> anyhow::Result<Option<SynthesisRequest>> {
    let mut line = String::new();
    match reader.read_line(&mut line) {
        Ok(0) => return Ok(None),
        Ok(_) => {}
        Err(err) => {
            log::error!("Failed to read from stdin: {}", err);
            return Ok(None);
        }
    }
    Ok(Some(serde_json::from_str(&line)?))
}

fn get_synthesis_request_from_stdin() -> anyhow::Result<Option<SynthesisRequest>> {
    read_synthesis_request(&mut io::stdin().lock())
}

fn process_synthesis_request<W: Write>(
    args: &Cli,
    synth: &DengjenSpeechSynthesizer,
    default_synth_config: &SynthesisConfig,
    req: SynthesisRequest,
    writer: &mut W,
) -> anyhow::Result<()> {
    let synthesis_config = req.as_synthesis_config(default_synth_config);
    synth.set_fallback_synthesis_config(&synthesis_config)?;
    let output_config = Some(req.as_audio_output_config());

    if let Some(output_file) = &args.output_file {
        if req.mode.is_some() {
            log::warn!("Synthesis mode has no effect when output-file is set");
        }
        return synth
            .synthesize_to_file(output_file, req.text, output_config)
            .map_err(anyhow::Error::from);
    }

    let mode = match req.mode.unwrap_or_default() {
        SynthesisMode::Lazy => StreamMode::Lazy,
        SynthesisMode::Parallel => StreamMode::Parallel,
        SynthesisMode::Batched => StreamMode::Batched {
            batch_size: match req.batch_size {
                None => default_batch_size(),
                Some(n) => std::num::NonZeroUsize::new(n)
                    .ok_or_else(|| anyhow::anyhow!("batch_size must be greater than zero"))?,
            },
        },
        SynthesisMode::Realtime => StreamMode::Realtime {
            chunk_size: req.chunk_size.unwrap_or(100),
            chunk_padding: req.chunk_padding.unwrap_or(3),
            cancel_token: CancellationToken::new(),
        },
    };
    let samples = synth.synthesize_samples(req.text, output_config, mode)?;
    consume_stream(samples, writer)
}

fn consume_stream(
    stream: impl Iterator<Item = DengjenResult<AudioSamples>>,
    writer: &mut impl Write,
) -> anyhow::Result<()> {
    for chunk in stream {
        let audio = chunk?;
        writer.write_all(&audio.as_wave_bytes())?;
        writer.flush()?;
    }
    Ok(())
}

#[cfg(test)]
mod synthesis_processing_tests {
    use super::*;
    use dengjen_tts::{
        Audio, AudioInfo, AudioStreamIterator, DengjenAudioResult, DengjenError, Phonemes,
    };
    use std::collections::HashMap;
    use std::sync::Mutex;

    struct FakeDengjenModel {
        speakers: HashMap<i64, String>,
        fallback_config: Mutex<PiperSynthesisConfig>,
        fail_speak: bool,
    }

    impl FakeDengjenModel {
        fn new() -> Self {
            Self::with_failure(false)
        }

        fn failing() -> Self {
            Self::with_failure(true)
        }

        fn with_failure(fail_speak: bool) -> Self {
            Self {
                speakers: HashMap::from([(0, "alice".to_string())]),
                fallback_config: Mutex::new(PiperSynthesisConfig {
                    speaker: Some(0),
                    noise_scale: 0.667,
                    length_scale: 1.0,
                    noise_w: 0.8,
                }),
                fail_speak,
            }
        }
    }

    impl DengjenModel for FakeDengjenModel {
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
            if self.fail_speak {
                return Err(DengjenError::OperationError("synthesis failed".to_string()));
            }
            Ok(Audio::new(
                AudioSamples::new(vec![0.0, 0.25, -0.25, 0.5]),
                22050,
                Some(1.0),
            ))
        }
        fn get_default_synthesis_config(&self) -> DengjenResult<Option<SynthesisConfig>> {
            Ok(Some(SynthesisConfig::from(
                &*self.fallback_config.lock().unwrap(),
            )))
        }
        fn get_fallback_synthesis_config(&self) -> DengjenResult<Option<SynthesisConfig>> {
            Ok(Some(SynthesisConfig::from(
                &*self.fallback_config.lock().unwrap(),
            )))
        }
        fn set_fallback_synthesis_config(
            &self,
            synthesis_config: &SynthesisConfig,
        ) -> DengjenResult<()> {
            *self.fallback_config.lock().unwrap() = PiperSynthesisConfig::from(synthesis_config);
            Ok(())
        }
        fn get_speakers(&self) -> DengjenResult<Option<&HashMap<i64, String>>> {
            Ok(Some(&self.speakers))
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
            // `DengjenModel::stream_synthesis` defaults to `Err(UnsupportedOperation)`;
            // `RealtimeSpeechStream` calls it unconditionally, so without this override
            // the Realtime-mode test below would fail before reaching `consume_stream`.
            if self.fail_speak {
                return Err(DengjenError::OperationError("synthesis failed".to_string()));
            }
            Ok(Box::new(std::iter::once(Ok(AudioSamples::new(vec![
                0.0, 0.25, -0.25, 0.5,
            ])))))
        }
    }

    fn fake_synth() -> DengjenSpeechSynthesizer {
        DengjenSpeechSynthesizer::new(std::sync::Arc::new(FakeDengjenModel::new())).unwrap()
    }

    fn failing_synth() -> DengjenSpeechSynthesizer {
        DengjenSpeechSynthesizer::new(std::sync::Arc::new(FakeDengjenModel::failing())).unwrap()
    }

    fn default_config() -> SynthesisConfig {
        SynthesisConfig::default()
    }

    fn cli_with_output_file(path: Option<std::path::PathBuf>) -> Cli {
        Cli {
            config: std::path::PathBuf::new(),
            input_file: None,
            output_file: path,
            mode: None,
            speaker_id: None,
            length_scale: None,
            noise_scale: None,
            noise_w: None,
            rate: None,
            pitch: None,
            volume: None,
            silence: None,
            chunk_size: None,
            chunk_padding: None,
            batch_size: None,
            param: Vec::new(),
        }
    }

    #[test]
    fn process_synthesis_request_lazy_mode_writes_pcm_bytes_to_the_writer() {
        let synth = fake_synth();
        let args = cli_with_output_file(None);
        let req = SynthesisRequest {
            text: "hello".to_string(),
            mode: Some(SynthesisMode::Lazy),
            ..Default::default()
        };
        let mut buffer: Vec<u8> = Vec::new();

        process_synthesis_request(&args, &synth, &default_config(), req, &mut buffer).unwrap();

        // Not an exact byte count: output always passes through
        // `AudioOutputConfig::apply` (the Sonic port even with all fields `None`),
        // which is `dengjen-audio-ops`'s own tested responsibility.
        assert!(!buffer.is_empty());
    }

    #[test]
    fn process_synthesis_request_parallel_mode_writes_pcm_bytes_to_the_writer() {
        let synth = fake_synth();
        let args = cli_with_output_file(None);
        let req = SynthesisRequest {
            text: "hello".to_string(),
            mode: Some(SynthesisMode::Parallel),
            ..Default::default()
        };
        let mut buffer: Vec<u8> = Vec::new();

        process_synthesis_request(&args, &synth, &default_config(), req, &mut buffer).unwrap();

        assert!(!buffer.is_empty());
    }

    #[test]
    fn process_synthesis_request_realtime_mode_writes_pcm_bytes_to_the_writer() {
        let synth = fake_synth();
        let args = cli_with_output_file(None);
        let req = SynthesisRequest {
            text: "hello".to_string(),
            mode: Some(SynthesisMode::Realtime),
            ..Default::default()
        };
        let mut buffer: Vec<u8> = Vec::new();

        process_synthesis_request(&args, &synth, &default_config(), req, &mut buffer).unwrap();

        assert!(!buffer.is_empty());
    }

    #[test]
    fn process_synthesis_request_batched_mode_writes_pcm_bytes_to_the_writer() {
        let synth = fake_synth();
        let args = cli_with_output_file(None);
        let req = SynthesisRequest {
            text: "hello".to_string(),
            mode: Some(SynthesisMode::Batched),
            batch_size: Some(2),
            ..Default::default()
        };
        let mut buffer: Vec<u8> = Vec::new();

        process_synthesis_request(&args, &synth, &default_config(), req, &mut buffer).unwrap();

        assert!(!buffer.is_empty());
    }

    #[test]
    fn process_synthesis_request_batched_mode_falls_back_to_the_default_batch_size_when_unset() {
        let synth = fake_synth();
        let args = cli_with_output_file(None);
        let req = SynthesisRequest {
            text: "hello".to_string(),
            mode: Some(SynthesisMode::Batched),
            ..Default::default()
        };
        let mut buffer: Vec<u8> = Vec::new();

        process_synthesis_request(&args, &synth, &default_config(), req, &mut buffer).unwrap();

        assert!(!buffer.is_empty());
    }

    #[test]
    fn process_synthesis_request_rejects_a_zero_batch_size_instead_of_silently_redefaulting() {
        let synth = fake_synth();
        let args = cli_with_output_file(None);
        let req = SynthesisRequest {
            text: "hello".to_string(),
            mode: Some(SynthesisMode::Batched),
            batch_size: Some(0),
            ..Default::default()
        };
        let mut buffer: Vec<u8> = Vec::new();

        let result = process_synthesis_request(&args, &synth, &default_config(), req, &mut buffer);

        assert!(result.is_err());
        assert!(buffer.is_empty());
    }

    #[test]
    fn process_synthesis_request_defaults_to_lazy_mode_when_unset() {
        let synth = fake_synth();
        let args = cli_with_output_file(None);
        let req = SynthesisRequest {
            text: "hello".to_string(),
            ..Default::default()
        };
        let mut buffer: Vec<u8> = Vec::new();

        process_synthesis_request(&args, &synth, &default_config(), req, &mut buffer).unwrap();

        assert!(!buffer.is_empty());
    }

    #[test]
    fn process_synthesis_request_propagates_a_synthesis_error() {
        let synth = failing_synth();
        let args = cli_with_output_file(None);
        let req = SynthesisRequest {
            text: "hello".to_string(),
            mode: Some(SynthesisMode::Lazy),
            ..Default::default()
        };
        let mut buffer: Vec<u8> = Vec::new();

        let result = process_synthesis_request(&args, &synth, &default_config(), req, &mut buffer);

        assert!(result.is_err());
    }

    #[test]
    fn process_synthesis_request_writes_to_the_output_file_when_set_instead_of_the_writer() {
        let synth = fake_synth();
        let dir = std::env::temp_dir().join(format!("dengjen-tts-cli-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("process_synthesis_request_writes_to_the_output_file.wav");
        let args = cli_with_output_file(Some(path.clone()));
        let req = SynthesisRequest {
            text: "hello".to_string(),
            ..Default::default()
        };
        let mut buffer: Vec<u8> = Vec::new();

        process_synthesis_request(&args, &synth, &default_config(), req, &mut buffer).unwrap();

        assert!(path.exists());
        assert!(
            buffer.is_empty(),
            "output-file mode must not also write to the writer"
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn process_synthesis_request_warns_but_still_writes_to_file_when_mode_is_also_set() {
        let synth = fake_synth();
        let dir = std::env::temp_dir().join(format!("dengjen-tts-cli-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("process_synthesis_request_warns_but_still_writes_to_file.wav");
        let args = cli_with_output_file(Some(path.clone()));
        let req = SynthesisRequest {
            text: "hello".to_string(),
            mode: Some(SynthesisMode::Lazy),
            ..Default::default()
        };
        let mut buffer: Vec<u8> = Vec::new();

        process_synthesis_request(&args, &synth, &default_config(), req, &mut buffer).unwrap();

        assert!(path.exists());
        assert!(
            buffer.is_empty(),
            "output-file mode must not also write to the writer, even when mode is set"
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_default_parameter_not_covered_by_any_named_field_survives_into_the_synthesized_request() {
        let mut default_config = SynthesisConfig::default();
        default_config
            .parameters
            .insert("custom_knob".to_string(), 4.5);
        let req = SynthesisRequest {
            text: "hello".to_string(),
            ..Default::default()
        };
        let result = req.as_synthesis_config(&default_config);
        assert_eq!(result.parameters.get("custom_knob"), Some(&4.5));
    }

    #[test]
    fn consume_stream_stops_at_the_first_error_without_writing_further_chunks() {
        let stream: Vec<DengjenResult<AudioSamples>> = vec![
            Ok(AudioSamples::new(vec![0.0, 0.5])),
            Err(DengjenError::OperationError("boom".to_string())),
            Ok(AudioSamples::new(vec![0.0, 0.5])),
        ];
        let mut buffer: Vec<u8> = Vec::new();

        let result = consume_stream(stream.into_iter(), &mut buffer);

        assert!(result.is_err());
        assert_eq!(buffer.len(), 2 * 2);
    }
}

fn init_ort_environment() -> anyhow::Result<()> {
    let providers = [
        #[cfg(feature = "cuda")]
        ort::ep::CUDA::default().build(),
        ort::ep::CPU::default().build(),
    ];
    let environment = ort::init()
        .with_name("dengjen")
        .with_execution_providers(providers)
        .build()?;
    dengjen_tts_core::install_environment(environment)?;
    Ok(())
}

fn load_voice(
    config_path: &std::path::Path,
) -> anyhow::Result<std::sync::Arc<dyn dengjen_tts::DengjenModel + Send + Sync>> {
    Ok(dengjen_tts::load_voice_with(
        config_path,
        |model_type, path| match model_type {
            dengjen_tts::ModelType::Piper => dengjen_tts_piper::from_config_path(path),
            dengjen_tts::ModelType::Kokoro => dengjen_tts_kokoro::from_config_path(path),
            dengjen_tts::ModelType::MeloTts => dengjen_tts_melotts::from_config_path(path),
            other => Err(dengjen_tts::DengjenError::InvalidConfiguration(format!(
                "unsupported model type `{other}`"
            ))),
        },
    )?)
}

fn main() -> anyhow::Result<()> {
    enable_logging()?;
    init_ort_environment()?;

    let mut cli = Cli::parse();
    let synthesizer = build_synthesizer(&cli.config)?;
    let default_synth_config = resolve_default_synthesis_config(&synthesizer)?;

    if let Some(input_path) = cli.input_file.take() {
        return synthesize_from_file(&cli, &input_path, &synthesizer, &default_synth_config);
    }
    synthesize_from_stdin_forever(cli, &synthesizer, &default_synth_config)
}

fn build_synthesizer(config_path: &std::path::Path) -> anyhow::Result<DengjenSpeechSynthesizer> {
    let voice = load_voice(config_path)?;
    let synthesizer = DengjenSpeechSynthesizer::new(voice)?;
    log::info!("Using model config: `{}`", config_path.display());
    Ok(synthesizer)
}

fn resolve_default_synthesis_config(
    synthesizer: &DengjenSpeechSynthesizer,
) -> anyhow::Result<SynthesisConfig> {
    Ok(synthesizer
        .get_default_synthesis_config()?
        .unwrap_or_default())
}

fn synthesis_request_from_cli(cli: &Cli, text: String) -> SynthesisRequest {
    SynthesisRequest {
        text,
        mode: cli.mode.clone(),
        speaker_id: cli.speaker_id,
        length_scale: cli.length_scale,
        noise_scale: cli.noise_scale,
        noise_w: cli.noise_w,
        rate: cli.rate,
        pitch: cli.pitch,
        volume: cli.volume,
        appended_silence_ms: cli.silence,
        chunk_size: cli.chunk_size,
        chunk_padding: cli.chunk_padding,
        batch_size: cli.batch_size,
        parameters: cli.param.clone(),
    }
}

fn synthesize_from_file(
    cli: &Cli,
    input_path: &std::path::Path,
    synthesizer: &DengjenSpeechSynthesizer,
    default_synth_config: &SynthesisConfig,
) -> anyhow::Result<()> {
    let mut file = File::open(input_path)?;
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    let request = synthesis_request_from_cli(cli, text);
    process_synthesis_request(
        cli,
        synthesizer,
        default_synth_config,
        request,
        &mut io::stdout().lock(),
    )
}

fn synthesize_from_stdin_forever(
    mut cli: Cli,
    synthesizer: &DengjenSpeechSynthesizer,
    default_synth_config: &SynthesisConfig,
) -> anyhow::Result<()> {
    let base_output_file = cli.output_file.take();
    let mut request_count: u64 = 0;
    loop {
        request_count += 1;
        cli.output_file = base_output_file
            .clone()
            .map(|path| enumerate_output_path(path, request_count));

        match get_synthesis_request_from_stdin() {
            Ok(Some(request)) => {
                process_synthesis_request(
                    &cli,
                    synthesizer,
                    default_synth_config,
                    request,
                    &mut io::stdout().lock(),
                )?;
                if let Some(output_path) = &cli.output_file {
                    log::info!("Wrote output to file: {}", output_path.display());
                }
            }
            Ok(None) => {
                log::info!("stdin closed, exiting");
                return Ok(());
            }
            Err(err) => log::error!("Invalid json input. Error: {}", err),
        }
    }
}

fn enumerate_output_path(path: PathBuf, suffix: u64) -> PathBuf {
    let Some(stem) = path.file_stem() else {
        return path;
    };
    let mut name = stem.to_os_string();
    name.push(format!("-{suffix}"));
    if let Some(extension) = path.extension() {
        name.push(".");
        name.push(extension);
    }
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    struct FailingReader;

    impl Read for FailingReader {
        fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("simulated I/O failure"))
        }
    }

    #[test]
    fn read_synthesis_request_returns_none_on_io_error_instead_of_looping_forever() {
        let mut reader = io::BufReader::new(FailingReader);
        let result = read_synthesis_request(&mut reader);
        assert!(matches!(result, Ok(None)));
    }

    #[test]
    fn synthesis_mode_from_str_parses_known_values_case_insensitively() {
        assert!(matches!(
            SynthesisMode::from_str("Lazy"),
            Ok(SynthesisMode::Lazy)
        ));
        assert!(matches!(
            SynthesisMode::from_str("PARALLEL"),
            Ok(SynthesisMode::Parallel)
        ));
        assert!(matches!(
            SynthesisMode::from_str("realtime"),
            Ok(SynthesisMode::Realtime)
        ));
        assert!(matches!(
            SynthesisMode::from_str("batched"),
            Ok(SynthesisMode::Batched)
        ));
    }

    #[test]
    fn synthesis_mode_from_str_returns_an_error_instead_of_panicking_on_unknown_value() {
        assert!(SynthesisMode::from_str("bogus").is_err());
    }

    #[test]
    fn as_synthesis_config_falls_back_to_defaults_when_fields_are_none() {
        let default_config = SynthesisConfig::from(&PiperSynthesisConfig {
            speaker: Some(0),
            length_scale: 1.0,
            noise_scale: 0.667,
            noise_w: 0.8,
        });
        let req = SynthesisRequest {
            text: "hello".to_string(),
            ..Default::default()
        };
        let result = req.as_synthesis_config(&default_config);
        assert_eq!(result.speaker, Some(0));
        assert_eq!(result.parameters.get("length_scale"), Some(&1.0));
        assert_eq!(result.parameters.get("noise_scale"), Some(&0.667));
        assert_eq!(result.parameters.get("noise_w"), Some(&0.8));
    }

    #[test]
    fn as_synthesis_config_overrides_defaults_when_fields_are_set() {
        let default_config = SynthesisConfig::from(&PiperSynthesisConfig::default());
        let req = SynthesisRequest {
            text: "hello".to_string(),
            speaker_id: Some(3),
            length_scale: Some(2.0),
            ..Default::default()
        };
        let result = req.as_synthesis_config(&default_config);
        assert_eq!(result.speaker, Some(3));
        assert_eq!(result.parameters.get("length_scale"), Some(&2.0));
    }

    #[test]
    fn synthesis_request_parameters_flow_into_the_generic_synthesis_config() {
        let default_config = SynthesisConfig::from(&PiperSynthesisConfig::default());
        let req = SynthesisRequest {
            text: "hello".to_string(),
            parameters: vec![("custom_knob".to_string(), 1.25)],
            ..Default::default()
        };
        let synthesis_config = req.as_synthesis_config(&default_config);
        assert_eq!(synthesis_config.parameters.get("custom_knob"), Some(&1.25));
    }

    #[test]
    fn as_synthesis_config_merges_generic_parameters_the_named_fields_dont_cover() {
        let default_config = SynthesisConfig::from(&PiperSynthesisConfig {
            speaker: Some(0),
            length_scale: 1.0,
            noise_scale: 0.667,
            noise_w: 0.8,
        });
        let req = SynthesisRequest {
            text: "hello".to_string(),
            parameters: vec![("length_scale".to_string(), 2.5)],
            ..Default::default()
        };
        let result = req.as_synthesis_config(&default_config);
        assert_eq!(result.parameters.get("length_scale"), Some(&2.5));
    }

    #[test]
    fn as_synthesis_config_prefers_the_named_field_over_a_conflicting_parameters_key() {
        let default_config = SynthesisConfig::from(&PiperSynthesisConfig {
            speaker: Some(0),
            length_scale: 1.0,
            noise_scale: 0.667,
            noise_w: 0.8,
        });
        let req = SynthesisRequest {
            text: "hello".to_string(),
            length_scale: Some(3.0),
            parameters: vec![("length_scale".to_string(), 9.9)],
            ..Default::default()
        };
        let result = req.as_synthesis_config(&default_config);
        assert_eq!(
            result.parameters.get("length_scale"),
            Some(&3.0),
            "named field must win over a conflicting parameters key"
        );
    }

    #[test]
    fn as_audio_output_config_carries_over_all_fields() {
        let req = SynthesisRequest {
            text: "hello".to_string(),
            rate: Some(80),
            pitch: Some(40),
            volume: Some(90),
            appended_silence_ms: Some(200),
            ..Default::default()
        };
        let config = req.as_audio_output_config();
        assert_eq!(config.rate, Some(80));
        assert_eq!(config.pitch, Some(40));
        assert_eq!(config.volume, Some(90));
        assert_eq!(config.appended_silence_ms, Some(200));
    }

    #[test]
    fn enumerate_output_path_inserts_the_suffix_before_the_extension() {
        let result = enumerate_output_path(PathBuf::from("out.wav"), 1);
        assert_eq!(result, PathBuf::from("out-1.wav"));
    }

    #[test]
    fn enumerate_output_path_derives_each_name_from_the_base_path() {
        let base = PathBuf::from("out.wav");
        assert_eq!(
            enumerate_output_path(base.clone(), 1),
            PathBuf::from("out-1.wav")
        );
        assert_eq!(enumerate_output_path(base, 2), PathBuf::from("out-2.wav"));
    }

    #[test]
    fn enumerate_output_path_handles_a_path_with_no_extension() {
        assert_eq!(
            enumerate_output_path(PathBuf::from("dir/noext"), 3),
            PathBuf::from("dir/noext-3")
        );
    }

    #[test]
    fn enumerate_output_path_leaves_a_path_without_a_file_name_unchanged() {
        assert_eq!(
            enumerate_output_path(PathBuf::from("/"), 1),
            PathBuf::from("/")
        );
    }
}
