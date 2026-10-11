use crate::{DengjenError, DengjenResult};
use std::path::{Component, Path, PathBuf};

/// Joins `relative` onto `base_dir`, rejecting values that would leave it.
///
/// Absolute paths, drive prefixes and `..` components are refused, so a voice config cannot name
/// a path outside its directory by lexical traversal. Symlinks are not resolved.
pub fn resolve_within(base_dir: &Path, field: &str, relative: &str) -> DengjenResult<PathBuf> {
    let escapes = Path::new(relative).components().any(|c| {
        matches!(
            c,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    });
    if escapes || relative.is_empty() {
        return Err(DengjenError::InvalidConfiguration(format!(
            "`{field}` must be a relative path inside the voice directory, got `{relative}`"
        )));
    }
    Ok(base_dir.join(relative))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_relative_paths_resolve_under_the_base_directory() {
        let resolved = resolve_within(Path::new("/voices/a"), "model_path", "onnx/model.onnx");
        assert_eq!(resolved.unwrap(), Path::new("/voices/a/onnx/model.onnx"));
    }

    #[test]
    fn current_directory_components_are_allowed() {
        assert!(resolve_within(Path::new("/v"), "model_path", "./model.onnx").is_ok());
    }

    #[test]
    fn parent_directory_components_are_rejected() {
        for value in ["../model.onnx", "a/../../model.onnx", ".."] {
            let result = resolve_within(Path::new("/v"), "model_path", value);
            assert!(
                matches!(result, Err(DengjenError::InvalidConfiguration(_))),
                "{value}"
            );
        }
    }

    #[test]
    fn absolute_and_empty_paths_are_rejected() {
        for value in ["/etc/passwd", ""] {
            let result = resolve_within(Path::new("/v"), "model_path", value);
            assert!(
                matches!(result, Err(DengjenError::InvalidConfiguration(_))),
                "{value}"
            );
        }
    }
}
