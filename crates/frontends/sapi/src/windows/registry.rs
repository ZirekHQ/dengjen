//! Writes and removes the registration that [`crate::registration`] describes.

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, E_FAIL};
use windows_registry::{Key, CURRENT_USER, LOCAL_MACHINE};

use crate::registration::{self, InstallOptions, Scope};

fn hive(scope: Scope) -> &'static Key {
    match scope {
        Scope::Machine => LOCAL_MACHINE,
        Scope::User => CURRENT_USER,
    }
}

fn write(root: &Key, values: &[registration::Value]) -> windows_core::Result<()> {
    values.iter().try_for_each(|value| {
        root.create(&value.key)?
            .set_string(value.name.as_deref().unwrap_or(""), &value.data)
    })
}

pub(super) fn register(options: &InstallOptions) -> windows_core::Result<()> {
    let dll = super::dll_path()?;
    let voices_dir = match &options.voices_dir {
        // The path is stored in each voice token and read back inside the host process, whose
        // working directory is not regsvr32's, so a relative `voices=` must be made absolute now.
        Some(dir) => std::path::absolute(dir).map_err(|error| {
            windows_core::Error::new(
                E_FAIL,
                format!("cannot resolve voices directory {}: {error}", dir.display()),
            )
        })?,
        None => super::dll_dir()?.join("voices"),
    };
    let voices = registration::discover(&voices_dir);
    if voices.is_empty() {
        return Err(windows_core::Error::new(
            E_FAIL,
            format!(
                "no voices in {}; put voices there as piper/<key>/<key>.onnx.json first",
                voices_dir.display()
            ),
        ));
    }
    let root = hive(options.scope);
    // Start from a clean slate so voices removed from disk disappear from the voice list too.
    unregister(options.scope)?;
    write(root, &registration::class_values(&dll))?;
    voices
        .iter()
        .try_for_each(|voice| write(root, &registration::token_values(voice, options.kurdish)))
}

pub(super) fn unregister(scope: Scope) -> windows_core::Result<()> {
    let root = hive(scope);
    if let Ok(tokens) = root.options().read().write().open(registration::TOKENS_KEY) {
        let ours: Vec<String> = tokens
            .keys()?
            .filter(|name| name.starts_with(registration::TOKEN_PREFIX))
            .collect();
        ours.iter().try_for_each(|name| tokens.remove_tree(name))?;
    }
    match root.remove_tree(registration::CLASS_KEY) {
        Err(error) if error.code() == ERROR_FILE_NOT_FOUND.to_hresult() => Ok(()),
        other => other,
    }
}
