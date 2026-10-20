use crate::catalogue::{Catalogue, Voice, VoiceFile, MAX_CATALOGUE_BYTES};
use crate::digest::Digest;
use crate::error::{io_at, VoiceError, VoiceResult};
use crate::store::{InstalledVoice, Store};
use crate::transport::{Response, Transport};
use crate::validate::{file_name, validate_key};
use std::cmp::Ordering;
use std::collections::HashSet;
use std::fmt::Write as _;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

const CHUNK: usize = 64 * 1024;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub voices_dir: PathBuf,
    pub model_type: String,
    pub catalogue_url: String,
    pub files_base_url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub done: u64,
    pub total: u64,
}

struct Reporter<'a> {
    done: u64,
    total: u64,
    last: Option<Instant>,
    sink: &'a mut dyn FnMut(Progress),
}

impl<'a> Reporter<'a> {
    fn new(total: u64, sink: &'a mut dyn FnMut(Progress)) -> Self {
        Self {
            done: 0,
            total,
            last: None,
            sink,
        }
    }

    fn add(&mut self, bytes: u64) {
        self.done += bytes;
        self.emit(false);
    }

    fn finish(&mut self) {
        self.emit(true);
    }

    fn emit(&mut self, force: bool) {
        if force || self.last.is_none_or(|at| at.elapsed() >= PROGRESS_INTERVAL) {
            self.last = Some(Instant::now());
            (self.sink)(Progress {
                done: self.done,
                total: self.total,
            });
        }
    }
}

fn discard(path: &Path) -> VoiceResult<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(VoiceError::io(path, &error)),
        _ => Ok(()),
    }
}

fn usable_len(part: &Path, expected: u64) -> VoiceResult<u64> {
    match fs::metadata(part) {
        Ok(meta) if meta.len() <= expected => Ok(meta.len()),
        Ok(_) => discard(part).map(|()| 0),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(VoiceError::io(part, &error)),
    }
}

fn part_name(name: &str, digest: &Digest) -> String {
    let prefix: String = digest.hex().chars().take(16).collect();
    format!("{name}.{prefix}.part")
}

fn keep_names(name: &str, digest: &Digest) -> [String; 2] {
    [part_name(name, digest), name.to_owned()]
}

fn prune_staging(dir: &Path, keep: &HashSet<String>) -> VoiceResult<()> {
    fs::read_dir(dir)
        .map_err(io_at(dir))?
        .flatten()
        .filter(|entry| !keep.contains(entry.file_name().to_string_lossy().as_ref()))
        .try_for_each(|entry| discard(&entry.path()))
}

fn read_capped(body: impl Read) -> VoiceResult<Vec<u8>> {
    let mut bytes = Vec::new();
    body.take(MAX_CATALOGUE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| VoiceError::Network(error.to_string()))?;
    Ok(bytes)
}

fn expect_ok(status: u16, url: &str) -> VoiceResult<()> {
    (status == 200)
        .then_some(())
        .ok_or_else(|| VoiceError::Network(format!("HTTP {status} for {url}")))
}

fn read_step(
    body: &mut impl Read,
    out: &mut File,
    buffer: &mut [u8],
    part: &Path,
    cancelled: &dyn Fn() -> bool,
) -> VoiceResult<usize> {
    if cancelled() {
        return Err(VoiceError::Cancelled);
    }
    let read = body
        .read(buffer)
        .map_err(|error| VoiceError::Network(error.to_string()))?;
    out.write_all(buffer.get(..read).unwrap_or_default())
        .map_err(io_at(part))?;
    Ok(read)
}

fn copy_body(
    mut body: impl Read,
    mut out: File,
    part: &Path,
    wanted: u64,
    reporter: &mut Reporter<'_>,
    cancelled: &dyn Fn() -> bool,
) -> VoiceResult<()> {
    let (mut buffer, mut left) = (vec![0u8; CHUNK], wanted);
    loop {
        let read = read_step(&mut body, &mut out, &mut buffer, part, cancelled)?;
        if read == 0 {
            return Ok(());
        }
        reporter.add((read as u64).min(left));
        left = left.saturating_sub(read as u64);
    }
}

