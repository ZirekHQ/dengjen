#![forbid(unsafe_code)]

#[cfg(feature = "espeak")]
use dengjen_espeak_phonemizer::text_to_phonemes;
#[cfg(feature = "tashkeel")]
use dengjen_tashkeel::do_tashkeel;
use dengjen_tts_core::{
    write_ignoring_poison, AudioInfo, DengjenError, DengjenModel, DengjenResult, Phonemes,
};
#[cfg(feature = "espeak")]
use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};

mod config;
mod inference;
mod phonemize;
mod streaming;
pub mod synth_config;

pub use config::*;
use config::{load_model_config, resolve_default_speaker_id};
pub use inference::VitsModel;
#[cfg(feature = "espeak")]
use phonemize::should_diacritize;
use phonemize::{phonemize_dispatch, HebrewEngine, PinyinEngine, TashkeelEngine};
pub use streaming::VitsStreamingModel;
pub use synth_config::PiperSynthesisConfig;

const PAD: &str = "_";
const BOS: &str = "^";
const EOS: &str = "$";

#[tracing::instrument(skip_all, fields(config_path = %config_path.display()), err)]
pub fn from_config_path(config_path: &Path) -> DengjenResult<Arc<dyn DengjenModel + Send + Sync>> {
    let (config, synth_config) = load_model_config(config_path)?;

    if config.streaming.unwrap_or_default() {
        let encoder_path = config_path.with_file_name("encoder.onnx");
        let decoder_path = config_path.with_file_name("decoder.onnx");
        let model = VitsStreamingModel::from_config(
            config,
            synth_config,
            config_path,
            &encoder_path,
            &decoder_path,
        )?;
        return Ok(Arc::new(model));
    }

    let Some(stem) = config_path.file_stem() else {
        return Err(DengjenError::InvalidConfiguration(format!(
            "Invalid config filename format `{}`",
            config_path.display()
        )));
    };
    let onnx_path = config_path.with_file_name(stem);
    let model = VitsModel::from_config(config, synth_config, config_path, &onnx_path)?;
    Ok(Arc::new(model))
}

trait VitsModelCommons {
    fn get_synth_config(&self) -> &RwLock<PiperSynthesisConfig>;
    fn get_config(&self) -> &ModelConfig;
    fn get_speaker_map(&self) -> &HashMap<i64, String>;
    #[cfg_attr(not(all(feature = "tashkeel", feature = "espeak")), allow(dead_code))]
    fn get_tashkeel_engine(&self) -> Option<&TashkeelEngine>;
    fn get_hebrew_engine(&self) -> Option<&HebrewEngine>;
    fn get_pinyin_engine(&self) -> Option<&PinyinEngine>;

    fn get_meta_ids(&self) -> DengjenResult<(i64, i64, i64)> {
        let phoneme_id_map = &self.get_config().phoneme_id_map;
        let first_id = |token: &str| {
            phoneme_id_map
                .get(token)
                .and_then(|ids| ids.first().copied())
                .ok_or_else(|| {
                    DengjenError::InvalidConfiguration(format!(
                        "Voice config's `phoneme_id_map` has no id for the required token `{token}`"
                    ))
                })
        };
        Ok((first_id(PAD)?, first_id(BOS)?, first_id(EOS)?))
    }

    fn language(&self) -> Option<String> {
        let config = self.get_config();
        match &config.language {
            Some(lang) => Some(lang.code.clone()),
            None => Some(config.espeak.voice.clone()),
        }
    }

    fn get_properties(&self) -> HashMap<String, String> {
        let quality = self
            .get_config()
            .audio
            .quality
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        HashMap::from([("quality".to_string(), quality)])
    }

    fn factory_synthesis_config(&self) -> PiperSynthesisConfig {
        let config = self.get_config();
        let speaker = (config.num_speakers > 1).then(|| resolve_default_speaker_id(config));
        PiperSynthesisConfig {
            speaker,
            length_scale: config.inference.length_scale,
            noise_scale: config.inference.noise_scale,
            noise_w: config.inference.noise_w,
        }
    }

    fn _do_set_default_synth_config(&self, new_config: &PiperSynthesisConfig) -> DengjenResult<()> {
        let mut synth_config = write_ignoring_poison(self.get_synth_config());
        synth_config.length_scale = new_config.length_scale;
        synth_config.noise_scale = new_config.noise_scale;
        synth_config.noise_w = new_config.noise_w;
        if let Some(sid) = new_config.speaker {
            if !self.get_speaker_map().contains_key(&sid) {
                return Err(DengjenError::InvalidConfiguration(format!(
                    "No speaker was found with the given id `{}`",
                    sid
                )));
            }
            synth_config.speaker = Some(sid);
        }
        Ok(())
    }

