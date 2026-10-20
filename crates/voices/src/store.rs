use crate::catalogue::Voice;
use crate::digest::sha256_hex;
use crate::error::{io_at, VoiceError, VoiceResult};
use crate::validate::validate_key;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

const STAGING: &str = ".staging";
const LOCKS: &str = ".locks";
const OLD: &str = ".old";
const TRASH: &str = ".trash";
const CACHE: &str = ".catalogue";
const SIDECAR: &str = "voice.json";

#[derive(Serialize, Deserialize)]
struct Sidecar {
    model_type: String,
    name: String,
    language: String,
    #[serde(default)]
    description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstalledVoice {
    pub key: String,
    pub model_type: String,
    pub name: String,
    pub language: String,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
    model_type: String,
}

fn not_found(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::NotFound
}

fn remove_dir_if_exists(path: &Path) -> VoiceResult<()> {
    match fs::remove_dir_all(path) {
        Err(error) if !not_found(&error) => Err(VoiceError::io(path, &error)),
        _ => Ok(()),
    }
}

fn rename_if_exists(from: &Path, to: &Path) -> VoiceResult<bool> {
    match fs::rename(from, to) {
        Ok(()) => Ok(true),
        Err(error) if not_found(&error) => Ok(false),
        Err(error) => Err(VoiceError::io(from, &error)),
    }
}

fn read_dir_if_exists(dir: &Path) -> VoiceResult<Option<fs::ReadDir>> {
    match fs::read_dir(dir) {
        Ok(entries) => Ok(Some(entries)),
        Err(error) if not_found(&error) => Ok(None),
        Err(error) => Err(VoiceError::io(dir, &error)),
    }
}

fn ensure_parents(paths: &[&Path]) -> VoiceResult<()> {
    paths
        .iter()
        .filter_map(|path| path.parent())
        .try_for_each(|parent| fs::create_dir_all(parent).map_err(io_at(parent)))
}

fn swap_in(staging: &Path, target: &Path, old: &Path) -> VoiceResult<()> {
    remove_dir_if_exists(old)?;
    let had_previous = rename_if_exists(target, old)?;
    if let Err(error) = fs::rename(staging, target) {
        if had_previous {
            fs::rename(old, target).map_err(io_at(old))?;
        }
        return Err(VoiceError::io(staging, &error));
    }
    // A leftover backup is removed by the next commit or removal of that key; the voice is already in place.
    let _ = remove_dir_if_exists(old);
    Ok(())
}

fn lock_error(key: &str, path: &Path, error: &io::Error) -> VoiceError {
    match error.kind() {
        io::ErrorKind::WouldBlock => VoiceError::Busy(key.to_owned()),
        _ => VoiceError::io(path, error),
    }
}

impl Store {
    pub fn new(root: PathBuf, model_type: &str) -> VoiceResult<Self> {
        let model_type = validate_key(model_type)?.to_owned();
        Ok(Self { root, model_type })
    }

    pub fn install_dir(&self, key: &str) -> PathBuf {
        self.root.join(&self.model_type).join(key)
    }

    fn work_dir(&self, area: &str) -> PathBuf {
        self.root.join(area).join(&self.model_type)
    }

    pub fn staging_dir(&self, key: &str) -> PathBuf {
        self.work_dir(STAGING).join(key)
    }

    fn open_lock(&self, key: &str) -> VoiceResult<(PathBuf, fs::File)> {
        let dir = self.work_dir(LOCKS);
        fs::create_dir_all(&dir).map_err(io_at(&dir))?;
        let path = dir.join(format!("{key}.lock"));
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .map_err(io_at(&path))?;
        Ok((path, file))
    }

    pub fn with_lock<T>(
        &self,
        key: &str,
        action: impl FnOnce() -> VoiceResult<T>,
    ) -> VoiceResult<T> {
        validate_key(key)?;
        let (path, file) = self.open_lock(key)?;
        let mut lock = fd_lock::RwLock::new(file);
        let _held = lock
            .try_write()
            .map_err(|error| lock_error(key, &path, &error))?;
        self.recover(key)?;
        action()
    }

    fn old_dir(&self, key: &str) -> PathBuf {
        self.work_dir(OLD).join(key)
    }

    fn trash_dir(&self, key: &str) -> PathBuf {
        self.work_dir(TRASH).join(key)
    }

