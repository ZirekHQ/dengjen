use std::path::{Path, PathBuf};

use dengjen_speechd::{Audio, Backend, Chunks, Settings, VoiceInfo};
use dengjen_tts::{
    AudioOutputConfig, DengjenModel, DengjenSpeechSynthesizer, ModelType, StreamMode,
};

const MODEL_TYPE: &str = "piper";
const VOICES_DIR_ENV: &str = "DENGJEN_VOICES_DIR";

pub struct Engine {
    voices_dir: PathBuf,
    loaded: Option<(String, DengjenSpeechSynthesizer)>,
    ort_ready: bool,
}

pub fn voices_dir() -> Result<PathBuf, String> {
    std::env::var_os(VOICES_DIR_ENV)
        .map(PathBuf::from)
        .or_else(|| dirs::data_dir().map(|dir| dir.join("dengjen").join("voices")))
        .ok_or_else(|| "cannot locate a data directory; set DENGJEN_VOICES_DIR".to_owned())
}

fn config_path(voices_dir: &Path, key: &str) -> PathBuf {
    voices_dir
        .join(MODEL_TYPE)
        .join(key)
        .join(format!("{key}.onnx.json"))
}

fn installed_keys(voices_dir: &Path) -> Vec<String> {
    let mut keys: Vec<String> = std::fs::read_dir(voices_dir.join(MODEL_TYPE))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|key| config_path(voices_dir, key).is_file())
        .collect();
    keys.sort();
    keys
}

fn language_of(key: &str) -> String {
    key.split('-').next().unwrap_or(key).replace('_', "-")
}

fn init_ort() -> Result<(), String> {
    let environment = ort::init()
        .with_name("dengjen")
        .with_execution_providers([ort::ep::CPU::default().build()])
        .build()
        .map_err(|error| error.to_string())?;
    dengjen_tts_core::install_environment(environment).map_err(|error| error.to_string())
}

fn load(voices_dir: &Path, key: &str) -> Result<DengjenSpeechSynthesizer, String> {
    let model =
        dengjen_tts::load_voice_with(&config_path(voices_dir, key), |kind, path| match kind {
            ModelType::Piper => dengjen_tts_piper::from_config_path(path),
            other => Err(dengjen_tts::DengjenError::InvalidConfiguration(format!(
                "unsupported model type `{other}`"
            ))),
        })
        .map_err(|error| error.to_string())?;
    DengjenSpeechSynthesizer::new(model).map_err(|error| error.to_string())
}

fn pick_voice(keys: &[String], settings: &Settings) -> Option<String> {
    let named = settings
        .synthesis_voice
        .as_ref()
        .filter(|voice| keys.contains(voice));
    let by_language = settings.language.as_ref().and_then(|wanted| {
        keys.iter().find(|key| {
            language_of(key).eq_ignore_ascii_case(wanted) || key.starts_with(wanted.as_str())
        })
    });
    named.or(by_language).or(keys.first()).cloned()
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
            init_ort()?;
            self.ort_ready = true;
        }
        if self.loaded.as_ref().is_none_or(|(loaded, _)| loaded != key) {
            self.loaded = Some((key.to_owned(), load(&self.voices_dir, key)?));
        }
        self.loaded
            .as_ref()
            .map(|(_, synth)| synth)
            .ok_or_else(|| "voice not loaded".to_owned())
    }
}

impl Backend for Engine {
    fn init(&mut self) -> Result<String, String> {
        match installed_keys(&self.voices_dir).len() {
            0 => Err(format!(
                "no voices installed in {}",
                self.voices_dir.display()
            )),
            count => Ok(format!(
                "dengjen: {count} voice(s) in {}",
                self.voices_dir.display()
            )),
        }
    }

    fn voices(&self) -> Vec<VoiceInfo> {
        installed_keys(&self.voices_dir)
            .into_iter()
            .map(|key| VoiceInfo {
                language: language_of(&key),
                name: key,
                variant: "none".to_owned(),
            })
            .collect()
    }

    fn synthesize<'a>(&'a mut self, text: &str, settings: &Settings) -> Result<Chunks<'a>, String> {
        let key =
            pick_voice(&installed_keys(&self.voices_dir), settings).ok_or("no voices installed")?;
        let synth = self.synthesizer(&key)?;
        let info = synth
            .audio_output_info()
            .map_err(|error| error.to_string())?;
        if info.num_channels != 1 {
            return Err(format!("unsupported channel count {}", info.num_channels));
        }
        let sample_rate = info.sample_rate as u32;
        let output = AudioOutputConfig {
            rate: settings.engine_rate(),
            volume: settings.engine_volume(),
            pitch: settings.engine_pitch(),
            appended_silence_ms: None,
        };
        let stream = synth
            .synthesize_samples(text.to_owned(), Some(output), StreamMode::Lazy)
            .map_err(|error| error.to_string())?;
        Ok(Box::new(stream.map(move |chunk| {
            chunk
                .map(|audio| Audio {
                    sample_rate,
                    samples: audio.to_i16_vec(),
                })
                .map_err(|error| error.to_string())
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn language_comes_from_the_voice_key() {
        assert_eq!(language_of("ku_TR-hozan-medium"), "ku-TR");
        assert_eq!(language_of("en_GB-alba-low"), "en-GB");
    }

    #[test]
    fn a_named_voice_beats_the_language_which_beats_the_first() {
        let installed = keys(&["en_GB-alba-low", "ku_TR-hozan-medium"]);
        let settings = |language: Option<&str>, voice: Option<&str>| Settings {
            language: language.map(str::to_owned),
            synthesis_voice: voice.map(str::to_owned),
            ..Settings::default()
        };
        assert_eq!(
            pick_voice(&installed, &settings(None, Some("ku_TR-hozan-medium"))).as_deref(),
            Some("ku_TR-hozan-medium")
        );
        assert_eq!(
            pick_voice(&installed, &settings(Some("ku"), None)).as_deref(),
            Some("ku_TR-hozan-medium")
        );
        assert_eq!(
            pick_voice(&installed, &settings(Some("fr"), None)).as_deref(),
            Some("en_GB-alba-low")
        );
        assert_eq!(
            pick_voice(&installed, &settings(None, Some("missing"))).as_deref(),
            Some("en_GB-alba-low")
        );
        assert_eq!(pick_voice(&[], &Settings::default()), None);
    }
}
