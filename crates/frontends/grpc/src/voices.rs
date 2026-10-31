//! Voice catalogue, install and removal behind the voice-manager RPCs: env config, error mapping and the manager seam.

use dengjen_voices::{
    Config, InstalledVoice, Progress, Transport, UreqTransport, Voice, VoiceError, VoiceManager,
    VoiceResult,
};
use std::path::PathBuf;
use std::sync::Arc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::Status;

const PROGRESS_BUFFER: usize = 16;

pub(crate) trait VoiceStore: Send + Sync {
    fn fetch_catalogue(&self) -> VoiceResult<usize>;
    fn list(&self, language: Option<&str>) -> VoiceResult<Vec<Voice>>;
    fn installed(&self) -> VoiceResult<Vec<InstalledVoice>>;
    fn remove(&self, key: &str) -> VoiceResult<()>;
    fn install(
        &self,
        key: &str,
        on_progress: &mut dyn FnMut(Progress),
        cancelled: &dyn Fn() -> bool,
    ) -> VoiceResult<()>;
}

impl<T: Transport> VoiceStore for VoiceManager<T> {
    fn fetch_catalogue(&self) -> VoiceResult<usize> {
        VoiceManager::fetch_catalogue(self)
    }

    fn list(&self, language: Option<&str>) -> VoiceResult<Vec<Voice>> {
        VoiceManager::list(self, language)
    }

    fn installed(&self) -> VoiceResult<Vec<InstalledVoice>> {
        VoiceManager::installed(self)
    }

    fn remove(&self, key: &str) -> VoiceResult<()> {
        VoiceManager::remove(self, key)
    }

    fn install(
        &self,
        key: &str,
        on_progress: &mut dyn FnMut(Progress),
        cancelled: &dyn Fn() -> bool,
    ) -> VoiceResult<()> {
        VoiceManager::install(self, key, on_progress, cancelled)
    }
}

pub(crate) fn open_store(config: Option<Config>) -> Result<Option<Arc<dyn VoiceStore>>, String> {
    let Some(config) = config else {
        return Ok(None);
    };
    let manager = VoiceManager::new(config, UreqTransport::new())
        .map_err(|error| format!("Could not open the voice directory: {error}"))?;
    if let Err(error) = manager.load_cached_catalogue() {
        log::info!("No cached voice catalogue: {error}");
    }
    Ok(Some(Arc::new(manager)))
}

async fn blocking<T: Send + 'static>(
    store: Arc<dyn VoiceStore>,
    work: impl FnOnce(&dyn VoiceStore) -> VoiceResult<T> + Send + 'static,
) -> Result<T, Status> {
    tokio::task::spawn_blocking(move || work(store.as_ref()))
        .await
        .map_err(|_| Status::internal("Voice task failed"))?
        .map_err(voice_status)
}

pub(crate) async fn fetch_catalogue(store: Arc<dyn VoiceStore>) -> Result<usize, Status> {
    blocking(store, |store| store.fetch_catalogue()).await
}

pub(crate) async fn list_voices(
    store: Arc<dyn VoiceStore>,
    language: Option<String>,
) -> Result<Vec<Voice>, Status> {
    blocking(store, move |store| store.list(language.as_deref())).await
}

pub(crate) async fn list_installed(
    store: Arc<dyn VoiceStore>,
) -> Result<Vec<InstalledVoice>, Status> {
    blocking(store, |store| store.installed()).await
}

pub(crate) async fn remove_voice(store: Arc<dyn VoiceStore>, key: String) -> Result<(), Status> {
    blocking(store, move |store| store.remove(&key)).await
}

