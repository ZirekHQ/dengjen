use clap::{Args, Subcommand};
use dengjen_voices::{
    Config, InstalledVoice, Progress, UreqTransport, Voice, VoiceError, VoiceManager,
};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const MODEL_TYPE: &str = "piper";
const CATALOGUE_URL: &str = "https://huggingface.co/rhasspy/piper-voices/raw/main/voices.json";
const FILES_BASE_URL: &str = "https://huggingface.co/rhasspy/piper-voices/resolve/main";
const VOICES_DIR_ENV: &str = "DENGJEN_VOICES_DIR";

type Manager = VoiceManager<UreqTransport>;

#[derive(Args)]
pub struct Location {
    /// Voices directory (default: the platform data dir, `dengjen/voices`)
    #[arg(long, env = VOICES_DIR_ENV, global = true)]
    voices_dir: Option<PathBuf>,
    /// Catalogue URL
    #[arg(long, env = "DENGJEN_CATALOGUE_URL", default_value = CATALOGUE_URL, global = true)]
    catalogue_url: String,
    /// Base URL the catalogue's file paths resolve against
    #[arg(long, env = "DENGJEN_FILES_BASE_URL", default_value = FILES_BASE_URL, global = true)]
    files_base_url: String,
}

#[derive(Subcommand)]
pub enum VoicesCommand {
    /// List catalogue voices (default)
    List {
        #[arg(long)]
        lang: Option<String>,
        /// Show only installed voices
        #[arg(long)]
        installed: bool,
        /// Print a JSON array
        #[arg(long)]
        json: bool,
    },
    /// Download and install a voice
    Install { key: String },
    /// Remove an installed voice
    Remove { key: String },
    /// Re-download the catalogue
    Refresh,
}

fn voices_dir_from(
    explicit: Option<PathBuf>,
    env: Option<PathBuf>,
    data_dir: Option<PathBuf>,
) -> Result<PathBuf, String> {
    explicit
        .or(env)
        .or_else(|| data_dir.map(|dir| dir.join("dengjen").join("voices")))
        .ok_or_else(|| "cannot locate a data directory; pass --voices-dir".to_owned())
}

pub fn voices_dir(explicit: Option<PathBuf>) -> Result<PathBuf, String> {
    voices_dir_from(
        explicit,
        std::env::var_os(VOICES_DIR_ENV).map(PathBuf::from),
        dirs::data_dir(),
    )
}

pub fn installed_config_path(voices_dir: &Path, key: &str) -> Option<PathBuf> {
    let dir = voices_dir.join(MODEL_TYPE).join(key);
    let config = dir.join(format!("{key}.onnx.json"));
    (config.is_file() && dir.join(format!("{key}.onnx")).is_file()).then_some(config)
}

pub fn exit_code(error: &VoiceError) -> u8 {
    match error {
        VoiceError::Busy(_) => 2,
        _ => 1,
    }
}

fn manager(location: &Location) -> Result<Manager, String> {
    let voices_dir = voices_dir(location.voices_dir.clone())?;
    std::fs::create_dir_all(&voices_dir)
        .map_err(|error| format!("cannot create {}: {error}", voices_dir.display()))?;
    let config = Config {
        voices_dir,
        model_type: MODEL_TYPE.to_owned(),
        catalogue_url: location.catalogue_url.clone(),
        files_base_url: location.files_base_url.clone(),
    };
    VoiceManager::new(config, UreqTransport::new()).map_err(|error| error.to_string())
}

fn ensure_catalogue(manager: &Manager) -> Result<(), VoiceError> {
    manager
        .load_cached_catalogue()
        .or_else(|error| match error {
            VoiceError::NoCatalogue => manager.fetch_catalogue(),
            other => Err(other),
        })
        .map(|_| ())
}

fn size_label(bytes: u64) -> String {
    match bytes / (1024 * 1024) {
        0 => format!("{} KiB", bytes / 1024),
        mib => format!("{mib} MiB"),
    }
}

fn render_json(voices: &[Voice], installed: &HashSet<String>) -> String {
    let rows = voices.iter().map(|voice| {
        serde_json::json!({
            "key": voice.key,
            "name": voice.name,
            "language": voice.language.code,
            "quality": voice.quality,
            "speakers": voice.num_speakers,
            "bytes": voice.total_bytes(),
            "installed": installed.contains(&voice.key),
        })
    });
    serde_json::Value::Array(rows.collect()).to_string()
}

