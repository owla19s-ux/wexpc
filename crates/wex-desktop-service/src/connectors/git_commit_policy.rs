//! Pure validation for an eventual exact-path local Git commit operation.
//! No Git command or filesystem operation is performed here. Host must still
//! recheck repository identity, revision, permissions and signed intent.
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitPreflightError {
    InvalidMessage,
    InvalidPath,
    DuplicatePath,
    TooManyPaths,
}

/// Deliberately reject Git magic pathspecs and ambiguous separators. These
/// are repository-relative file paths, not arbitrary Git path expressions.
pub fn validate_commit_request(
    message: &str,
    paths: &[String],
) -> Result<(), CommitPreflightError> {
    if message.trim().is_empty()
        || message.len() > 4096
        || message.chars().any(|ch| ch == '\0' || ch == '\r')
    {
        return Err(CommitPreflightError::InvalidMessage);
    }
    if paths.is_empty() || paths.len() > 128 {
        return Err(CommitPreflightError::TooManyPaths);
    }
    let mut seen = BTreeSet::new();
    for path in paths {
        if path.is_empty()
            || path.len() > 1024
            || path.starts_with('/')
            || path.starts_with(':')
            || path.contains('\\')
            || path.contains("//")
            || path.chars().any(char::is_control)
            || path.split('/').any(|s| {
                s.is_empty() || s == "." || s == ".." || s == ".git"
            })
        {
            return Err(CommitPreflightError::InvalidPath);
        }
        if !seen.insert(path) {
            return Err(CommitPreflightError::DuplicatePath);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_file_paths_and_message_are_accepted() {
        assert_eq!(validate_commit_request(
            "Implement feature",
            &["src/main.rs".into(), "tests/check.rs".into()]
        ), Ok(()));
    }

    #[test]
    fn unsafe_and_ambiguous_pathspecs_are_rejected() {
        for path in [
            "", "../outside", "src/../other", "src/./file", "/etc/passwd",
            "src//file", "src\\file", "src/.git/config", ".git/config",
            ":(top)src/main.rs", ":!secret", "src/", "src\nfile",
        ] {
            assert_eq!(
                validate_commit_request("message", &[path.into()]),
                Err(CommitPreflightError::InvalidPath),
                "{path:?}"
            );
        }
    }

    #[test]
    fn message_and_path_count_have_explicit_bounds() {
        assert_eq!(validate_commit_request("  ", &["x".into()]), Err(CommitPreflightError::InvalidMessage));
        assert_eq!(validate_commit_request("bad\0message", &["x".into()]), Err(CommitPreflightError::InvalidMessage));
        assert_eq!(validate_commit_request("ok", &[]), Err(CommitPreflightError::TooManyPaths));
        assert_eq!(
            validate_commit_request("ok", &vec!["x".into(); 129]),
            Err(CommitPreflightError::TooManyPaths)
        );
        assert_eq!(
            validate_commit_request("ok", &["src/a.rs".into(), "src/a.rs".into()]),
            Err(CommitPreflightError::DuplicatePath)
        );
    }
}