pub(crate) fn install_voice(
    store: Arc<dyn VoiceStore>,
    key: String,
) -> ReceiverStream<Result<Progress, Status>> {
    let (sender, receiver) = tokio::sync::mpsc::channel(PROGRESS_BUFFER);
    tokio::task::spawn_blocking(move || {
        let outcome = store.install(
            &key,
            &mut |progress| {
                let _ = sender.try_send(Ok(progress));
            },
            &|| sender.is_closed(),
        );
        if let Err(error) = outcome {
            let _ = sender.blocking_send(Err(voice_status(error)));
        }
    });
    ReceiverStream::new(receiver)
}

pub(crate) const VOICES_DIR_VAR: &str = "DENGJEN_GRPC_VOICES_DIR";
pub(crate) const CATALOGUE_URL_VAR: &str = "DENGJEN_GRPC_VOICES_CATALOGUE_URL";
pub(crate) const FILES_BASE_URL_VAR: &str = "DENGJEN_GRPC_VOICES_FILES_BASE_URL";
pub(crate) const MODEL_TYPE_VAR: &str = "DENGJEN_GRPC_VOICES_MODEL_TYPE";
const DEFAULT_MODEL_TYPE: &str = "piper";

pub(crate) fn voices_config_from_lookup(
    lookup: impl Fn(&str) -> Option<String>,
) -> Result<Option<Config>, String> {
    let get = |name: &str| {
        lookup(name)
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    };
    match [VOICES_DIR_VAR, CATALOGUE_URL_VAR, FILES_BASE_URL_VAR].map(get) {
        [None, None, None] => Ok(None),
        [Some(dir), Some(catalogue_url), Some(files_base_url)] => Ok(Some(Config {
            voices_dir: PathBuf::from(dir),
            model_type: get(MODEL_TYPE_VAR).unwrap_or_else(|| DEFAULT_MODEL_TYPE.to_owned()),
            catalogue_url,
            files_base_url,
        })),
        [dir, catalogue, files] => {
            let missing: Vec<&str> = [
                (VOICES_DIR_VAR, dir),
                (CATALOGUE_URL_VAR, catalogue),
                (FILES_BASE_URL_VAR, files),
            ]
            .into_iter()
            .filter_map(|(name, value)| value.is_none().then_some(name))
            .collect();
            Err(format!(
                "{} must be set together with the other voice variables",
                missing.join(", ")
            ))
        }
    }
}

