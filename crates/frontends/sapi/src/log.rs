//! Opt-in diagnostics for a voice running inside someone else's process.
//!
//! A screen reader loads this DLL, so there is no console to print to. When the environment
//! variable `DENGJEN_SAPI_LOG` names a file, each call below appends one line to it; without the
//! variable nothing is read, written or allocated.
#![forbid(unsafe_code)]

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

pub const LOG_VARIABLE: &str = "DENGJEN_SAPI_LOG";

fn target() -> Option<&'static Path> {
    static TARGET: OnceLock<Option<PathBuf>> = OnceLock::new();
    TARGET
        .get_or_init(|| std::env::var_os(LOG_VARIABLE).map(PathBuf::from))
        .as_deref()
}

fn append(path: &Path, message: &str) {
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis());
    // Diagnostics must never fail a speak request, so a full disk is ignored.
    let _ = writeln!(file, "{millis} pid={} {message}", std::process::id());
}

/// Appends one line when logging is on. The message is only built then.
pub fn line(message: impl FnOnce() -> String) {
    if let Some(path) = target() {
        append(path, &message());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_appended_with_a_timestamp_and_the_process_id() {
        let path =
            std::env::temp_dir().join(format!("dengjen-sapi-log-{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&path);
        append(&path, "first");
        append(&path, "second");
        let text = std::fs::read_to_string(&path).expect("read log");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with(&format!("pid={} first", std::process::id())));
        assert!(lines[1].ends_with("second"));
        std::fs::remove_file(path).expect("clean up");
    }

    #[test]
    fn an_unwritable_path_is_ignored() {
        append(Path::new("/nonexistent-dir/dengjen.log"), "dropped");
    }
}