fn open_part(part: &Path, append: bool) -> VoiceResult<File> {
    let mut options = OpenOptions::new();
    match append {
        true => options.append(true).create(true),
        false => options.write(true).create(true).truncate(true),
    };
    options.open(part).map_err(io_at(part))
}

fn finished(path: &Path, file: &VoiceFile) -> VoiceResult<bool> {
    match fs::metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(VoiceError::io(path, &error)),
        Ok(meta)
            if meta.len() == file.size && file.digest.matches(path).map_err(io_at(path))? =>
        {
            Ok(true)
        }
        Ok(_) => discard(path).map(|()| false),
    }
}

struct Target<'a> {
    url: &'a str,
    part: &'a Path,
    file: &'a VoiceFile,
}

fn save(
    body: Box<dyn Read>,
    target: &Target<'_>,
    resume_from: Option<u64>,
    reporter: &mut Reporter<'_>,
    cancelled: &dyn Fn() -> bool,
) -> VoiceResult<()> {
    let start = resume_from.unwrap_or(0);
    let wanted = target.file.size.saturating_sub(start);
    let out = open_part(target.part, resume_from.is_some())?;
    reporter.add(start);
    let body = body.take(wanted.saturating_add(1));
    copy_body(body, out, target.part, wanted, reporter, cancelled)
}

fn check_digest(part: &Path, file: &VoiceFile) -> VoiceResult<()> {
    match file.digest.matches(part) {
        Ok(true) => Ok(()),
        Ok(false) => discard(part).and(Err(VoiceError::DigestMismatch(part.to_path_buf()))),
        Err(error) => Err(VoiceError::io(part, &error)),
    }
}

fn verify(part: &Path, file: &VoiceFile) -> VoiceResult<()> {
    let len = fs::metadata(part).map_err(io_at(part))?.len();
    match len.cmp(&file.size) {
        Ordering::Less => Err(VoiceError::Network(format!(
            "download ended after {len} of {} bytes",
            file.size
        ))),
        Ordering::Greater => discard(part).and(Err(VoiceError::SizeMismatch(part.to_path_buf()))),
        Ordering::Equal => check_digest(part, file),
    }
}

pub struct VoiceManager<T: Transport> {
    config: Config,
    store: Store,
    transport: T,
    catalogue: Mutex<Option<Catalogue>>,
}

impl<T: Transport> VoiceManager<T> {
    pub fn new(config: Config, transport: T) -> VoiceResult<Self> {
        let store = Store::new(config.voices_dir.clone(), &config.model_type)?;
        Ok(Self {
            config,
            store,
            transport,
            catalogue: Mutex::new(None),
        })
    }

    fn slot(&self) -> MutexGuard<'_, Option<Catalogue>> {
        self.catalogue
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    pub fn set_catalogue(&self, catalogue: Catalogue) {
        *self.slot() = Some(catalogue);
    }

    fn adopt(&self, bytes: &[u8]) -> VoiceResult<usize> {
        let catalogue = Catalogue::from_slice(bytes)?;
        let count = catalogue.len();
        self.set_catalogue(catalogue);
        Ok(count)
    }

    pub fn fetch_catalogue(&self) -> VoiceResult<usize> {
        let url = &self.config.catalogue_url;
        let response = self.transport.get(url, None)?;
        expect_ok(response.status, url)?;
        let bytes = read_capped(response.body)?;
        let catalogue = Catalogue::from_slice(&bytes)?;
        self.store.write_cache(url, &bytes)?;
        let count = catalogue.len();
        self.set_catalogue(catalogue);
        Ok(count)
    }

    pub fn load_cached_catalogue(&self) -> VoiceResult<usize> {
        let cached = self.store.read_cache(&self.config.catalogue_url)?;
        self.adopt(&cached.ok_or(VoiceError::NoCatalogue)?)
    }

    pub fn list(&self, language: Option<&str>) -> VoiceResult<Vec<Voice>> {
        let slot = self.slot();
        let catalogue = slot.as_ref().ok_or(VoiceError::NoCatalogue)?;
        Ok(catalogue
            .by_language(language)
            .into_iter()
            .cloned()
            .collect())
    }

    pub fn installed(&self) -> VoiceResult<Vec<InstalledVoice>> {
        self.store.installed()
    }