fn render_table(voices: &[Voice], installed: &HashSet<String>) -> String {
    voices
        .iter()
        .map(|voice| {
            let mark = if installed.contains(&voice.key) {
                "\tinstalled"
            } else {
                ""
            };
            format!(
                "{}\t{}\t{}\t{}{mark}",
                voice.key,
                voice.language.code,
                voice.quality,
                size_label(voice.total_bytes())
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn cancel_flag() -> Arc<AtomicBool> {
    let flag = Arc::new(AtomicBool::new(false));
    let handler = Arc::clone(&flag);
    // The first signal only stops the install between reads; a second exits at once, so a
    // stalled download cannot trap the user.
    let _ = ctrlc::set_handler(move || {
        if handler.swap(true, Ordering::SeqCst) {
            std::process::exit(130);
        }
    });
    flag
}

fn progress_line(progress: Progress, terminal: bool) -> Option<String> {
    let Progress { done, total } = progress;
    let rewind = if terminal { "\r" } else { "" };
    match (terminal, done >= total) {
        (true, false) => Some(format!("{rewind}{done}/{total} bytes")),
        (_, true) => Some(format!("{rewind}{done}/{total} bytes\n")),
        (false, false) => None,
    }
}

fn install(manager: &Manager, key: &str) -> Result<(), VoiceError> {
    ensure_catalogue(manager)?;
    let cancelled = cancel_flag();
    let terminal = std::io::IsTerminal::is_terminal(&std::io::stderr());
    let mut on_progress = |p: Progress| {
        if let Some(line) = progress_line(p, terminal) {
            eprint!("{line}");
        }
    };
    manager.install(key, &mut on_progress, &|| cancelled.load(Ordering::SeqCst))
}

fn print_rows(json: bool, json_text: String, table_text: String) {
    match (json, table_text.is_empty()) {
        (true, _) => println!("{json_text}"),
        (false, false) => println!("{table_text}"),
        (false, true) => {}
    }
}

fn matches_language(code: &str, lang: Option<&str>) -> bool {
    lang.is_none_or(|lang| {
        let lang = lang.replace('-', "_").to_lowercase();
        let code = code.to_lowercase();
        code == lang || code.starts_with(&format!("{lang}_"))
    })
}

fn list_installed(manager: &Manager, lang: Option<&str>, json: bool) -> Result<(), VoiceError> {
    let voices: Vec<InstalledVoice> = manager
        .installed()?
        .into_iter()
        .filter(|voice| matches_language(&voice.language, lang))
        .collect();
    let rows = voices.iter().map(|voice| {
        serde_json::json!({
            "key": voice.key,
            "name": voice.name,
            "language": voice.language,
            "description": voice.description,
        })
    });
    let table = voices
        .iter()
        .map(|voice| format!("{}\t{}\t{}", voice.key, voice.language, voice.description))
        .collect::<Vec<_>>()
        .join("\n");
    print_rows(
        json,
        serde_json::Value::Array(rows.collect()).to_string(),
        table,
    );
    Ok(())
}

fn list(
    manager: &Manager,
    lang: Option<&str>,
    installed_only: bool,
    json: bool,
) -> Result<(), VoiceError> {
    if installed_only {
        return list_installed(manager, lang, json);
    }
    ensure_catalogue(manager)?;
    let voices = manager.list(lang)?;
    let installed: HashSet<String> = manager.installed()?.into_iter().map(|v| v.key).collect();
    print_rows(
        json,
        render_json(&voices, &installed),
        render_table(&voices, &installed),
    );
    Ok(())
}

fn dispatch(manager: &Manager, command: VoicesCommand) -> Result<(), VoiceError> {
    match command {
        VoicesCommand::List {
            lang,
            installed,
            json,
        } => list(manager, lang.as_deref(), installed, json),
        VoicesCommand::Install { key } => install(manager, &key),
        VoicesCommand::Remove { key } => manager.remove(&key),
        VoicesCommand::Refresh => manager
            .fetch_catalogue()
            .map(|count| println!("{count} voices")),
    }
}

fn fail(message: impl std::fmt::Display, code: u8) -> ExitCode {
    eprintln!("error: {message}");
    ExitCode::from(code)
}

pub fn run(location: &Location, command: Option<VoicesCommand>) -> ExitCode {
    let command = command.unwrap_or(VoicesCommand::List {
        lang: None,
        installed: false,
        json: false,
    });
    match manager(location) {
        Err(message) => fail(message, 1),
        Ok(manager) => match dispatch(&manager, command) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => fail(&error, exit_code(&error)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dengjen_voices::VoiceError;

    #[test]
    fn busy_maps_to_exit_code_2_and_everything_else_to_1() {
        assert_eq!(exit_code(&VoiceError::Busy("ku".into())), 2);
        assert_eq!(exit_code(&VoiceError::Cancelled), 1);
        assert_eq!(exit_code(&VoiceError::NoCatalogue), 1);
    }

    #[test]
    fn explicit_voices_dir_wins_over_default() {
        let dir = voices_dir(Some("/x/voices".into())).unwrap();
        assert_eq!(dir, std::path::PathBuf::from("/x/voices"));
    }

    #[test]
    fn installed_config_path_requires_config_and_model() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("piper").join("ku");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ku.onnx.json"), "{}").unwrap();
        assert_eq!(installed_config_path(tmp.path(), "ku"), None);
        std::fs::write(dir.join("ku.onnx"), "").unwrap();
        assert_eq!(
            installed_config_path(tmp.path(), "ku"),
            Some(dir.join("ku.onnx.json"))
        );
    }

    #[test]
    fn voices_dir_resolution_fails_without_any_source() {
        assert!(voices_dir_from(None, None, None).is_err());
        assert_eq!(
            voices_dir_from(None, None, Some("/d".into())).unwrap(),
            std::path::PathBuf::from("/d/dengjen/voices")
        );
        assert_eq!(
            voices_dir_from(None, Some("/e".into()), Some("/d".into())).unwrap(),
            std::path::PathBuf::from("/e")
        );
    }

    #[test]
    fn size_label_uses_the_unit_that_keeps_small_voices_visible() {
        assert_eq!(size_label(400 * 1024), "400 KiB");
        assert_eq!(size_label(63 * 1024 * 1024 + 5), "63 MiB");
    }

    #[test]
    fn progress_lines_overwrite_on_a_terminal_and_print_once_otherwise() {
        let mid = Progress { done: 1, total: 4 };
        let end = Progress { done: 4, total: 4 };
        assert_eq!(progress_line(mid, true).as_deref(), Some("\r1/4 bytes"));
        assert_eq!(progress_line(mid, false), None);
        assert_eq!(progress_line(end, false).as_deref(), Some("4/4 bytes\n"));
        assert_eq!(progress_line(end, true).as_deref(), Some("\r4/4 bytes\n"));
    }

    #[test]
    fn list_json_of_no_voices_is_an_empty_array() {
        assert_eq!(render_json(&[], &HashSet::new()), "[]");
    }
}
