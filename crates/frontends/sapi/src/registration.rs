//! What registering the voice writes to the registry, worked out without touching it.
//!
//! SAPI lists voices from tokens under `SOFTWARE\Microsoft\Speech\Voices\Tokens`, in HKLM for
//! every account (including the secure desktop's) or in HKCU for one user without admin rights.
//! Each token names this DLL's COM class and the voice's config file. The Windows layer writes
//! exactly the values returned here, so the layout is tested on every OS.
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use crate::language::{self, KurdishPolicy};

/// The COM class SAPI instantiates for every dengjen voice token.
pub const CLSID: &str = "{EE94E0DE-50A8-4645-B76B-38F65AE052D4}";
pub const CLASS_KEY: &str = r"SOFTWARE\Classes\CLSID\{EE94E0DE-50A8-4645-B76B-38F65AE052D4}";
pub const TOKENS_KEY: &str = r"SOFTWARE\Microsoft\Speech\Voices\Tokens";
/// Every token this DLL registers starts with this, so unregistering removes only ours.
pub const TOKEN_PREFIX: &str = "Dengjen-";
/// The token value holding the voice's `.onnx.json` path, read back in `SetObjectToken`.
pub const VOICE_PATH_VALUE: &str = "VoicePath";
const MODEL_TYPE: &str = "piper";
const VENDOR: &str = "Zirek";

/// Where a registration goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scope {
    /// HKLM: every account, the logon screen and UAC prompts. Needs an elevated `regsvr32`.
    #[default]
    Machine,
    /// HKCU: the current user only, no admin rights needed.
    User,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InstallOptions {
    pub scope: Scope,
    pub kurdish: KurdishPolicy,
    /// Defaults to a `voices` directory beside the DLL.
    pub voices_dir: Option<PathBuf>,
}

/// Parses `DllInstall`'s command line, as passed by `regsvr32 /i:"user;kurdish=both;voices=D:\v"`.
pub fn parse_install_args(command_line: &str) -> Result<InstallOptions, String> {
    command_line
        .split(';')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .try_fold(InstallOptions::default(), |mut options, part| {
            match part.split_once('=') {
                None if part.eq_ignore_ascii_case("user") => options.scope = Scope::User,
                None if part.eq_ignore_ascii_case("machine") => options.scope = Scope::Machine,
                Some((key, value)) if key.trim().eq_ignore_ascii_case("kurdish") => {
                    options.kurdish = value.parse()?;
                }
                Some((key, value)) if key.trim().eq_ignore_ascii_case("voices") => {
                    options.voices_dir = Some(PathBuf::from(value.trim()));
                }
                _ => {
                    return Err(format!(
                    "unknown option `{part}`; use user, machine, kurdish=<policy> or voices=<dir>"
                ))
                }
            }
            Ok(options)
        })
}

/// An installed voice, as found on disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Voice {
    /// The voice's directory name, e.g. `ku_TR-berfin_renas-medium`.
    pub key: String,
    pub config_path: PathBuf,
}

impl Voice {
    /// `ku_TR` from `ku_TR-berfin_renas-medium`.
    pub fn language(&self) -> &str {
        self.key.split('-').next().unwrap_or(&self.key)
    }

    /// `Dengjen Berfin Renas (ku_TR, medium)`.
    pub fn display_name(&self) -> String {
        let mut parts = self.key.split('-');
        let language = parts.next().unwrap_or_default();
        let name = parts
            .next()
            .map(|name| {
                name.split('_')
                    .map(capitalized)
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_else(|| self.key.clone());
        let details: Vec<&str> = std::iter::once(language).chain(parts).collect();
        format!("Dengjen {name} ({})", details.join(", "))
    }

    pub fn token_name(&self) -> String {
        format!("{TOKEN_PREFIX}{}", self.key)
    }
}

fn capitalized(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// The voices under `voices_dir`, laid out as `piper/<key>/<key>.onnx.json` like every other
/// dengjen frontend expects, sorted by key.
pub fn discover(voices_dir: &Path) -> Vec<Voice> {
    let mut voices: Vec<Voice> = std::fs::read_dir(voices_dir.join(MODEL_TYPE))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let key = entry.file_name().to_string_lossy().into_owned();
            let config_path = entry.path().join(format!("{key}.onnx.json"));
            config_path.is_file().then_some(Voice { key, config_path })
        })
        .collect();
    voices.sort_by(|a, b| a.key.cmp(&b.key));
    voices
}

/// The sample rate a Piper voice's config declares, read without loading the model, so SAPI can
/// learn the output format as soon as a voice is selected.
pub fn sample_rate(config_path: &Path) -> Result<u32, String> {
    let text = std::fs::read_to_string(config_path)
        .map_err(|error| format!("cannot read `{}`: {error}", config_path.display()))?;
    let config: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| format!("`{}` is not JSON: {error}", config_path.display()))?;
    config
        .pointer("/audio/sample_rate")
        .and_then(serde_json::Value::as_u64)
        .and_then(|rate| u32::try_from(rate).ok())
        .filter(|rate| *rate > 0)
        .ok_or_else(|| format!("`{}` has no audio.sample_rate", config_path.display()))
}

/// One registry value: `key` is relative to the hive's root, `name` `None` is the default value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Value {
    pub key: String,
    pub name: Option<String>,
    pub data: String,
}

fn value(key: &str, name: Option<&str>, data: impl Into<String>) -> Value {
    Value {
        key: key.to_owned(),
        name: name.map(str::to_owned),
        data: data.into(),
    }
}

