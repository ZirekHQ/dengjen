use crate::digest::Digest;
use crate::error::{VoiceError, VoiceResult};
use crate::validate::{file_name, validate_key};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

pub const MAX_CATALOGUE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Language {
    pub code: String,
    #[serde(default)]
    pub family: String,
    #[serde(default)]
    pub name_english: String,
    #[serde(default)]
    pub name_native: String,
    #[serde(default)]
    pub country_english: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VoiceFile {
    pub path: String,
    pub size: u64,
    #[serde(skip_serializing)]
    pub digest: Digest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Voice {
    pub key: String,
    pub name: String,
    pub language: Language,
    pub quality: String,
    pub num_speakers: u32,
    pub files: Vec<VoiceFile>,
}

impl Voice {
    pub fn total_bytes(&self) -> u64 {
        self.files
            .iter()
            .fold(0, |sum, file| sum.saturating_add(file.size))
    }
}

#[derive(Deserialize)]
struct RawFile {
    size_bytes: u64,
    #[serde(default)]
    md5_digest: Option<String>,
    #[serde(default)]
    sha256_digest: Option<String>,
}

#[derive(Deserialize)]
struct RawVoice {
    key: String,
    name: String,
    language: Language,
    quality: String,
    #[serde(default)]
    num_speakers: u32,
    files: BTreeMap<String, RawFile>,
}

fn digest_of(path: &str, raw: &RawFile) -> VoiceResult<Digest> {
    let digest = raw
        .sha256_digest
        .clone()
        .map(Digest::Sha256)
        .or_else(|| raw.md5_digest.clone().map(Digest::Md5))
        .ok_or_else(|| VoiceError::CatalogueParse(format!("{path} has no digest")))?;
    digest
        .is_well_formed()
        .then_some(digest)
        .ok_or_else(|| VoiceError::CatalogueParse(format!("{path} has a malformed digest")))
}

fn convert_file((path, raw): (String, RawFile)) -> VoiceResult<VoiceFile> {
    file_name(&path)?;
    Ok(VoiceFile {
        digest: digest_of(&path, &raw)?,
        size: raw.size_bytes,
        path,
    })
}

const RESERVED_NAME: &str = "voice.json";

fn collision(path: &str) -> VoiceError {
    VoiceError::CatalogueParse(format!(
        "{path} collides with another file or a reserved name"
    ))
}

fn check_names(files: &[VoiceFile]) -> VoiceResult<()> {
    let mut seen = HashSet::new();
    files.iter().try_for_each(|file| {
        let name = file_name(&file.path)?.to_lowercase();
        (name != RESERVED_NAME && seen.insert(name))
            .then_some(())
            .ok_or_else(|| collision(&file.path))
    })
}

fn check_total(files: &[VoiceFile]) -> VoiceResult<()> {
    files
        .iter()
        .try_fold(0u64, |sum, file| sum.checked_add(file.size))
        .map(|_| ())
        .ok_or_else(|| VoiceError::CatalogueParse("voice size overflows".into()))
}

fn convert_files(raw: BTreeMap<String, RawFile>) -> VoiceResult<Vec<VoiceFile>> {
    let files: Vec<VoiceFile> = raw
        .into_iter()
        .map(convert_file)
        .collect::<VoiceResult<_>>()?;
    check_names(&files)?;
    check_total(&files)?;
    Ok(files)
}

fn convert(raw: RawVoice) -> VoiceResult<Voice> {
    validate_key(&raw.key)?;
    Ok(Voice {
        files: convert_files(raw.files)?,
        key: raw.key,
        name: raw.name,
        language: raw.language,
        quality: raw.quality,
        num_speakers: raw.num_speakers,
    })
}

fn convert_entry((key, raw): (String, RawVoice)) -> VoiceResult<(String, Voice)> {
    let voice = convert(raw)?;
    (voice.key == key).then_some(()).ok_or_else(|| {
        VoiceError::CatalogueParse(format!("{key} does not match its inner key {}", voice.key))
    })?;
    Ok((key, voice))
}

fn check_keys(voices: &BTreeMap<String, Voice>) -> VoiceResult<()> {
    let mut seen = HashSet::new();
    voices.keys().try_for_each(|key| {
        seen.insert(key.to_lowercase())
            .then_some(())
            .ok_or_else(|| VoiceError::CatalogueParse(format!("{key} collides with another voice")))
    })
}

fn same_language(voice: &Voice, query: &str) -> bool {
    let query = query.replace('-', "_");
    voice.language.code.eq_ignore_ascii_case(&query)
        || voice.language.family.eq_ignore_ascii_case(&query)
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Catalogue {
    voices: BTreeMap<String, Voice>,
}

impl Catalogue {
    pub fn from_slice(bytes: &[u8]) -> VoiceResult<Self> {
        if bytes.len() > MAX_CATALOGUE_BYTES {
            return Err(VoiceError::CatalogueParse(
                "catalogue exceeds the size limit".into(),
            ));
        }
        let raw: BTreeMap<String, RawVoice> = serde_json::from_slice(bytes)
            .map_err(|error| VoiceError::CatalogueParse(error.to_string()))?;
        let voices = raw
            .into_iter()
            .map(convert_entry)
            .collect::<VoiceResult<_>>()?;
        check_keys(&voices)?;
        Ok(Self { voices })
    }

    pub fn get(&self, key: &str) -> Option<&Voice> {
        self.voices.get(key)
    }

    pub fn by_language(&self, language: Option<&str>) -> Vec<&Voice> {
        self.voices
            .values()
            .filter(|voice| language.is_none_or(|query| same_language(voice, query)))
            .collect()
    }

    pub fn len(&self) -> usize {
        self.voices.len()
    }

    pub fn is_empty(&self) -> bool {
        self.voices.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::VoiceError;

    const SHA: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn sample() -> String {
        format!(
            r#"{{
  "ku_TR-berfin_renas-medium": {{
    "key": "ku_TR-berfin_renas-medium", "name": "berfin_renas",
    "language": {{"code": "ku_TR", "family": "ku", "region": "TR",
      "name_native": "Kurdî", "name_english": "Kurdish", "country_english": "Turkey"}},
    "quality": "medium", "num_speakers": 1, "speaker_id_map": {{}},
    "files": {{
      "ku/ku_TR/berfin_renas/medium/ku_TR-berfin_renas-medium.onnx":
        {{"size_bytes": 100, "md5_digest": "d335cd06fe4045a7ee9d8fb0712afaa9"}},
      "ku/ku_TR/berfin_renas/medium/ku_TR-berfin_renas-medium.onnx.json":
        {{"size_bytes": 20, "md5_digest": "465724f7d2d5f2ff061b53acb8e7f7cc", "sha256_digest": "{SHA}"}},
      "ku/ku_TR/berfin_renas/medium/MODEL_CARD":
        {{"size_bytes": 3, "md5_digest": "b6f0eaf5a7fd094be22a1bcb162173fb"}}
    }},
    "aliases": [], "future_field": true
  }},
  "en_US-lessac-medium": {{
    "key": "en_US-lessac-medium", "name": "lessac",
    "language": {{"code": "en_US", "family": "en"}},
    "quality": "medium", "num_speakers": 1,
    "files": {{"en/en_US/lessac/medium/en_US-lessac-medium.onnx":
      {{"size_bytes": 7, "md5_digest": "900150983cd24fb0d6963f7d28e17f72"}}}}
  }}
}}"#
        )
    }

    fn with_file(path: &str, entry: &str) -> String {
        format!(
            r#"{{"v": {{"key": "v", "name": "v", "language": {{"code": "xx"}}, "quality": "low",
              "files": {{"{path}": {entry}}}}}}}"#
        )
    }

    #[test]
    fn parses_voices_and_ignores_unknown_fields() {
        let catalogue = Catalogue::from_slice(sample().as_bytes()).unwrap();
        assert_eq!(catalogue.len(), 2);
        let voice = catalogue.get("ku_TR-berfin_renas-medium").unwrap();
        assert_eq!(voice.language.name_english, "Kurdish");
        assert_eq!(voice.files.len(), 3);
        assert_eq!(voice.total_bytes(), 123);
    }

    #[test]
    fn prefers_sha256_over_md5() {
        let catalogue = Catalogue::from_slice(sample().as_bytes()).unwrap();
        let voice = catalogue.get("ku_TR-berfin_renas-medium").unwrap();
        let digest_of = |suffix: &str| {
            voice
                .files
                .iter()
                .find(|f| f.path.ends_with(suffix))
                .unwrap()
                .digest
                .clone()
        };
        assert_eq!(digest_of(".onnx.json"), Digest::Sha256(SHA.into()));
        assert_eq!(
            digest_of("MODEL_CARD"),
            Digest::Md5("b6f0eaf5a7fd094be22a1bcb162173fb".into())
        );
    }

    #[test]
    fn rejects_an_entry_without_any_digest() {
        let json = with_file("a/b.onnx", r#"{"size_bytes": 1}"#);
        assert!(matches!(
            Catalogue::from_slice(json.as_bytes()),
            Err(VoiceError::CatalogueParse(reason)) if reason.contains("a/b.onnx")
        ));
    }

    fn with_files(files: &str) -> String {
        format!(
            r#"{{"v": {{"key": "v", "name": "v", "language": {{"code": "xx"}}, "quality": "low",
              "files": {{{files}}}}}}}"#
        )
    }

    #[test]
    fn rejects_duplicate_and_reserved_file_names() {
        let entry = r#"{"size_bytes": 1, "md5_digest": "d335cd06fe4045a7ee9d8fb0712afaa9"}"#;
        let duplicate = with_files(&format!(r#""a/m.onnx": {entry}, "b/m.onnx": {entry}"#));
        let reserved = with_files(&format!(r#""a/voice.json": {entry}"#));
        for json in [duplicate, reserved] {
            assert!(matches!(
                Catalogue::from_slice(json.as_bytes()),
                Err(VoiceError::CatalogueParse(reason)) if reason.contains("collides")
            ));
        }
    }

    #[test]
    fn rejects_file_names_that_collide_ignoring_case() {
        let entry = r#"{"size_bytes": 1, "md5_digest": "d335cd06fe4045a7ee9d8fb0712afaa9"}"#;
        let duplicate = with_files(&format!(r#""a/A.onnx": {entry}, "b/a.onnx": {entry}"#));
        let reserved = with_files(&format!(r#""a/Voice.JSON": {entry}"#));
        let accented = with_files(&format!(r#""a/Ã.onnx": {entry}, "b/ã.onnx": {entry}"#));
        for json in [duplicate, reserved, accented] {
            assert!(matches!(
                Catalogue::from_slice(json.as_bytes()),
                Err(VoiceError::CatalogueParse(reason)) if reason.contains("collides")
            ));
        }
    }

    fn voice_entry(outer: &str, inner: &str) -> String {
        format!(
            r#""{outer}": {{"key": "{inner}", "name": "v", "language": {{"code": "xx"}},
              "quality": "low", "files": {{}}}}"#
        )
    }

    #[test]
    fn rejects_an_entry_whose_map_key_differs_from_its_key() {
        let json = format!("{{{}}}", voice_entry("en_US-a-low", "en_US-b-low"));
        assert!(matches!(
            Catalogue::from_slice(json.as_bytes()),
            Err(VoiceError::CatalogueParse(reason)) if reason.contains("does not match")
        ));
    }

    #[test]
    fn rejects_voice_keys_that_collide_ignoring_case() {
        let (upper, lower) = (
            voice_entry("EN_us-a-low", "EN_us-a-low"),
            voice_entry("en_US-a-low", "en_US-a-low"),
        );
        let json = format!("{{{upper}, {lower}}}");
        assert!(matches!(
            Catalogue::from_slice(json.as_bytes()),
            Err(VoiceError::CatalogueParse(reason)) if reason.contains("collides")
        ));
    }

    #[test]
    fn rejects_sizes_that_overflow_when_summed() {
        let big = format!(
            r#"{{"size_bytes": {}, "md5_digest": "d335cd06fe4045a7ee9d8fb0712afaa9"}}"#,
            u64::MAX
        );
        let json = with_files(&format!(r#""a/x.onnx": {big}, "a/y.onnx": {big}"#));
        assert!(matches!(
            Catalogue::from_slice(json.as_bytes()),
            Err(VoiceError::CatalogueParse(reason)) if reason.contains("overflow")
        ));
    }

    #[test]
    fn rejects_malformed_digests() {
        for entry in [
            r#"{"size_bytes": 1, "md5_digest": "xyz"}"#,
            r#"{"size_bytes": 1, "md5_digest": "../../etc/passwd0000000000000000"}"#,
            r#"{"size_bytes": 1, "sha256_digest": "d335cd06fe4045a7ee9d8fb0712afaa9"}"#,
            r#"{"size_bytes": 1, "sha256_digest": ""}"#,
        ] {
            let json = with_file("a/b.onnx", entry);
            assert!(
                matches!(
                    Catalogue::from_slice(json.as_bytes()),
                    Err(VoiceError::CatalogueParse(reason)) if reason.contains("malformed")
                ),
                "{entry} must be rejected"
            );
        }
    }

    #[test]
    fn rejects_unsafe_file_paths() {
        let entry = r#"{"size_bytes": 1, "md5_digest": "d335cd06fe4045a7ee9d8fb0712afaa9"}"#;
        for path in ["../../x", "/etc/passwd", "a//b", "a\\\\b", "a/./b"] {
            let json = with_file(path, entry);
            assert!(
                matches!(
                    Catalogue::from_slice(json.as_bytes()),
                    Err(VoiceError::UnsafePath(_))
                ),
                "{path} must be rejected"
            );
        }
    }

    #[test]
    fn rejects_unsafe_keys() {
        let json = r#"{"v": {"key": "../v", "name": "v", "language": {"code": "xx"},
            "quality": "low", "files": {}}}"#;
        assert_eq!(
            Catalogue::from_slice(json.as_bytes()),
            Err(VoiceError::InvalidKey("../v".into()))
        );
    }

    #[test]
    fn rejects_malformed_json_and_oversize_input() {
        assert!(matches!(
            Catalogue::from_slice(b"{not json"),
            Err(VoiceError::CatalogueParse(_))
        ));
        let huge = vec![b' '; MAX_CATALOGUE_BYTES + 1];
        assert!(matches!(
            Catalogue::from_slice(&huge),
            Err(VoiceError::CatalogueParse(reason)) if reason.contains("limit")
        ));
    }

    #[test]
    fn filters_by_language_code_or_family() {
        let catalogue = Catalogue::from_slice(sample().as_bytes()).unwrap();
        let keys = |q| {
            catalogue
                .by_language(q)
                .iter()
                .map(|v| v.key.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(keys(Some("ku")), ["ku_TR-berfin_renas-medium"]);
        assert_eq!(keys(Some("ku-TR")), ["ku_TR-berfin_renas-medium"]);
        assert_eq!(keys(Some("EN_us")), ["en_US-lessac-medium"]);
        assert!(keys(Some("de")).is_empty());
        assert_eq!(keys(None).len(), 2);
    }
}