    fn recover(&self, key: &str) -> VoiceResult<()> {
        let (target, old) = (self.install_dir(key), self.old_dir(key));
        if target.exists() || !old.exists() {
            return Ok(());
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(io_at(parent))?;
        }
        fs::rename(&old, &target).map_err(io_at(&old))
    }

    pub fn write_sidecar(&self, dir: &Path, voice: &Voice) -> VoiceResult<()> {
        let path = dir.join(SIDECAR);
        let sidecar = Sidecar {
            model_type: self.model_type.clone(),
            name: voice.name.clone(),
            language: voice.language.code.clone(),
            description: String::new(),
        };
        let bytes = serde_json::to_vec_pretty(&sidecar)
            .map_err(|_| VoiceError::Io(path.clone(), io::ErrorKind::InvalidData))?;
        fs::write(&path, bytes).map_err(io_at(&path))
    }

    pub fn commit(&self, key: &str) -> VoiceResult<()> {
        validate_key(key)?;
        let (staging, target) = (self.staging_dir(key), self.install_dir(key));
        let old = self.old_dir(key);
        self.recover(key)?;
        if !staging.exists() {
            return Err(VoiceError::Io(staging, io::ErrorKind::NotFound));
        }
        ensure_parents(&[&target, &old])?;
        swap_in(&staging, &target, &old)
    }

    fn read_installed(&self, key: &str) -> Option<InstalledVoice> {
        validate_key(key).ok()?;
        let bytes = fs::read(self.install_dir(key).join(SIDECAR)).ok()?;
        let sidecar: Sidecar = serde_json::from_slice(&bytes).ok()?;
        Some(InstalledVoice {
            key: key.to_owned(),
            model_type: sidecar.model_type,
            name: sidecar.name,
            language: sidecar.language,
            description: sidecar.description,
        })
    }

    fn installed_keys(&self) -> VoiceResult<Vec<String>> {
        let dir = self.root.join(&self.model_type);
        read_dir_if_exists(&dir)?
            .into_iter()
            .flatten()
            .map(|entry| {
                entry
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .map_err(io_at(&dir))
            })
            .collect()
    }

    pub fn installed(&self) -> VoiceResult<Vec<InstalledVoice>> {
        let mut voices: Vec<InstalledVoice> = self
            .installed_keys()?
            .iter()
            .filter_map(|key| self.read_installed(key))
            .collect();
        voices.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(voices)
    }

    pub fn remove(&self, key: &str) -> VoiceResult<()> {
        validate_key(key)?;
        self.with_lock(key, || {
            let (dir, trash) = (self.install_dir(key), self.trash_dir(key));
            ensure_parents(&[&trash])?;
            remove_dir_if_exists(&trash)?;
            remove_dir_if_exists(&self.old_dir(key))?;
            rename_if_exists(&dir, &trash)?
                .then_some(())
                .ok_or_else(|| VoiceError::NotInstalled(key.to_owned()))?;
            // A leftover in .trash is deleted by the next removal of that key.
            let _ = remove_dir_if_exists(&trash);
            Ok(())
        })
    }

    fn cache_path(&self, url: &str) -> PathBuf {
        let name = format!("{}.json", sha256_hex(url.as_bytes()));
        self.root.join(CACHE).join(name)
    }

    pub fn write_cache(&self, url: &str, bytes: &[u8]) -> VoiceResult<()> {
        let path = self.cache_path(url);
        let dir = self.root.join(CACHE);
        fs::create_dir_all(&dir).map_err(io_at(&dir))?;
        let temporary = path.with_extension("json.tmp");
        fs::write(&temporary, bytes).map_err(io_at(&temporary))?;
        fs::rename(&temporary, &path).map_err(io_at(&path))
    }

    pub fn read_cache(&self, url: &str) -> VoiceResult<Option<Vec<u8>>> {
        let path = self.cache_path(url);
        match fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if not_found(&error) => Ok(None),
            Err(error) => Err(VoiceError::io(&path, &error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalogue::Language;
    use std::fs;

    fn voice(key: &str) -> Voice {
        Voice {
            key: key.into(),
            name: "berfin".into(),
            language: Language {
                code: "ku_TR".into(),
                family: "ku".into(),
                name_english: String::new(),
                name_native: String::new(),
                country_english: String::new(),
            },
            quality: "medium".into(),
            num_speakers: 1,
            files: Vec::new(),
        }
    }

    fn store(dir: &tempfile::TempDir) -> Store {
        Store::new(dir.path().to_path_buf(), "piper").unwrap()
    }

    fn stage(store: &Store, key: &str, content: &str) {
        let staging = store.staging_dir(key);
        fs::create_dir_all(&staging).unwrap();
        fs::write(staging.join("m.onnx"), content).unwrap();
        store.write_sidecar(&staging, &voice(key)).unwrap();
    }

    #[test]
    fn new_rejects_an_unsafe_model_type() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Store::new(dir.path().into(), "../piper").is_err());
    }

    #[test]
    fn commit_moves_the_staging_directory_into_place() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        stage(&store, "ku", "v1");
        store.commit("ku").unwrap();
        assert_eq!(
            fs::read_to_string(store.install_dir("ku").join("m.onnx")).unwrap(),
            "v1"
        );
        assert!(!store.staging_dir("ku").exists());
    }

