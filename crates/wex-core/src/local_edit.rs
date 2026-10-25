//! Inert, platform-neutral existing-file edit intent contract.
//! No path access, filesystem mutation, or authorization is performed here.
//! Execution must independently bind the target to a pinned directory handle,
//! re-check live grants, and compare the exact file version at commit.
pub const MAX_TEXT_BYTES: usize = 64 * 1024;
pub const MAX_LEAF_BYTES: usize = 255;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditRejection {
    InvalidDirectory,
    InvalidLeaf,
    MissingVersion,
    InvalidVersion,
    ContentTooLarge,
    VersionConflict,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditIntent<'a> {
    /// Opaque ID issued only by trusted native directory selection.
    pub directory_id: &'a str,
    /// One direct leaf, never an absolute or relative multi-component path.
    pub file_name: &'a str,
    /// Current version returned by an authorized read, not model-assumed.
    pub expected_sha256: &'a str,
    /// Complete replacement content. This is not a patch or shell command.
    pub replacement: &'a str,
}

impl EditIntent<'_> {
    pub fn validate(&self) -> Result<(), EditRejection> {
        if self.directory_id.is_empty()
            || self.directory_id.len() > 64
            || !self
                .directory_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(EditRejection::InvalidDirectory);
        }
        if !valid_leaf(self.file_name) {
            return Err(EditRejection::InvalidLeaf);
        }
        if self.expected_sha256.is_empty() {
            return Err(EditRejection::MissingVersion);
        }
        if self.expected_sha256.len() != 64
            || !self.expected_sha256.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(EditRejection::InvalidVersion);
        }
        if self.replacement.len() > MAX_TEXT_BYTES {
            return Err(EditRejection::ContentTooLarge);
        }
        Ok(())
    }

    /// Strict compare-and-swap predicate; never overwrite from a stale read.
    pub fn matches_version(&self, actual_sha256: &str) -> Result<(), EditRejection> {
        self.validate()?;
        if !self.expected_sha256.eq_ignore_ascii_case(actual_sha256) {
            return Err(EditRejection::VersionConflict);
        }
        Ok(())
    }
}

/// Conservative cross-platform grammar. Platform backends must still apply
/// stricter Windows device-name / reparse / short-name rules.
pub fn valid_leaf(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_LEAF_BYTES
        && s != "."
        && s != ".."
        && !s.ends_with('.')
        && !s.ends_with(' ')
        && !s.chars().any(|c| {
            c.is_control() || matches!(c, '/' | '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*')
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request<'a>(sha: &'a str, replacement: &'a str) -> EditIntent<'a> {
        EditIntent {
            directory_id: "saved_1",
            file_name: "main.rs",
            expected_sha256: sha,
            replacement,
        }
    }
    #[test]
    fn requires_exact_version_before_dispatch() {
        let sha = "a".repeat(64);
        let edit = request(&sha, "updated");
        assert_eq!(edit.validate(), Ok(()));
        assert_eq!(edit.matches_version(&sha), Ok(()));
        assert_eq!(
            edit.matches_version(&"b".repeat(64)),
            Err(EditRejection::VersionConflict)
        );
        assert_eq!(
            request("", "x").validate(),
            Err(EditRejection::MissingVersion)
        );
        assert_eq!(
            request("not-hash", "x").validate(),
            Err(EditRejection::InvalidVersion)
        );
    }
    #[test]
    fn cannot_promote_paths_or_unbounded_content() {
        let sha = "a".repeat(64);
        for leaf in [
            "",
            ".",
            "..",
            "../config",
            "sub/file",
            "file:stream",
            "file ",
            "file.",
            "a\nline",
        ] {
            assert_eq!(
                EditIntent {
                    file_name: leaf,
                    ..request(&sha, "x")
                }
                .validate(),
                Err(EditRejection::InvalidLeaf)
            );
        }
        assert_eq!(
            request(&sha, &"x".repeat(MAX_TEXT_BYTES + 1)).validate(),
            Err(EditRejection::ContentTooLarge)
        );
        assert_eq!(
            EditIntent {
                directory_id: "../other",
                ..request(&sha, "x")
            }
            .validate(),
            Err(EditRejection::InvalidDirectory)
        );
    }
}
