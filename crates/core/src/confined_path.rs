use std::fmt;
use std::path::{Component, Path, PathBuf};

/// A relative path that would leave the directory it is joined onto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathEscapesBase {
    relative: String,
}

impl PathEscapesBase {
    /// The rejected path, exactly as supplied.
    pub fn relative(&self) -> &str {
        &self.relative
    }
}

impl fmt::Display for PathEscapesBase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "`{}` must be a relative path inside its base directory",
            self.relative
        )
    }
}

impl std::error::Error for PathEscapesBase {}

/// Joins `relative` onto `base_dir`, rejecting values that would leave it.
///
/// Absolute paths, drive prefixes and `..` components are refused, so a config cannot name a
/// path outside its directory by lexical traversal. Symlinks are not resolved.
pub fn resolve_within(base_dir: &Path, relative: &str) -> Result<PathBuf, PathEscapesBase> {
    let escapes = Path::new(relative).components().any(|c| {
        matches!(
            c,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    });
    if escapes || relative.is_empty() {
        return Err(PathEscapesBase {
            relative: relative.to_owned(),
        });
    }
    Ok(base_dir.join(relative))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_relative_paths_resolve_under_the_base_directory() {
        let resolved = resolve_within(Path::new("/voices/a"), "onnx/model.onnx");
        assert_eq!(resolved.unwrap(), Path::new("/voices/a/onnx/model.onnx"));
    }

    #[test]
    fn current_directory_components_are_allowed() {
        assert!(resolve_within(Path::new("/v"), "./model.onnx").is_ok());
    }

    #[test]
    fn parent_directory_components_are_rejected() {
        for value in ["../model.onnx", "a/../../model.onnx", ".."] {
            let error = resolve_within(Path::new("/v"), value).unwrap_err();
            assert_eq!(error.relative(), value);
        }
    }

    #[test]
    fn absolute_and_empty_paths_are_rejected() {
        for value in ["/etc/passwd", ""] {
            assert!(resolve_within(Path::new("/v"), value).is_err(), "{value}");
        }
    }
}
