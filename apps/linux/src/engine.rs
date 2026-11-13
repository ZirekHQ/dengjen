//! Loads installed Piper voices and synthesizes one sentence or a whole text.

use std::path::{Path, PathBuf};

use dengjen_tts::{
    AudioOutputConfig, DengjenModel, DengjenSpeechSynthesizer, ModelType, StreamMode,
};

const MODEL_TYPE: &str = "piper";
const VOICES_DIR_ENV: &str = "DENGJEN_VOICES_DIR";
pub const SUGGESTED_VOICE: &str = "ku_TR-berfin_renas-medium";

/// The voices directory `dengjen voices install` writes to.
pub fn voices_dir() -> Option<PathBuf> {
    std::env::var_os(VOICES_DIR_ENV)
        .map(PathBuf::from)
        .or_else(|| dirs::data_dir().map(|dir| dir.join("dengjen").join("voices")))
}

fn config_path(voices_dir: &Path, key: &str) -> PathBuf {
    voices_dir
        .join(MODEL_TYPE)
        .join(key)
        .join(format!("{key}.onnx.json"))
}

/// Installed voice keys, sorted, with Kurmanji voices first.
pub fn installed_voices(voices_dir: &Path) -> Vec<String> {
    let mut keys: Vec<String> = std::fs::read_dir(voices_dir.join(MODEL_TYPE))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|key| config_path(voices_dir, key).is_file())
        .collect();
    keys.sort_by_key(|key| (!key.starts_with("ku"), key.clone()));
    keys
}

/// Speech settings taken from the window when reading starts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Voice {
    pub key: String,
    pub rate: Option<u8>,
    pub pitch: Option<u8>,
}

impl Voice {
    fn output(&self) -> AudioOutputConfig {
        AudioOutputConfig {
            rate: self.rate,
            volume: None,
            pitch: self.pitch,
            appended_silence_ms: None,
        }
    }
}

/// Mono 16-bit samples of one sentence.
pub struct Clip {
    pub sample_rate: u32,
    pub samples: Vec<i16>,
}

pub struct Engine {
    voices_dir: PathBuf,
    loaded: Option<(String, DengjenSpeechSynthesizer)>,
    ort_ready: bool,
}

impl Engine {
    pub fn new(voices_dir: PathBuf) -> Self {
        Self {
            voices_dir,
            loaded: None,
            ort_ready: false,
        }
    }

    fn synthesizer(&mut self, key: &str) -> Result<&DengjenSpeechSynthesizer, String> {
        if !self.ort_ready {
            let environment = ort::init()
                .with_name("dengjen")
                .with_execution_providers([ort::ep::CPU::default().build()])
                .build()
                .map_err(|error| error.to_string())?;
            dengjen_tts_core::install_environment(environment)
                .map_err(|error| error.to_string())?;
            self.ort_ready = true;
        }
        if self.loaded.as_ref().is_none_or(|(loaded, _)| loaded != key) {
            self.loaded = None;
            let model =
                dengjen_tts::load_voice_with(&config_path(&self.voices_dir, key), |kind, path| {
                    match kind {
                        ModelType::Piper => dengjen_tts_piper::from_config_path(path),
                        other => Err(dengjen_tts::DengjenError::InvalidConfiguration(format!(
                            "unsupported model type `{other}`"
                        ))),
                    }
                })
                .map_err(|error| error.to_string())?;
            let synth = DengjenSpeechSynthesizer::new(model).map_err(|error| error.to_string())?;
            self.loaded = Some((key.to_owned(), synth));
        }
        self.loaded
            .as_ref()
            .map(|(_, synth)| synth)
            .ok_or_else(|| "voice not loaded".to_owned())
    }

    /// The whole audio of one sentence.
    pub fn speak(&mut self, voice: &Voice, text: &str) -> Result<Clip, String> {
        let synth = self.synthesizer(&voice.key)?;
        let info = synth
            .audio_output_info()
            .map_err(|error| error.to_string())?;
        if info.num_channels != 1 {
            return Err(format!("unsupported channel count {}", info.num_channels));
        }
        let sample_rate = u32::try_from(info.sample_rate).map_err(|error| error.to_string())?;
        let mut samples = Vec::new();
        for chunk in synth
            .synthesize_samples(text.to_owned(), Some(voice.output()), StreamMode::Lazy)
            .map_err(|error| error.to_string())?
        {
            samples.extend(chunk.map_err(|error| error.to_string())?.to_i16_vec());
        }
        Ok(Clip {
            sample_rate,
            samples,
        })
    }

    /// Writes the whole text to a WAV file.
    pub fn save(&mut self, voice: &Voice, text: &str, path: &Path) -> Result<(), String> {
        self.synthesizer(&voice.key)?
            .synthesize_to_file(path, text.to_owned(), Some(voice.output()))
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_voices_lists_only_complete_voices_kurmanji_first() {
        let dir = std::env::temp_dir().join(format!("dengjen-gtk-{}", std::process::id()));
        for key in ["en_US-lessac-medium", "ku_TR-berfin_renas-medium", "half"] {
            std::fs::create_dir_all(dir.join(MODEL_TYPE).join(key)).unwrap();
        }
        for key in ["en_US-lessac-medium", "ku_TR-berfin_renas-medium"] {
            std::fs::write(config_path(&dir, key), "{}").unwrap();
        }
        assert_eq!(
            installed_voices(&dir),
            ["ku_TR-berfin_renas-medium", "en_US-lessac-medium"]
        );
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(installed_voices(&dir).is_empty());
    }
}