    fn phonemes_to_input_ids(
        &self,
        phonemes: &str,
        pad_id: i64,
        bos_id: i64,
        eos_id: i64,
    ) -> Vec<i64> {
        map_phonemes_to_ids(
            &self.get_config().phoneme_id_map,
            phonemes,
            pad_id,
            bos_id,
            eos_id,
        )
    }
    #[cfg(feature = "espeak")]
    fn do_phonemize_text(&self, text: &str) -> DengjenResult<Phonemes> {
        let config = self.get_config();
        if let Some(handled) = phonemize_dispatch(
            config.phoneme_type.unwrap_or_default(),
            text,
            self.get_hebrew_engine(),
            self.get_pinyin_engine(),
        ) {
            return handled;
        }
        let voice = &config.espeak.voice;
        let source: Cow<'_, str> = if should_diacritize(voice) {
            Cow::Owned(self.diacritize_text(text)?)
        } else {
            Cow::Borrowed(text)
        };

        text_to_phonemes(&source, voice, None, true, false)
            .map(Phonemes::from)
            .map_err(|e| {
                DengjenError::PhonemizationError(format!(
                    "Failed to phonemize given text using espeak-ng. Error: {e}"
                ))
            })
    }
    #[cfg(not(feature = "espeak"))]
    fn do_phonemize_text(&self, text: &str) -> DengjenResult<Phonemes> {
        let config = self.get_config();
        phonemize_dispatch(
            config.phoneme_type.unwrap_or_default(),
            text,
            self.get_hebrew_engine(),
            self.get_pinyin_engine(),
        )
        .unwrap_or_else(|| {
            Err(DengjenError::PhonemizationError(
                "This voice requires espeak-based phonemization, but the `espeak` feature (GPL-3.0-or-later, via espeak-ng) is disabled".to_string(),
            ))
        })
    }
    #[cfg(feature = "tashkeel")]
    #[cfg_attr(not(feature = "espeak"), allow(dead_code))]
    fn diacritize_text(&self, text: &str) -> DengjenResult<String> {
        let engine = self
            .get_tashkeel_engine()
            .expect("tashkeel engine missing for a voice that needs diacritization");
        do_tashkeel(engine, text, None, false).map_err(|msg| {
            DengjenError::InferenceError(format!(
                "Failed to diacritize text using libtashkeel. {msg}"
            ))
        })
    }

    #[cfg(not(feature = "tashkeel"))]
    #[cfg_attr(not(feature = "espeak"), allow(dead_code))]
    fn diacritize_text(&self, _text: &str) -> DengjenResult<String> {
        unreachable!("diacritize_text called with the `tashkeel` feature disabled")
    }
    fn get_audio_output_info(&self) -> DengjenResult<AudioInfo> {
        Ok(AudioInfo {
            sample_rate: self.get_config().audio.sample_rate as usize,
            num_channels: 1,
            sample_width: 2,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestVitsCommons {
        synth_config: RwLock<PiperSynthesisConfig>,
        config: ModelConfig,
        speaker_map: HashMap<i64, String>,
    }

    impl VitsModelCommons for TestVitsCommons {
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
            None
        }
        fn get_hebrew_engine(&self) -> Option<&HebrewEngine> {
            None
        }
        fn get_pinyin_engine(&self) -> Option<&PinyinEngine> {
            None
        }
    }

    #[test]
    fn get_meta_ids_reads_bos_pad_eos_from_phoneme_id_map() {
        let commons = TestVitsCommons {
            synth_config: RwLock::new(PiperSynthesisConfig::default()),
            config: ModelConfig {
                phoneme_id_map: HashMap::from([
                    (PAD.to_string(), vec![3]),
                    (BOS.to_string(), vec![1]),
                    (EOS.to_string(), vec![2]),
                ]),
                ..Default::default()
            },
            speaker_map: HashMap::new(),
        };
        assert_eq!(commons.get_meta_ids().unwrap(), (3, 1, 2));
    }

    #[test]
    fn get_meta_ids_errors_when_a_required_token_is_missing() {
        let commons = TestVitsCommons {
            synth_config: RwLock::new(PiperSynthesisConfig::default()),
            config: ModelConfig {
                phoneme_id_map: HashMap::from([
                    (BOS.to_string(), vec![1]),
                    (EOS.to_string(), vec![2]),
                ]),
                ..Default::default()
            },
            speaker_map: HashMap::new(),
        };
        assert!(matches!(
            commons.get_meta_ids(),
            Err(DengjenError::InvalidConfiguration(_))
        ));
    }

    #[test]
    fn get_meta_ids_errors_when_a_required_token_maps_to_an_empty_id_list() {
        let commons = TestVitsCommons {
            synth_config: RwLock::new(PiperSynthesisConfig::default()),
            config: ModelConfig {
                phoneme_id_map: HashMap::from([
                    (PAD.to_string(), vec![]),
                    (BOS.to_string(), vec![1]),
                    (EOS.to_string(), vec![2]),
                ]),
                ..Default::default()
            },
            speaker_map: HashMap::new(),
        };
        assert!(matches!(
            commons.get_meta_ids(),
            Err(DengjenError::InvalidConfiguration(_))
        ));
    }

    #[test]
    fn do_set_default_synth_config_updates_scales_and_accepts_a_known_speaker() {
        let commons = TestVitsCommons {
            synth_config: RwLock::new(PiperSynthesisConfig::default()),
            config: ModelConfig::default(),
            speaker_map: HashMap::from([(5, "narrator".to_string())]),
        };
        let new_config = PiperSynthesisConfig {
            speaker: Some(5),
            noise_scale: 0.5,
            length_scale: 1.2,
            noise_w: 0.9,
        };
        commons._do_set_default_synth_config(&new_config).unwrap();
        let synth_config = commons.synth_config.read().unwrap();
        assert_eq!(synth_config.speaker, Some(5));
        assert_eq!(synth_config.length_scale, 1.2);
    }

    #[test]
    fn do_set_default_synth_config_errors_for_an_unknown_speaker_id() {
        let commons = TestVitsCommons {
            synth_config: RwLock::new(PiperSynthesisConfig::default()),
            config: ModelConfig::default(),
            speaker_map: HashMap::new(),
        };
        let new_config = PiperSynthesisConfig {
            speaker: Some(99),
            ..Default::default()
        };
        let result = commons._do_set_default_synth_config(&new_config);
        assert!(matches!(result, Err(DengjenError::InvalidConfiguration(_))));
    }

    fn commons_with(num_speakers: u32, speaker_map: HashMap<i64, String>) -> TestVitsCommons {
        TestVitsCommons {
            synth_config: RwLock::new(PiperSynthesisConfig::default()),
            config: ModelConfig {
                num_speakers,
                ..Default::default()
            },
            speaker_map,
        }
    }

    #[test]
    fn a_single_speaker_voice_with_an_empty_speaker_map_accepts_its_factory_config() {
        let commons = commons_with(1, HashMap::new());
        let factory = commons.factory_synthesis_config();
        assert_eq!(factory.speaker, None);
        assert!(commons._do_set_default_synth_config(&factory).is_ok());
    }

    #[test]
    fn a_multi_speaker_voice_keeps_a_default_speaker_in_its_factory_config() {
        let commons = commons_with(2, HashMap::from([(0, "a".into()), (1, "b".into())]));
        assert_eq!(commons.factory_synthesis_config().speaker, Some(0));
    }

    #[test]
    fn do_set_default_synth_config_applies_scales_even_when_the_speaker_id_is_unknown() {
        let commons = TestVitsCommons {
            synth_config: RwLock::new(PiperSynthesisConfig::default()),
            config: ModelConfig::default(),
            speaker_map: HashMap::new(),
        };
        let new_config = PiperSynthesisConfig {
            speaker: Some(99),
            noise_scale: 0.5,
            length_scale: 1.2,
            noise_w: 0.9,
        };
        let result = commons._do_set_default_synth_config(&new_config);
        assert!(matches!(result, Err(DengjenError::InvalidConfiguration(_))));
        let synth_config = commons.synth_config.read().unwrap();
        assert_eq!(synth_config.length_scale, 1.2);
        assert_eq!(synth_config.noise_scale, 0.5);
        assert_eq!(synth_config.noise_w, 0.9);
        assert_eq!(synth_config.speaker, None);
    }

    #[test]
    fn do_phonemize_text_passes_through_unchanged_for_text_phoneme_type() {
        let commons = TestVitsCommons {
            synth_config: RwLock::new(PiperSynthesisConfig::default()),
            config: ModelConfig {
                phoneme_type: Some(PhonemeType::Text),
                ..Default::default()
            },
            speaker_map: HashMap::new(),
        };
        let result = commons.do_phonemize_text("hello").unwrap();
        assert_eq!(result.to_vec(), vec!["hello".to_string()]);
    }

    #[cfg(not(feature = "espeak"))]
    #[test]
    fn do_phonemize_text_errors_when_espeak_feature_is_disabled() {
        let commons = TestVitsCommons {
            synth_config: RwLock::new(PiperSynthesisConfig::default()),
            config: ModelConfig {
                phoneme_type: Some(PhonemeType::Espeak),
                ..Default::default()
            },
            speaker_map: HashMap::new(),
        };
        let result = commons.do_phonemize_text("hello");
        assert!(matches!(result, Err(DengjenError::PhonemizationError(_))));
    }

    #[cfg(feature = "espeak")]
    #[test]
    fn do_phonemize_text_wraps_the_full_espeak_init_error_message() {
        std::env::set_var(
            "DENGJEN_ESPEAKNG_DATA_DIRECTORY",
            "/nonexistent/dengjen-test-guaranteed-missing-espeak-ng-data",
        );

        let commons = TestVitsCommons {
            synth_config: RwLock::new(PiperSynthesisConfig::default()),
            config: ModelConfig::default(),
            speaker_map: HashMap::new(),
        };
        let result = commons.do_phonemize_text("hello");
        let msg = match result {
            Err(DengjenError::PhonemizationError(msg)) => msg,
            Err(other) => panic!("expected a PhonemizationError, got {other:?}"),
            Ok(_) => panic!("expected a PhonemizationError, got Ok"),
        };
        assert!(
            msg.contains(dengjen_espeak_phonemizer::ESPEAKNG_INIT_FAILURE_MARKER),
            "wrapped message lost the inner espeak-ng error text: {msg:?}"
        );
    }
}
