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
        || message
            .chars()
            .any(|ch| ch.is_control() && ch != '\n' && ch != '\t')
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
            || path
                .chars()
                .any(|ch| matches!(ch, ':' | '*' | '?' | '[' | ']' | '<' | '>' | '"' | '|'))
            || path.contains('\\')
            || path.contains("//")
            || path.chars().any(char::is_control)
            || path.split('/').any(|s| {
                s.is_empty()
                    || s == "."
                    || s == ".."
                    || s.eq_ignore_ascii_case(".git")
                    || s.ends_with('.')
                    || s.ends_with(' ')
                    || windows_device_name(s)
            })
        {
            return Err(CommitPreflightError::InvalidPath);
        }
        if !seen.insert(path.to_lowercase()) {
            return Err(CommitPreflightError::DuplicatePath);
        }
    }
    Ok(())
}

// Windows resolves these basenames even with extensions. Reject on all hosts
// so a Linux-produced plan cannot gain a different scope on Windows.
fn windows_device_name(component: &str) -> bool {
    let stem = component
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ["COM", "LPT"].iter().any(|prefix| {
        stem.strip_prefix(prefix).is_some_and(|suffix| {
            matches!(
                suffix,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_windows_aliases_and_git_globs() {
        for path in [
            "C:secret",
            "C:/secret",
            "src/.GIT/config",
            "src/.git./config",
            "src/.git /config",
            "src/a*",
            "src/a?",
            "src/[ab]",
            "src/file:stream",
            "src/file.",
            "src/file ",
            "src/CON.txt",
            "src/NUL",
            "aux/file",
            "src/com1.log",
            "src/LPT9",
            "src/COM¹",
        ] {
            assert_eq!(
                validate_commit_request("message", &[path.into()]),
                Err(CommitPreflightError::InvalidPath),
                "{path:?}"
            );
        }
        assert_eq!(
            validate_commit_request("message", &["Src/a.rs".into(), "src/A.rs".into()]),
            Err(CommitPreflightError::DuplicatePath)
        );
    }

    #[test]
    fn rejects_controls_in_message_but_allows_multiline_body() {
        for message in ["bad\u{1b}message", "bad\u{7f}message", "bad\u{85}message"] {
            assert_eq!(
                validate_commit_request(message, &["src/a".into()]),
                Err(CommitPreflightError::InvalidMessage)
            );
        }
        assert_eq!(
            validate_commit_request("Subject\n\nBody\twith tab", &["src/见月.rs".into()]),
            Ok(())
        );
    }

    #[test]
    fn exact_file_paths_and_message_are_accepted() {
        assert_eq!(
            validate_commit_request(
                "Implement feature",
                &["src/main.rs".into(), "tests/check.rs".into()]
            ),
            Ok(())
        );
    }

    #[test]
    fn unsafe_and_ambiguous_pathspecs_are_rejected() {
        for path in [
            "",
            "../outside",
            "src/../other",
            "src/./file",
            "/etc/passwd",
            "src//file",
            "src\\file",
            "src/.git/config",
            ".git/config",
            ":(top)src/main.rs",
            ":!secret",
            "src/",
            "src\nfile",
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
        assert_eq!(
            validate_commit_request("  ", &["x".into()]),
            Err(CommitPreflightError::InvalidMessage)
        );
        assert_eq!(
            validate_commit_request("bad\0message", &["x".into()]),
            Err(CommitPreflightError::InvalidMessage)
        );
        assert_eq!(
            validate_commit_request("ok", &[]),
            Err(CommitPreflightError::TooManyPaths)
        );
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