    pub fn remove(&self, key: &str) -> VoiceResult<()> {
        self.store.remove(key)
    }

    fn voice(&self, key: &str) -> VoiceResult<Voice> {
        let slot = self.slot();
        let catalogue = slot.as_ref().ok_or(VoiceError::NoCatalogue)?;
        catalogue
            .get(key)
            .cloned()
            .ok_or_else(|| VoiceError::UnknownVoice(key.to_owned()))
    }

    pub fn install(
        &self,
        key: &str,
        on_progress: &mut dyn FnMut(Progress),
        cancelled: &dyn Fn() -> bool,
    ) -> VoiceResult<()> {
        let key = validate_key(key)?;
        let voice = self.voice(key)?;
        self.store.with_lock(key, || {
            self.stage_and_commit(&voice, on_progress, cancelled)
        })
    }

    fn prepare_staging(&self, voice: &Voice) -> VoiceResult<PathBuf> {
        let staging = self.store.staging_dir(&voice.key);
        fs::create_dir_all(&staging).map_err(io_at(&staging))?;
        let keep = voice
            .files
            .iter()
            .map(|file| file_name(&file.path).map(|name| keep_names(name, &file.digest)))
            .collect::<VoiceResult<Vec<_>>>()?;
        prune_staging(&staging, &keep.into_iter().flatten().collect()).map(|()| staging)
    }

    fn stage_and_commit(
        &self,
        voice: &Voice,
        on_progress: &mut dyn FnMut(Progress),
        cancelled: &dyn Fn() -> bool,
    ) -> VoiceResult<()> {
        let staging = self.prepare_staging(voice)?;
        let mut reporter = Reporter::new(voice.total_bytes(), on_progress);
        voice
            .files
            .iter()
            .try_for_each(|file| self.fetch_verified(&staging, file, &mut reporter, cancelled))?;
        reporter.finish();
        self.store.write_sidecar(&staging, voice)?;
        self.store.commit(&voice.key)
    }

    fn fetch_verified(
        &self,
        dir: &Path,
        file: &VoiceFile,
        reporter: &mut Reporter<'_>,
        cancelled: &dyn Fn() -> bool,
    ) -> VoiceResult<()> {
        let name = file_name(&file.path)?;
        let (part, done) = (dir.join(part_name(name, &file.digest)), dir.join(name));
        if finished(&done, file)? {
            reporter.add(file.size);
            return Ok(());
        }
        self.fetch_part(&part, file, reporter, cancelled)?;
        fs::rename(&part, &done).map_err(io_at(&part))
    }

    fn fetch_part(
        &self,
        part: &Path,
        file: &VoiceFile,
        reporter: &mut Reporter<'_>,
        cancelled: &dyn Fn() -> bool,
    ) -> VoiceResult<()> {
        let url = self.url_of(file);
        let target = Target {
            url: &url,
            part,
            file,
        };
        self.download(&target, reporter, cancelled)?;
        verify(part, file)
    }

    fn url_of(&self, file: &VoiceFile) -> String {
        let base = self.config.files_base_url.trim_end_matches('/');
        format!("{base}/{}", encode_path(&file.path))
    }

    fn download(
        &self,
        target: &Target<'_>,
        reporter: &mut Reporter<'_>,
        cancelled: &dyn Fn() -> bool,
    ) -> VoiceResult<()> {
        let have = usable_len(target.part, target.file.size)?;
        if have == target.file.size {
            reporter.add(have);
            return Ok(());
        }
        let response = self.transport.get(target.url, (have > 0).then_some(have))?;
        self.receive(response, have, target, reporter, cancelled)
    }

    fn receive(
        &self,
        response: Response,
        have: u64,
        target: &Target<'_>,
        reporter: &mut Reporter<'_>,
        cancelled: &dyn Fn() -> bool,
    ) -> VoiceResult<()> {
        match response.status {
            206 => save(response.body, target, Some(have), reporter, cancelled),
            200 => save(response.body, target, None, reporter, cancelled),
            416 if have > 0 => {
                discard(target.part).and_then(|()| self.download(target, reporter, cancelled))
            }
            status => Err(VoiceError::Network(format!(
                "HTTP {status} for {}",
                target.url
            ))),
        }
    }
}