    #[test]
    fn commit_replaces_an_existing_install() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        stage(&store, "ku", "v1");
        store.commit("ku").unwrap();
        stage(&store, "ku", "v2");
        store.commit("ku").unwrap();
        assert_eq!(
            fs::read_to_string(store.install_dir("ku").join("m.onnx")).unwrap(),
            "v2"
        );
    }

    #[test]
    fn a_failed_commit_keeps_the_previous_install() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        stage(&store, "ku", "v1");
        store.commit("ku").unwrap();
        assert!(store.commit("ku").is_err());
        assert_eq!(
            fs::read_to_string(store.install_dir("ku").join("m.onnx")).unwrap(),
            "v1"
        );
    }

    #[test]
    fn a_stranded_backup_is_restored_when_the_target_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        stage(&store, "ku", "v1");
        store.commit("ku").unwrap();
        fs::rename(store.install_dir("ku"), dir.path().join(".old/piper/ku")).unwrap();
        assert!(!store.install_dir("ku").exists());
        store.with_lock("ku", || Ok(())).unwrap();
        assert_eq!(
            fs::read_to_string(store.install_dir("ku").join("m.onnx")).unwrap(),
            "v1"
        );
    }

    #[cfg(windows)]
    #[test]
    fn remove_leaves_the_voice_untouched_when_a_file_is_open() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        stage(&store, "ku", "x");
        store.commit("ku").unwrap();
        let path = store.install_dir("ku").join("m.onnx");
        let _open = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        assert!(matches!(store.remove("ku"), Err(VoiceError::Io(..))));
        assert!(path.exists());
    }

    #[test]
    fn a_removed_voice_is_not_resurrected_by_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        stage(&store, "ku", "x");
        store.commit("ku").unwrap();
        store.remove("ku").unwrap();
        assert!(!dir.path().join(".trash/piper/ku").exists());
        let trash = dir.path().join(".trash/piper/ku");
        fs::create_dir_all(&trash).unwrap();
        fs::write(trash.join("m.onnx"), "x").unwrap();
        store.with_lock("ku", || Ok(())).unwrap();
        assert!(!store.install_dir("ku").exists());
    }

    #[test]
    fn commit_restores_a_stranded_backup_instead_of_deleting_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        stage(&store, "ku", "v1");
        store.commit("ku").unwrap();
        fs::rename(store.install_dir("ku"), dir.path().join(".old/piper/ku")).unwrap();
        stage(&store, "ku", "v2");
        store.commit("ku").unwrap();
        assert_eq!(
            fs::read_to_string(store.install_dir("ku").join("m.onnx")).unwrap(),
            "v2"
        );
        assert!(!dir.path().join(".old/piper/ku").exists());
    }

    #[test]
    fn commit_without_staging_restores_a_stranded_backup() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        stage(&store, "ku", "v1");
        store.commit("ku").unwrap();
        fs::rename(store.install_dir("ku"), dir.path().join(".old/piper/ku")).unwrap();
        assert!(store.commit("ku").is_err());
        assert_eq!(
            fs::read_to_string(store.install_dir("ku").join("m.onnx")).unwrap(),
            "v1"
        );
    }

    #[test]
    fn keys_are_validated_under_the_lock_and_on_commit() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        let bad = VoiceError::InvalidKey("../x".into());
        assert_eq!(store.with_lock("../x", || Ok(())), Err(bad.clone()));
        assert_eq!(store.commit("../x"), Err(bad));
        assert!(!dir.path().join("x.lock").exists());
        assert!(!dir.path().join(".locks/x.lock").exists());
        assert!(!dir.path().join(".locks/piper/x.lock").exists());
    }

    #[test]
    fn remove_clears_a_stale_backup_so_the_voice_stays_removed() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        stage(&store, "ku", "v1");
        store.commit("ku").unwrap();
        let old = dir.path().join(".old/piper/ku");
        fs::create_dir_all(&old).unwrap();
        fs::write(old.join("m.onnx"), "v0").unwrap();
        assert_eq!(store.remove("ku"), Ok(()));
        assert!(!store.install_dir("ku").exists());
        assert!(!old.exists());
        store.with_lock("ku", || Ok(())).unwrap();
        assert!(!store.install_dir("ku").exists());
    }

    #[test]
    fn installed_lists_voices_with_a_sidecar_sorted_by_key() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        for key in ["ku_b", "ku_a"] {
            stage(&store, key, "x");
            store.commit(key).unwrap();
        }
        fs::create_dir_all(store.install_dir("no_sidecar")).unwrap();
        let listed = store.installed().unwrap();
        let keys: Vec<_> = listed.iter().map(|v| v.key.as_str()).collect();
        assert_eq!(keys, ["ku_a", "ku_b"]);
        assert_eq!(listed[0].model_type, "piper");
        assert_eq!(listed[0].language, "ku_TR");
    }

    #[test]
    fn installed_is_empty_before_anything_is_installed() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(store(&dir).installed(), Ok(Vec::new()));
    }

    #[test]
    fn remove_deletes_the_voice_and_reports_a_missing_one() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        stage(&store, "ku", "x");
        store.commit("ku").unwrap();
        assert_eq!(store.remove("ku"), Ok(()));
        assert!(!store.install_dir("ku").exists());
        assert!(!dir.path().join(".old/piper/ku").exists());
        assert_eq!(
            store.remove("ku"),
            Err(VoiceError::NotInstalled("ku".into()))
        );
        assert_eq!(
            store.remove("../x"),
            Err(VoiceError::InvalidKey("../x".into()))
        );
    }

    #[test]
    fn the_lock_is_exclusive_per_key() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        let result = store.with_lock("ku", || store.with_lock("ku", || Ok(())));
        assert_eq!(result, Err(VoiceError::Busy("ku".into())));
        assert_eq!(
            store.with_lock("ku", || store.with_lock("de", || Ok(7))),
            Ok(7)
        );
        assert_eq!(store.with_lock("ku", || Ok(1)), Ok(1));
    }

    #[test]
    fn remove_is_busy_while_the_voice_is_locked() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        let result = store.with_lock("ku", || store.remove("ku"));
        assert_eq!(result, Err(VoiceError::Busy("ku".into())));
    }

    fn read_model(store: &Store, key: &str) -> String {
        fs::read_to_string(store.install_dir(key).join("m.onnx")).unwrap()
    }

    #[test]
    fn model_types_sharing_a_root_keep_separate_working_directories() {
        let dir = tempfile::tempdir().unwrap();
        let (piper, kokoro) = (
            store(&dir),
            Store::new(dir.path().into(), "kokoro").unwrap(),
        );
        for (store, content) in [(&piper, "piper"), (&kokoro, "kokoro")] {
            stage(store, "ku", content);
        }
        assert_ne!(piper.staging_dir("ku"), kokoro.staging_dir("ku"));
        piper.commit("ku").unwrap();
        kokoro.commit("ku").unwrap();
        assert_eq!(read_model(&piper, "ku"), "piper");
        assert_eq!(read_model(&kokoro, "ku"), "kokoro");
        assert_eq!(
            piper.with_lock("ku", || kokoro.with_lock("ku", || Ok(1))),
            Ok(1)
        );
        kokoro.remove("ku").unwrap();
        fs::rename(piper.install_dir("ku"), piper.old_dir("ku")).unwrap();
        kokoro.with_lock("ku", || Ok(())).unwrap();
        assert!(!kokoro.install_dir("ku").exists());
        piper.with_lock("ku", || Ok(())).unwrap();
        assert_eq!(read_model(&piper, "ku"), "piper");
    }

    #[test]
    fn the_catalogue_cache_round_trips_per_url() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        assert_eq!(store.read_cache("https://a/v.json"), Ok(None));
        store.write_cache("https://a/v.json", b"{}").unwrap();
        assert_eq!(
            store.read_cache("https://a/v.json"),
            Ok(Some(b"{}".to_vec()))
        );
        assert_eq!(store.read_cache("https://b/v.json"), Ok(None));
    }
}