pub(crate) fn voice_status(error: VoiceError) -> Status {
    log::warn!("voice request failed: {error}");
    match error {
        VoiceError::Busy(key) => {
            Status::aborted(format!("Voice {key} is being changed by another request"))
        }
        VoiceError::UnknownVoice(key) => Status::not_found(format!("Unknown voice: {key}")),
        VoiceError::NotInstalled(key) => Status::not_found(format!("Voice not installed: {key}")),
        VoiceError::InvalidKey(key) => {
            Status::invalid_argument(format!("Invalid voice key: {key}"))
        }
        VoiceError::NoCatalogue => {
            Status::failed_precondition("No voice catalogue is loaded; fetch it first")
        }
        VoiceError::CatalogueParse(_) | VoiceError::UnsafePath(_) => {
            Status::failed_precondition("The voice catalogue is invalid")
        }
        VoiceError::Network(_) => Status::unavailable("Could not reach the voice server"),
        VoiceError::DigestMismatch(_) | VoiceError::SizeMismatch(_) => {
            Status::data_loss("A downloaded voice file failed verification")
        }
        VoiceError::Cancelled => Status::cancelled("Voice request cancelled"),
        VoiceError::Io(..) => Status::internal("Voice storage error"),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::path::PathBuf;

    fn lookup_from<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_owned())
        }
    }

    const ALL: [(&str, &str); 3] = [
        (VOICES_DIR_VAR, "/v"),
        (CATALOGUE_URL_VAR, "https://example.test/voices.json"),
        (FILES_BASE_URL_VAR, "https://example.test/files"),
    ];

    #[test]
    fn no_variables_means_voice_management_is_not_configured() {
        assert_eq!(voices_config_from_lookup(lookup_from(&[])), Ok(None));
    }

    #[test]
    fn the_three_required_variables_build_a_piper_config() {
        let config = voices_config_from_lookup(lookup_from(&ALL))
            .unwrap()
            .unwrap();
        assert_eq!(config.voices_dir, PathBuf::from("/v"));
        assert_eq!(config.model_type, "piper");
        assert_eq!(config.catalogue_url, "https://example.test/voices.json");
        assert_eq!(config.files_base_url, "https://example.test/files");
    }

    #[test]
    fn the_model_type_variable_overrides_the_default() {
        let mut pairs = ALL.to_vec();
        pairs.push((MODEL_TYPE_VAR, "kokoro"));
        let config = voices_config_from_lookup(lookup_from(&pairs))
            .unwrap()
            .unwrap();
        assert_eq!(config.model_type, "kokoro");
    }

    #[test]
    fn a_partial_set_names_the_missing_variables() {
        let error = voices_config_from_lookup(lookup_from(&[(VOICES_DIR_VAR, "/v")])).unwrap_err();
        assert!(error.contains(CATALOGUE_URL_VAR), "{error}");
        assert!(error.contains(FILES_BASE_URL_VAR), "{error}");
        assert!(!error.contains(VOICES_DIR_VAR), "{error}");
    }

    use dengjen_voices::{Response, Transport, VoiceManager, VoiceResult};
    use std::collections::HashMap;
    use std::io::Cursor;
    use std::sync::Arc;
    use tonic::Code;

    pub(crate) const KEY: &str = "ku_TR-test-medium";
    const CATALOGUE_URL: &str = "https://voices.test/voices.json";
    const ABC_MD5: &str = "900150983cd24fb0d6963f7d28e17f72";

    struct InMemoryTransport(HashMap<String, Vec<u8>>);

    impl Transport for InMemoryTransport {
        fn get(&self, url: &str, range_from: Option<u64>) -> VoiceResult<Response> {
            let bytes = self
                .0
                .get(url)
                .ok_or_else(|| VoiceError::Network(format!("no route for {url}")))?;
            let from = range_from.map_or(0, |from| from as usize);
            Ok(Response {
                status: if range_from.is_some() { 206 } else { 200 },
                content_range_total: range_from.map(|_| bytes.len() as u64),
                body: Box::new(Cursor::new(bytes[from..].to_vec())),
            })
        }
    }

    fn catalogue_json() -> String {
        let file = format!(r#"{{"size_bytes": 3, "md5_digest": "{ABC_MD5}"}}"#);
        format!(
            r#"{{"{KEY}": {{"key": "{KEY}", "name": "test", "quality": "medium", "num_speakers": 1,
                "language": {{"code": "ku_TR", "family": "ku"}},
                "files": {{"ku/test/a.onnx": {file}, "ku/test/a.onnx.json": {file}, "ku/test/MODEL_CARD": {file}}}}}}}"#
        )
    }

    fn routes() -> InMemoryTransport {
        let mut routes = HashMap::from([(CATALOGUE_URL.to_owned(), catalogue_json().into_bytes())]);
        for name in ["a.onnx", "a.onnx.json", "MODEL_CARD"] {
            routes.insert(
                format!("https://voices.test/files/ku/test/{name}"),
                b"abc".to_vec(),
            );
        }
        InMemoryTransport(routes)
    }

    fn config_in(dir: &std::path::Path) -> Config {
        Config {
            voices_dir: dir.to_path_buf(),
            model_type: "piper".into(),
            catalogue_url: CATALOGUE_URL.into(),
            files_base_url: "https://voices.test/files".into(),
        }
    }

    fn store_with(transport: impl Transport + 'static) -> (Arc<dyn VoiceStore>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let manager = VoiceManager::new(config_in(dir.path()), transport).unwrap();
        (Arc::new(manager), dir)
    }

    #[test]
    fn without_a_config_there_is_no_store() {
        assert!(open_store(None).unwrap().is_none());
    }

    #[tokio::test]
    async fn a_cached_catalogue_is_listed_without_the_network() {
        let (store, dir) = fixture();
        fetch_catalogue(store).await.unwrap();
        let reopened = open_store(Some(config_in(dir.path()))).unwrap().unwrap();
        assert_eq!(list_voices(reopened, None).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_fresh_store_has_no_catalogue_until_one_is_fetched() {
        let dir = tempfile::tempdir().unwrap();
        let store = open_store(Some(config_in(dir.path()))).unwrap().unwrap();
        let error = list_voices(store, None).await.unwrap_err();
        assert_eq!(error.code(), Code::FailedPrecondition);
    }

    pub(crate) fn fixture() -> (Arc<dyn VoiceStore>, tempfile::TempDir) {
        store_with(routes())
    }

    struct GatedTransport {
        inner: InMemoryTransport,
        reached: std::sync::mpsc::SyncSender<()>,
        release: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
        gated: std::sync::atomic::AtomicBool,
    }

    impl Transport for GatedTransport {
        fn get(&self, url: &str, range_from: Option<u64>) -> VoiceResult<Response> {
            let first_file =
                url != CATALOGUE_URL && !self.gated.swap(true, std::sync::atomic::Ordering::SeqCst);
            if first_file {
                let _ = self.reached.try_send(());
                let _ = self.release.lock().unwrap().recv();
            }
            self.inner.get(url, range_from)
        }
    }

    #[tokio::test]
    async fn installing_streams_progress_and_leaves_the_voice_installed() {
        use tokio_stream::StreamExt;
        let (store, _dir) = fixture();
        fetch_catalogue(Arc::clone(&store)).await.unwrap();
        let items: Vec<_> = install_voice(Arc::clone(&store), KEY.to_owned())
            .collect()
            .await;
        assert!(items.iter().all(Result::is_ok), "{items:?}");
        let last = items.last().expect("a progress update").as_ref().unwrap();
        assert_eq!((last.done, last.total), (9, 9));
        let installed = list_installed(store).await.unwrap();
        let keys: Vec<&str> = installed.iter().map(|voice| voice.key.as_str()).collect();
        assert_eq!(keys, [KEY]);
    }

    #[tokio::test]
    async fn installing_an_unknown_voice_ends_the_stream_with_not_found() {
        use tokio_stream::StreamExt;
        let (store, _dir) = fixture();
        fetch_catalogue(Arc::clone(&store)).await.unwrap();
        let items: Vec<_> = install_voice(store, "no-such-voice".to_owned())
            .collect()
            .await;
        assert_eq!(items.len(), 1, "{items:?}");
        assert_eq!(items[0].as_ref().unwrap_err().code(), Code::NotFound);
    }

    #[tokio::test]
    async fn dropping_the_stream_cancels_the_install() {
        let (reached_tx, reached_rx) = std::sync::mpsc::sync_channel(1);
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let (store, _dir) = store_with(GatedTransport {
            inner: routes(),
            reached: reached_tx,
            release: std::sync::Mutex::new(release_rx),
            gated: std::sync::atomic::AtomicBool::new(false),
        });
        fetch_catalogue(Arc::clone(&store)).await.unwrap();
        let stream = install_voice(Arc::clone(&store), KEY.to_owned());
        tokio::task::spawn_blocking(move || reached_rx.recv())
            .await
            .unwrap()
            .unwrap();
        drop(stream);
        release_tx.send(()).unwrap();
        let outcome = loop {
            match remove_voice(Arc::clone(&store), KEY.to_owned()).await {
                Err(status) if status.code() == Code::Aborted => {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                other => break other,
            }
        };
        let status = outcome.expect_err("the voice must not have been installed");
        assert_eq!(status.code(), Code::NotFound);
    }

    #[tokio::test]
    async fn fetching_reports_the_voice_count() {
        let (store, _dir) = fixture();
        assert_eq!(fetch_catalogue(store).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn listing_before_fetching_is_a_failed_precondition() {
        let (store, _dir) = fixture();
        let error = list_voices(store, None).await.unwrap_err();
        assert_eq!(error.code(), Code::FailedPrecondition);
    }

    #[tokio::test]
    async fn listing_filters_by_language_family() {
        let (store, _dir) = fixture();
        fetch_catalogue(Arc::clone(&store)).await.unwrap();
        let sizes = [
            (None, 1),
            (Some("ku"), 1),
            (Some("ku_TR"), 1),
            (Some("de"), 0),
        ];
        for (language, expected) in sizes {
            let found = list_voices(Arc::clone(&store), language.map(str::to_owned))
                .await
                .unwrap();
            assert_eq!(found.len(), expected, "{language:?}");
        }
    }

    #[tokio::test]
    async fn nothing_is_installed_at_first() {
        let (store, _dir) = fixture();
        assert!(list_installed(store).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn removing_a_voice_that_is_not_installed_is_not_found() {
        let (store, _dir) = fixture();
        let error = remove_voice(store, KEY.to_owned()).await.unwrap_err();
        assert_eq!(error.code(), Code::NotFound);
    }

    fn code_of(error: VoiceError) -> tonic::Code {
        voice_status(error).code()
    }

    #[test]
    fn voice_errors_map_to_the_matching_status_codes() {
        use tonic::Code;
        let path = PathBuf::from("/secret/dir/model.onnx");
        let cases = [
            (VoiceError::Busy("k".into()), Code::Aborted),
            (VoiceError::UnknownVoice("k".into()), Code::NotFound),
            (VoiceError::NotInstalled("k".into()), Code::NotFound),
            (VoiceError::InvalidKey("../k".into()), Code::InvalidArgument),
            (
                VoiceError::UnsafePath("../x".into()),
                Code::FailedPrecondition,
            ),
            (VoiceError::NoCatalogue, Code::FailedPrecondition),
            (
                VoiceError::CatalogueParse("bad".into()),
                Code::FailedPrecondition,
            ),
            (VoiceError::Network("down".into()), Code::Unavailable),
            (VoiceError::DigestMismatch(path.clone()), Code::DataLoss),
            (VoiceError::SizeMismatch(path.clone()), Code::DataLoss),
            (VoiceError::Cancelled, Code::Cancelled),
            (
                VoiceError::Io(path, std::io::ErrorKind::NotFound),
                Code::Internal,
            ),
        ];
        for (error, expected) in cases {
            let shown = format!("{error:?}");
            assert_eq!(code_of(error), expected, "{shown}");
        }
    }

    #[test]
    fn client_messages_never_carry_server_paths_or_urls() {
        let path = PathBuf::from("/secret/dir/model.onnx");
        for error in [
            VoiceError::DigestMismatch(path.clone()),
            VoiceError::SizeMismatch(path.clone()),
            VoiceError::Io(path, std::io::ErrorKind::PermissionDenied),
            VoiceError::Network("GET https://internal.example/x failed".into()),
        ] {
            let message = voice_status(error).message().to_owned();
            assert!(!message.contains("/secret"), "{message}");
            assert!(!message.contains("internal.example"), "{message}");
        }
    }

    #[test]
    fn unknown_voice_messages_name_the_key() {
        let message = voice_status(VoiceError::UnknownVoice("ku_TR-x".into()))
            .message()
            .to_owned();
        assert!(message.contains("ku_TR-x"), "{message}");
    }

    #[test]
    fn blank_values_count_as_unset() {
        let blank = [
            (VOICES_DIR_VAR, "  "),
            (CATALOGUE_URL_VAR, ""),
            (FILES_BASE_URL_VAR, "\t"),
        ];
        assert_eq!(voices_config_from_lookup(lookup_from(&blank)), Ok(None));
    }
}