fn encode_path(path: &str) -> String {
    path.bytes()
        .fold(String::with_capacity(path.len()), |mut out, byte| {
            if byte.is_ascii() {
                out.push(char::from(byte));
            } else {
                let _ = write!(out, "%{byte:02X}");
            }
            out
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::digest::sha256_hex;
    use crate::error::VoiceError;
    use crate::store::InstalledVoice;
    use crate::transport::Response;
    use std::collections::HashMap;
    use std::io::Cursor;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    const CATALOGUE_URL: &str = "https://cat.test/voices.json";
    const FILES: &str = "https://files.test";
    const KEY: &str = "ku_TR-x-medium";
    const ONNX_URL: &str = "https://files.test/ku/x/ku_TR-x-medium.onnx";
    const JSON_URL: &str = "https://files.test/ku/x/ku_TR-x-medium.onnx.json";
    const CARD_URL: &str = "https://files.test/ku/x/MODEL_CARD";

    #[derive(Clone, Copy)]
    enum Mode {
        Honour,
        Ignore,
        Reject,
    }

    type Log = Arc<Mutex<Vec<(String, Option<u64>)>>>;

    struct Fake {
        files: HashMap<String, Vec<u8>>,
        mode: Mode,
        cut: Arc<Mutex<Option<usize>>>,
        log: Log,
        broken: Arc<Mutex<Option<String>>>,
        endless: Option<(String, Arc<AtomicUsize>)>,
    }

    struct Endless(Arc<AtomicUsize>);

    impl Read for Endless {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            buf.fill(1);
            self.0.fetch_add(buf.len(), Ordering::SeqCst);
            Ok(buf.len())
        }
    }

    fn reply(status: u16, body: Vec<u8>) -> Response {
        Response {
            status,
            content_range_total: None,
            body: Box::new(Cursor::new(body)),
        }
    }

    impl Transport for Fake {
        fn get(&self, url: &str, range_from: Option<u64>) -> VoiceResult<Response> {
            self.log.lock().unwrap().push((url.to_owned(), range_from));
            if self.broken.lock().unwrap().as_deref() == Some(url) {
                return Ok(reply(500, Vec::new()));
            }
            if let Some((target, counter)) = &self.endless {
                if target == url {
                    let body = Box::new(Endless(counter.clone()));
                    return Ok(Response {
                        status: 200,
                        content_range_total: None,
                        body,
                    });
                }
            }
            let Some(data) = self.files.get(url) else {
                return Ok(reply(404, Vec::new()));
            };
            let cut = *self.cut.lock().unwrap();
            let served = |bytes: &[u8]| match cut {
                Some(limit) if bytes.len() > limit => bytes[..limit].to_vec(),
                _ => bytes.to_vec(),
            };
            Ok(match (range_from, self.mode) {
                (Some(from), Mode::Honour) => reply(206, served(&data[from as usize..])),
                (Some(_), Mode::Reject) => reply(416, Vec::new()),
                _ => reply(200, served(data)),
            })
        }
    }

    struct Fixture {
        onnx: Vec<u8>,
        json: Vec<u8>,
    }

    fn fixture() -> Fixture {
        Fixture {
            onnx: (0..200 * 1024).map(|i| (i % 251) as u8).collect(),
            json: b"{\"audio\":{}}".to_vec(),
        }
    }

    fn catalogue_json(f: &Fixture) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            KEY: {
                "key": KEY, "name": "x", "quality": "medium", "num_speakers": 1,
                "language": {"code": "ku_TR", "family": "ku"},
                "files": {
                    "ku/x/ku_TR-x-medium.onnx":
                        {"size_bytes": f.onnx.len(), "sha256_digest": sha256_hex(&f.onnx)},
                    "ku/x/ku_TR-x-medium.onnx.json":
                        {"size_bytes": f.json.len(), "sha256_digest": sha256_hex(&f.json)},
                    "ku/x/MODEL_CARD":
                        {"size_bytes": 3, "md5_digest": "900150983cd24fb0d6963f7d28e17f72"}
                }
            }
        }))
        .unwrap()
    }

    fn fake(f: &Fixture, mode: Mode) -> Fake {
        let files = HashMap::from([
            (CATALOGUE_URL.to_owned(), catalogue_json(f)),
            (ONNX_URL.to_owned(), f.onnx.clone()),
            (JSON_URL.to_owned(), f.json.clone()),
            (CARD_URL.to_owned(), b"abc".to_vec()),
        ]);
        Fake {
            files,
            mode,
            cut: Arc::default(),
            log: Arc::default(),
            broken: Arc::default(),
            endless: None,
        }
    }

    fn manager(dir: &tempfile::TempDir, fake: Fake) -> VoiceManager<Fake> {
        let config = Config {
            voices_dir: dir.path().to_path_buf(),
            model_type: "piper".into(),
            catalogue_url: CATALOGUE_URL.into(),
            files_base_url: FILES.into(),
        };
        VoiceManager::new(config, fake).unwrap()
    }

    fn ready(dir: &tempfile::TempDir, fake: Fake) -> VoiceManager<Fake> {
        let manager = manager(dir, fake);
        assert_eq!(manager.fetch_catalogue(), Ok(1));
        manager
    }

    fn cancel_after_two_chunks(path: std::path::PathBuf) -> impl Fn() -> bool {
        move || std::fs::metadata(&path).is_ok_and(|meta| meta.len() >= 2 * CHUNK as u64)
    }

    fn install(manager: &VoiceManager<Fake>) -> VoiceResult<()> {
        manager.install(KEY, &mut |_| {}, &|| false)
    }

    fn part(dir: &tempfile::TempDir, name: &str, data: &[u8]) -> std::path::PathBuf {
        let prefix = &sha256_hex(data)[..16];
        dir.path()
            .join(".staging/piper")
            .join(KEY)
            .join(format!("{name}.{prefix}.part"))
    }

    #[test]
    fn fetch_caches_the_catalogue_for_offline_use() {
        let (f, dir) = (fixture(), tempfile::tempdir().unwrap());
        ready(&dir, fake(&f, Mode::Honour));
        let mut offline = fake(&f, Mode::Honour);
        offline.files.clear();
        let second = manager(&dir, offline);
        assert_eq!(second.list(None), Err(VoiceError::NoCatalogue));
        assert_eq!(second.load_cached_catalogue(), Ok(1));
        assert_eq!(second.list(Some("ku")).unwrap().len(), 1);
        assert!(second.list(Some("de")).unwrap().is_empty());
    }

    #[test]
    fn a_cache_write_failure_keeps_the_previous_catalogue_active() {
        let (f, dir) = (fixture(), tempfile::tempdir().unwrap());
        let manager = manager(&dir, fake(&f, Mode::Honour));
        manager.set_catalogue(Catalogue::default());
        std::fs::write(dir.path().join(".catalogue"), b"").unwrap();
        assert!(matches!(manager.fetch_catalogue(), Err(VoiceError::Io(..))));
        assert_eq!(manager.list(None), Ok(Vec::new()));
    }

    #[test]
    fn fetch_fails_on_a_bad_status_and_keeps_the_error_visible() {
        let (f, dir) = (fixture(), tempfile::tempdir().unwrap());
        let mut missing = fake(&f, Mode::Honour);
        missing.files.remove(CATALOGUE_URL);
        let manager = manager(&dir, missing);
        assert!(matches!(
            manager.fetch_catalogue(),
            Err(VoiceError::Network(_))
        ));
        assert_eq!(
            manager.load_cached_catalogue(),
            Err(VoiceError::NoCatalogue)
        );
    }

    #[test]
    fn encode_path_escapes_only_non_ascii_bytes() {
        assert_eq!(encode_path("pt/tugão/a.onnx"), "pt/tug%C3%A3o/a.onnx");
        assert_eq!(encode_path("ku/x/a_b-c+d.onnx"), "ku/x/a_b-c+d.onnx");
        assert_eq!(encode_path("a/b/c"), "a/b/c");
    }

    #[test]
    fn a_non_ascii_catalogue_path_is_requested_percent_encoded() {
        let (f, dir) = (fixture(), tempfile::tempdir().unwrap());
        let path = "pt/pt_PT/tugão/medium/pt_PT-tugao-medium.onnx";
        let catalogue = serde_json::to_vec(&serde_json::json!({"pt_PT-tugao-medium": {
            "key": "pt_PT-tugao-medium", "name": "tugao", "quality": "medium",
            "num_speakers": 1, "language": {"code": "pt_PT", "family": "pt"},
            "files": {path: {"size_bytes": f.json.len(), "sha256_digest": sha256_hex(&f.json)}}
        }}))
        .unwrap();
        let encoded = format!("{FILES}/pt/pt_PT/tug%C3%A3o/medium/pt_PT-tugao-medium.onnx");
        let mut transport = fake(&f, Mode::Honour);
        transport.files = HashMap::from([
            (CATALOGUE_URL.to_owned(), catalogue),
            (encoded.clone(), f.json.clone()),
        ]);
        let log = transport.log.clone();
        let manager = ready(&dir, transport);
        manager
            .install("pt_PT-tugao-medium", &mut |_| {}, &|| false)
            .unwrap();
        assert_eq!(
            log.lock().unwrap().last().map(|r| r.0.clone()),
            Some(encoded)
        );
        let installed = dir
            .path()
            .join("piper/pt_PT-tugao-medium/pt_PT-tugao-medium.onnx");
        assert_eq!(std::fs::read(installed).unwrap(), f.json);
    }

    #[test]
    fn install_writes_every_file_the_sidecar_and_reports_progress() {
        let (f, dir) = (fixture(), tempfile::tempdir().unwrap());
        let manager = ready(&dir, fake(&f, Mode::Honour));
        let mut last = None;
        manager
            .install(KEY, &mut |p| last = Some(p), &|| false)
            .unwrap();
        let total = (f.onnx.len() + f.json.len() + 3) as u64;
        assert_eq!(last, Some(Progress { done: total, total }));
        let installed = dir.path().join("piper").join(KEY);
        assert_eq!(
            std::fs::read(installed.join("ku_TR-x-medium.onnx")).unwrap(),
            f.onnx
        );
        assert_eq!(std::fs::read(installed.join("MODEL_CARD")).unwrap(), b"abc");
        assert!(!dir.path().join(".staging/piper").join(KEY).exists());
        assert_eq!(
            manager.installed().unwrap(),
            [InstalledVoice {
                key: KEY.into(),
                model_type: "piper".into(),
                name: "x".into(),
                language: "ku_TR".into(),
                description: String::new(),
            }]
        );
    }

    #[test]
    fn a_wrong_digest_fails_and_leaves_nothing_installed() {
        let (f, dir) = (fixture(), tempfile::tempdir().unwrap());
        let mut bad = fake(&f, Mode::Honour);
        bad.files.insert(ONNX_URL.into(), vec![9; f.onnx.len()]);
        let manager = ready(&dir, bad);
        assert!(matches!(
            install(&manager),
            Err(VoiceError::DigestMismatch(_))
        ));
        assert!(!part(&dir, "ku_TR-x-medium.onnx", &f.onnx).exists());
        assert!(manager.installed().unwrap().is_empty());
    }

    #[test]
    fn a_failed_reinstall_keeps_the_previous_voice() {
        let (f, dir) = (fixture(), tempfile::tempdir().unwrap());
        let manager = ready(&dir, fake(&f, Mode::Honour));
        install(&manager).unwrap();
        let installed = dir
            .path()
            .join("piper")
            .join(KEY)
            .join("ku_TR-x-medium.onnx");
        let bad = Fake {
            files: HashMap::new(),
            ..fake(&f, Mode::Honour)
        };
        let broken = manager_with_catalogue(&dir, bad, &f);
        assert!(matches!(install(&broken), Err(VoiceError::Network(_))));
        assert_eq!(std::fs::read(installed).unwrap(), f.onnx);
    }

    fn manager_with_catalogue(
        dir: &tempfile::TempDir,
        fake: Fake,
        f: &Fixture,
    ) -> VoiceManager<Fake> {
        let manager = manager(dir, fake);
        manager.set_catalogue(Catalogue::from_slice(&catalogue_json(f)).unwrap());
        manager
    }

    #[test]
    fn cancel_then_reinstall_resumes_from_the_partial_file() {
        let (f, dir) = (fixture(), tempfile::tempdir().unwrap());
        let fake = fake(&f, Mode::Honour);
        let log = fake.log.clone();
        let manager = ready(&dir, fake);
        let cancel = cancel_after_two_chunks(part(&dir, "ku_TR-x-medium.onnx", &f.onnx));
        let first = manager.install(KEY, &mut |_| {}, &cancel);
        assert_eq!(first, Err(VoiceError::Cancelled));
        let kept = std::fs::metadata(part(&dir, "ku_TR-x-medium.onnx", &f.onnx))
            .unwrap()
            .len();
        assert_eq!(kept, 2 * CHUNK as u64);
        install(&manager).unwrap();
        let ranges: Vec<_> = log.lock().unwrap().iter().filter_map(|(_, r)| *r).collect();
        assert_eq!(ranges, [kept]);
        let installed = dir
            .path()
            .join("piper")
            .join(KEY)
            .join("ku_TR-x-medium.onnx");
        assert_eq!(std::fs::read(installed).unwrap(), f.onnx);
    }

    #[test]
    fn a_server_that_ignores_range_still_yields_a_correct_file() {
        let (f, dir) = (fixture(), tempfile::tempdir().unwrap());
        let fake = fake(&f, Mode::Ignore);
        let log = fake.log.clone();
        let manager = ready(&dir, fake);
        let cancel = cancel_after_two_chunks(part(&dir, "ku_TR-x-medium.onnx", &f.onnx));
        let first = manager.install(KEY, &mut |_| {}, &cancel);
        assert_eq!(first, Err(VoiceError::Cancelled));
        install(&manager).unwrap();
        let ranged = log
            .lock()
            .unwrap()
            .iter()
            .any(|(url, range)| url == ONNX_URL && range.is_some());
        assert!(ranged);
        let installed = dir
            .path()
            .join("piper")
            .join(KEY)
            .join("ku_TR-x-medium.onnx");
        assert_eq!(std::fs::read(installed).unwrap(), f.onnx);
    }

    #[test]
    fn a_failure_on_a_later_file_keeps_the_verified_earlier_files() {
        let (f, dir) = (fixture(), tempfile::tempdir().unwrap());
        let fake = fake(&f, Mode::Honour);
        let (log, broken) = (fake.log.clone(), fake.broken.clone());
        let manager = ready(&dir, fake);
        *broken.lock().unwrap() = Some(JSON_URL.into());
        assert!(matches!(install(&manager), Err(VoiceError::Network(_))));
        *broken.lock().unwrap() = None;
        install(&manager).unwrap();
        let count = |wanted: &str| {
            log.lock()
                .unwrap()
                .iter()
                .filter(|(url, _)| url == wanted)
                .count()
        };
        assert_eq!(
            (count(ONNX_URL), count(CARD_URL), count(JSON_URL)),
            (1, 1, 2)
        );
    }

    #[test]
    fn an_oversized_body_is_cut_off_near_the_expected_length() {
        let (f, dir) = (fixture(), tempfile::tempdir().unwrap());
        let counter = Arc::new(AtomicUsize::new(0));
        let mut endless = fake(&f, Mode::Honour);
        endless.endless = Some((ONNX_URL.into(), counter.clone()));
        let manager = ready(&dir, endless);
        assert!(matches!(
            install(&manager),
            Err(VoiceError::SizeMismatch(_))
        ));
        assert!(counter.load(Ordering::SeqCst) <= f.onnx.len() + 1 + CHUNK);
        assert!(!part(&dir, "ku_TR-x-medium.onnx", &f.onnx).exists());
    }

    #[test]
    fn a_server_that_rejects_range_restarts_from_zero() {
        let (f, dir) = (fixture(), tempfile::tempdir().unwrap());
        let fake = fake(&f, Mode::Reject);
        let log = fake.log.clone();
        let manager = ready(&dir, fake);
        let cancel = cancel_after_two_chunks(part(&dir, "ku_TR-x-medium.onnx", &f.onnx));
        let _ = manager.install(KEY, &mut |_| {}, &cancel);
        install(&manager).unwrap();
        let onnx_requests: Vec<_> = log
            .lock()
            .unwrap()
            .iter()
            .filter(|(url, _)| url == ONNX_URL)
            .map(|(_, r)| *r)
            .collect();
        assert_eq!(onnx_requests, [None, Some(2 * CHUNK as u64), None]);
        let installed = dir
            .path()
            .join("piper")
            .join(KEY)
            .join("ku_TR-x-medium.onnx");
        assert_eq!(std::fs::read(installed).unwrap(), f.onnx);
    }

    #[test]
    fn a_body_longer_than_the_catalogue_size_is_rejected_and_discarded() {
        let (f, dir) = (fixture(), tempfile::tempdir().unwrap());
        let mut long = fake(&f, Mode::Honour);
        long.files
            .insert(JSON_URL.into(), [f.json.clone(), vec![0; 5]].concat());
        let manager = ready(&dir, long);
        assert!(matches!(
            install(&manager),
            Err(VoiceError::SizeMismatch(_))
        ));
        assert!(!part(&dir, "ku_TR-x-medium.onnx.json", &f.json).exists());
    }

    #[test]
    fn a_short_body_fails_but_keeps_the_partial_file_for_resume() {
        let (f, dir) = (fixture(), tempfile::tempdir().unwrap());
        let fake = fake(&f, Mode::Honour);
        let cut = fake.cut.clone();
        let manager = ready(&dir, fake);
        *cut.lock().unwrap() = Some(1000);
        assert!(matches!(install(&manager), Err(VoiceError::Network(_))));
        assert_eq!(
            std::fs::metadata(part(&dir, "ku_TR-x-medium.onnx", &f.onnx))
                .unwrap()
                .len(),
            1000
        );
        *cut.lock().unwrap() = None;
        install(&manager).unwrap();
    }

    #[test]
    fn unknown_unsafe_and_unloaded_requests_are_rejected() {
        let (f, dir) = (fixture(), tempfile::tempdir().unwrap());
        let manager = manager(&dir, fake(&f, Mode::Honour));
        assert_eq!(install(&manager), Err(VoiceError::NoCatalogue));
        manager.fetch_catalogue().unwrap();
        assert_eq!(
            manager.install("de_DE-none-low", &mut |_| {}, &|| false),
            Err(VoiceError::UnknownVoice("de_DE-none-low".into()))
        );
        assert_eq!(
            manager.install("../x", &mut |_| {}, &|| false),
            Err(VoiceError::InvalidKey("../x".into()))
        );
    }

    #[test]
    fn install_and_remove_report_busy_while_the_voice_is_locked() {
        let (f, dir) = (fixture(), tempfile::tempdir().unwrap());
        let manager = ready(&dir, fake(&f, Mode::Honour));
        let store = Store::new(dir.path().to_path_buf(), "piper").unwrap();
        let result = store.with_lock(KEY, || install(&manager));
        assert_eq!(result, Err(VoiceError::Busy(KEY.into())));
        let result = store.with_lock(KEY, || manager.remove(KEY));
        assert_eq!(result, Err(VoiceError::Busy(KEY.into())));
    }

    #[test]
    fn a_partial_file_for_another_digest_is_discarded_and_never_shipped() {
        let (f, dir) = (fixture(), tempfile::tempdir().unwrap());
        let fake = fake(&f, Mode::Honour);
        let log = fake.log.clone();
        let manager = ready(&dir, fake);
        let staging = dir.path().join(".staging/piper").join(KEY);
        std::fs::create_dir_all(&staging).unwrap();
        let stale = staging.join("ku_TR-x-medium.onnx.0000000000000000.part");
        std::fs::write(&stale, vec![7; 5000]).unwrap();
        install(&manager).unwrap();
        assert!(log.lock().unwrap().iter().all(|(_, range)| range.is_none()));
        assert!(!stale.exists());
        let installed = dir.path().join("piper").join(KEY);
        let leftovers = std::fs::read_dir(installed)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".part"))
            .count();
        assert_eq!(leftovers, 0);
    }

    #[test]
    fn remove_deletes_an_installed_voice() {
        let (f, dir) = (fixture(), tempfile::tempdir().unwrap());
        let manager = ready(&dir, fake(&f, Mode::Honour));
        install(&manager).unwrap();
        assert_eq!(manager.remove(KEY), Ok(()));
        assert!(manager.installed().unwrap().is_empty());
        assert_eq!(
            manager.remove(KEY),
            Err(VoiceError::NotInstalled(KEY.into()))
        );
    }
}