/// The COM class registration pointing at the DLL.
pub fn class_values(dll_path: &Path) -> Vec<Value> {
    let server = format!(r"{CLASS_KEY}\InprocServer32");
    vec![
        value(CLASS_KEY, None, "Dengjen SAPI5 voice"),
        value(&server, None, dll_path.display().to_string()),
        value(&server, Some("ThreadingModel"), "Both"),
    ]
}

/// The voice token for one voice.
pub fn token_values(voice: &Voice, kurdish: KurdishPolicy) -> Vec<Value> {
    let token = format!(r"{TOKENS_KEY}\{}", voice.token_name());
    let attributes = format!(r"{token}\Attributes");
    let langids = language::langids(voice.language(), kurdish);
    let display = voice.display_name();
    let mut values = vec![
        value(&token, None, display.clone()),
        value(&token, Some("CLSID"), CLSID),
        value(
            &token,
            Some(VOICE_PATH_VALUE),
            voice.config_path.display().to_string(),
        ),
        value(&attributes, Some("Name"), display.clone()),
        value(&attributes, Some("Language"), language::attribute(&langids)),
        value(&attributes, Some("Age"), "Adult"),
        value(&attributes, Some("Vendor"), VENDOR),
    ];
    // SAPI shows a token's name from the value named after the user's LANGID, falling back to
    // the default value; naming it for each of the voice's LANGIDs covers its own speakers.
    values.extend(
        langids
            .iter()
            .map(|langid| value(&token, Some(&format!("{langid:x}")), display.clone())),
    );
    values
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voice(key: &str) -> Voice {
        Voice {
            key: key.to_owned(),
            config_path: PathBuf::from(format!(r"C:\voices\piper\{key}\{key}.onnx.json")),
        }
    }

    #[test]
    fn install_args_default_to_machine_scope_and_central_kurdish_first() {
        assert_eq!(parse_install_args(""), Ok(InstallOptions::default()));
        assert_eq!(
            parse_install_args(r" user ; kurdish=both; voices=D:\My Voices "),
            Ok(InstallOptions {
                scope: Scope::User,
                kurdish: KurdishPolicy::CentralKurdishThenUnspecified,
                voices_dir: Some(PathBuf::from(r"D:\My Voices")),
            })
        );
        assert!(parse_install_args("kurdish=sorani").is_err());
        assert!(parse_install_args("global").is_err());
    }

    #[test]
    fn display_names_read_well_in_a_voice_list() {
        assert_eq!(
            voice("ku_TR-berfin_renas-medium").display_name(),
            "Dengjen Berfin Renas (ku_TR, medium)"
        );
        assert_eq!(voice("odd").display_name(), "Dengjen odd (odd)");
        assert_eq!(voice("ku_TR-berfin_renas-medium").language(), "ku_TR");
    }

    #[test]
    fn the_token_names_the_class_the_voice_file_and_the_language() {
        let values = token_values(&voice("ku_TR-hozan-medium"), KurdishPolicy::default());
        let token = r"SOFTWARE\Microsoft\Speech\Voices\Tokens\Dengjen-ku_TR-hozan-medium";
        let find = |key: &str, name: &str| {
            values
                .iter()
                .find(|value| value.key == key && value.name.as_deref() == Some(name))
                .map(|value| value.data.as_str())
        };
        assert_eq!(find(token, "CLSID"), Some(CLSID));
        assert_eq!(
            find(token, VOICE_PATH_VALUE),
            Some(r"C:\voices\piper\ku_TR-hozan-medium\ku_TR-hozan-medium.onnx.json")
        );
        assert_eq!(
            find(&format!(r"{token}\Attributes"), "Language"),
            Some("492;1000")
        );
        assert_eq!(find(token, "492"), Some("Dengjen Hozan (ku_TR, medium)"));
        assert_eq!(find(token, "1000"), Some("Dengjen Hozan (ku_TR, medium)"));
        assert_eq!(
            find(&format!(r"{token}\Attributes"), "Name"),
            Some("Dengjen Hozan (ku_TR, medium)")
        );
    }

    #[test]
    fn the_class_is_registered_both_threaded_at_the_dll() {
        let values = class_values(Path::new(r"C:\Dengjen\dengjen_sapi.dll"));
        assert!(values.contains(&Value {
            key: format!(r"{CLASS_KEY}\InprocServer32"),
            name: None,
            data: r"C:\Dengjen\dengjen_sapi.dll".to_owned(),
        }));
        assert!(values
            .iter()
            .any(|value| value.name.as_deref() == Some("ThreadingModel") && value.data == "Both"));
        assert!(CLASS_KEY.ends_with(CLSID));
    }

    #[test]
    fn discovery_finds_voices_with_a_config_and_skips_the_rest() {
        let root = std::env::temp_dir().join(format!(
            "dengjen-sapi-discover-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
        ));
        for (key, with_config) in [("b_voice", true), ("a_voice", true), ("broken", false)] {
            let dir = root.join("piper").join(key);
            std::fs::create_dir_all(&dir).expect("create voice dir");
            if with_config {
                std::fs::write(dir.join(format!("{key}.onnx.json")), "{}").expect("write config");
            }
        }
        let keys: Vec<String> = discover(&root).into_iter().map(|voice| voice.key).collect();
        assert_eq!(keys, ["a_voice", "b_voice"]);
        assert!(discover(&root.join("missing")).is_empty());
        let config = root.join("piper/a_voice/a_voice.onnx.json");
        assert!(sample_rate(&config).is_err());
        std::fs::write(&config, r#"{"audio": {"sample_rate": 22050}}"#).expect("write config");
        assert_eq!(sample_rate(&config), Ok(22050));
        std::fs::remove_dir_all(root).expect("clean up");
    }
}
