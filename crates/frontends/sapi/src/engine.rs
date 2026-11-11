//! The real [`Synth`]: a dengjen Piper voice, run in whichever process loaded the DLL.
#![forbid(unsafe_code)]

use std::path::Path;
use std::sync::{Mutex, OnceLock};

use dengjen_tts::{
    AudioOutputConfig, DengjenError, DengjenSpeechSynthesizer, ModelType, StreamMode,
};

use crate::prosody::EngineProsody;
use crate::speak::Synth;

/// ONNX Runtime's DLL, shipped beside ours in the same bitness.
pub const ORT_DLL: &str = "onnxruntime.dll";

static RUNTIME: OnceLock<Result<(), String>> = OnceLock::new();
static RUNTIME_LOCK: Mutex<()> = Mutex::new(());

/// Points eSpeak-ng and ONNX Runtime at the files beside the DLL, once per process.
///
/// The host process is someone else's (Narrator, NVDA, a browser), so nothing may come from its
/// executable's directory, its search path or its environment.
pub fn init_runtime(dll_dir: &Path) -> Result<(), String> {
    let _guard = RUNTIME_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    RUNTIME
        .get_or_init(|| {
            #[cfg(feature = "espeak")]
            dengjen_espeak_phonemizer::set_data_directory(dll_dir.to_path_buf());
            let environment = ort_builder(dll_dir)?
                .with_name("dengjen-sapi")
                .with_execution_providers([ort::ep::CPU::default().build()])
                .build()
                .map_err(|error| error.to_string())?;
            dengjen_tts_core::install_environment(environment).map_err(|error| error.to_string())
        })
        .clone()
}

#[cfg(feature = "ort-dylib")]
fn ort_builder(dll_dir: &Path) -> Result<ort::environment::EnvironmentBuilder, String> {
    let ort_path: std::path::PathBuf = dll_dir.join(ORT_DLL);
    ort::init_from(&ort_path).map_err(|error| {
        format!(
            "failed to load ONNX Runtime from `{}`: {error}",
            ort_path.display()
        )
    })
}

/// Without `ort-dylib`, ONNX Runtime is linked at build time; only for development builds.
#[cfg(not(feature = "ort-dylib"))]
fn ort_builder(_dll_dir: &Path) -> Result<ort::environment::EnvironmentBuilder, String> {
    Ok(ort::init())
}

/// A loaded voice.
pub struct Engine {
    synth: DengjenSpeechSynthesizer,
    sample_rate: u32,
}

impl Engine {
    pub fn load(config_path: &Path) -> Result<Self, String> {
        let model = dengjen_tts::load_voice_with(config_path, |kind, path| match kind {
            ModelType::Piper => dengjen_tts_piper::from_config_path(path),
            other => Err(DengjenError::InvalidConfiguration(format!(
                "unsupported model type `{other}`"
            ))),
        })
        .map_err(|error| error.to_string())?;
        let info = model
            .audio_output_info()
            .map_err(|error| error.to_string())?;
        if info.num_channels != 1 {
            return Err(format!("unsupported channel count {}", info.num_channels));
        }
        let sample_rate = u32::try_from(info.sample_rate)
            .map_err(|_| format!("unsupported sample rate {}", info.sample_rate))?;
        let synth = DengjenSpeechSynthesizer::new(model).map_err(|error| error.to_string())?;
        Ok(Self { synth, sample_rate })
    }
}

impl Synth for Engine {
    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn synthesize(
        &self,
        text: &str,
        prosody: EngineProsody,
        chunk: &mut dyn FnMut(&[i16]) -> bool,
    ) -> Result<(), String> {
        let output = AudioOutputConfig {
            rate: prosody.rate,
            volume: prosody.volume,
            pitch: prosody.pitch,
            appended_silence_ms: None,
        };
        let stream = self
            .synth
            .synthesize_samples(text.to_owned(), Some(output), StreamMode::Lazy)
            .map_err(|error| error.to_string())?;
        for audio in stream {
            let samples = audio.map_err(|error| error.to_string())?.to_i16_vec();
            if !chunk(&samples) {
                break;
            }
        }
        Ok(())
    }

    fn interrupt(&self) {
        self.synth.interrupt();
    }
}
