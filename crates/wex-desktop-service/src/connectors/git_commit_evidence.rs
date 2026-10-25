//! Pure commit evidence checks. Inputs must come from trusted Host reads,
//! never tool arguments or model output. No command, lock or approval is issued.
use super::git_commit_policy::{validate_commit_request, CommitPreflightError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryVersion {
    /// Host identity of the opened worktree AND Git directory, not just a path.
    pub identity: String,
    /// None represents an unborn branch. Detached HEAD is not admitted yet.
    pub head: Option<String>,
    pub branch: String,
    /// SHA-256 of exact index bytes, or a Host-defined absent-index sentinel.
    pub index_sha256: String,
    /// Host digest of exact selected leaf contents, modes and deletions.
    pub selected_sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitEvidenceError {
    Request(CommitPreflightError),
    InvalidVersion,
    VersionChanged,
    InvalidCommit,
    ParentMismatch,
    TreeMismatch,
    MessageMismatch,
    PathsMismatch,
    HeadMismatch,
}

/// Immutable reviewed input. This is NOT an authority token or an executor.
#[derive(Debug, PartialEq, Eq)]
pub struct PreparedCommit {
    message: String,
    paths: Vec<String>,
    version: RepositoryVersion,
}

/// Host-read commit object plus the current repository observation.
#[derive(Debug, Clone)]
pub struct CommitReadback {
    pub oid: String,
    pub parents: Vec<String>,
    pub tree: String,
    pub message: String,
    /// Full changed paths against the parent, including both rename sides.
    pub changed_paths: Vec<String>,
    pub repository_identity: String,
    pub branch: String,
    pub head: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct CommitObject {
    pub tree: String,
    pub parents: Vec<String>,
    pub message: String,
}

/// Parse bounded `git cat-file commit <oid>` bytes, preserving exact message.
/// The Host must obtain oid and raw bytes using its trusted Git runner.
pub fn parse_commit_object(raw: &[u8]) -> Result<CommitObject, CommitEvidenceError> {
    if raw.len() > 64 * 1024 || raw.contains(&0) {
        return Err(CommitEvidenceError::InvalidCommit);
    }
    let text = std::str::from_utf8(raw).map_err(|_| CommitEvidenceError::InvalidCommit)?;
    let (header, message) = text
        .split_once("\n\n")
        .ok_or(CommitEvidenceError::InvalidCommit)?;
    let mut tree = None;
    let mut parents = Vec::new();
    let mut author = false;
    let mut committer = false;
    let mut previous_header = false;
    for (index, line) in header.split('\n').enumerate() {
        if line.chars().any(|c| c.is_control() && c != '\t') {
            return Err(CommitEvidenceError::InvalidCommit);
        }
        if line.starts_with(' ') && previous_header {
            continue;
        }
        let (name, value) = line
            .split_once(' ')
            .ok_or(CommitEvidenceError::InvalidCommit)?;
        if index == 0 && name != "tree" {
            return Err(CommitEvidenceError::InvalidCommit);
        }
        previous_header = true;
        match name {
            "tree" if tree.is_none() && is_hex(value, &[40, 64]) => tree = Some(value.to_owned()),
            "tree" => return Err(CommitEvidenceError::InvalidCommit),
            "parent" if is_hex(value, &[40, 64]) && parents.len() < 16 => {
                parents.push(value.to_owned())
            }
            "parent" => return Err(CommitEvidenceError::InvalidCommit),
            "author" if !author && !value.is_empty() => author = true,
            "committer" if !committer && !value.is_empty() => committer = true,
            "author" | "committer" => return Err(CommitEvidenceError::InvalidCommit),
            "encoding" if !value.eq_ignore_ascii_case("UTF-8") => {
                return Err(CommitEvidenceError::InvalidCommit)
            }
            _ => {}
        }
    }
    let tree = tree.ok_or(CommitEvidenceError::InvalidCommit)?;
    if !author || !committer {
        return Err(CommitEvidenceError::InvalidCommit);
    }
    if parents.iter().any(|p| p.len() != tree.len()) {
        return Err(CommitEvidenceError::InvalidCommit);
    }
    Ok(CommitObject {
        tree,
        parents,
        message: message.into(),
    })
}

fn is_hex(value: &str, lengths: &[usize]) -> bool {
    lengths.contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl PreparedCommit {
    /// The parent version that the Core operation must carry. None means an
    /// unborn branch, whose branch name is still included in binding_parts.
    pub fn expected_head(&self) -> Option<&str> {
        self.version.head.as_deref()
    }

    pub fn paths(&self) -> &[String] {
        &self.paths
    }

    pub fn new(
        message: String,
        paths: Vec<String>,
        version: RepositoryVersion,
    ) -> Result<Self, CommitEvidenceError> {
        validate_commit_request(&message, &paths).map_err(CommitEvidenceError::Request)?;
        if version.identity.is_empty()
            || version.identity.len() > 1024
            || version.identity.chars().any(char::is_control)
            || !version.branch.starts_with("refs/heads/")
            || version.branch.len() <= "refs/heads/".len()
            || version.branch.len() > 1024
            || version.branch.chars().any(char::is_control)
            || version.head.as_ref().is_some_and(|h| !is_hex(h, &[40, 64]))
            || !is_hex(&version.index_sha256, &[64])
            || !is_hex(&version.selected_sha256, &[64])
        {
            return Err(CommitEvidenceError::InvalidVersion);
        }
        Ok(Self {
            message,
            paths,
            version,
        })
    }

    /// All parts are length-delimited by the existing digest_arguments helper.
    /// The caller must bind capability git_commit and the Host resource ID too.
    pub fn binding_parts(&self) -> Vec<&[u8]> {
        let mut parts = vec![
            b"WEXPC_GIT_COMMIT_V1".as_slice(),
            self.version.identity.as_bytes(),
            self.version.branch.as_bytes(),
            self.version.head.as_deref().unwrap_or("").as_bytes(),
            self.version.index_sha256.as_bytes(),
            self.version.selected_sha256.as_bytes(),
            self.message.as_bytes(),
        ];
        parts.extend(self.paths.iter().map(|p| p.as_bytes()));
        parts
    }

    /// Call under the same resource ownership as the eventual write. A check
    /// without a lock is not TOCTOU protection and must not enable execution.
    pub fn recheck(&self, current: &RepositoryVersion) -> Result<(), CommitEvidenceError> {
        if current != &self.version {
            return Err(CommitEvidenceError::VersionChanged);
        }
        Ok(())
    }

    /// expected_tree is independently read from the isolated, exact-path index
    /// before creating the commit; NEVER copy it from this readback to compare.
    /// An error after dispatch means unverified/UNKNOWN, not a safe retry.
    pub fn verify_readback(
        &self,
        expected_tree: &str,
        actual: &CommitReadback,
    ) -> Result<(), CommitEvidenceError> {
        if !is_hex(&actual.oid, &[40, 64])
            || !is_hex(expected_tree, &[actual.oid.len()])
            || !is_hex(&actual.tree, &[actual.oid.len()])
            || actual
                .parents
                .iter()
                .any(|p| !is_hex(p, &[actual.oid.len()]))
            || self
                .version
                .head
                .as_ref()
                .is_some_and(|h| h.len() != actual.oid.len())
        {
            return Err(CommitEvidenceError::InvalidCommit);
        }
        let expected_parents: Vec<_> = self.version.head.iter().cloned().collect();
        if actual.parents != expected_parents {
            return Err(CommitEvidenceError::ParentMismatch);
        }
        if actual.tree != expected_tree {
            return Err(CommitEvidenceError::TreeMismatch);
        }
        if actual.message != self.message {
            return Err(CommitEvidenceError::MessageMismatch);
        }
        // Approved paths may include unchanged files. Every actual change must
        // be within scope, and the result must contain at least one change.
        validate_commit_request(&actual.message, &actual.changed_paths)
            .map_err(|_| CommitEvidenceError::PathsMismatch)?;
        if actual.changed_paths.iter().any(|p| !self.paths.contains(p)) {
            return Err(CommitEvidenceError::PathsMismatch);
        }
        if actual.repository_identity != self.version.identity
            || actual.branch != self.version.branch
            || actual.head != actual.oid
            || self.version.head.as_ref() == Some(&actual.oid)
        {
            return Err(CommitEvidenceError::HeadMismatch);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn version() -> RepositoryVersion {
        RepositoryVersion {
            identity: "opened-worktree-and-gitdir".into(),
            head: Some("a".repeat(40)),
            branch: "refs/heads/work".into(),
            index_sha256: "b".repeat(64),
            selected_sha256: "c".repeat(64),
        }
    }
    fn prepared() -> PreparedCommit {
        PreparedCommit::new(
            "Exact message\n".into(),
            vec!["src/a".into(), "src/b".into()],
            version(),
        )
        .unwrap()
    }
    fn readback() -> CommitReadback {
        CommitReadback {
            oid: "d".repeat(40),
            parents: vec!["a".repeat(40)],
            tree: "e".repeat(40),
            message: "Exact message\n".into(),
            changed_paths: vec!["src/a".into()],
            repository_identity: version().identity,
            branch: version().branch,
            head: "d".repeat(40),
        }
    }
    #[test]
    fn unchanged_version_and_exact_readback_pass() {
        let p = prepared();
        assert_eq!(p.recheck(&version()), Ok(()));
        assert_eq!(p.verify_readback(&"e".repeat(40), &readback()), Ok(()));
    }
    #[test]
    fn each_repository_version_change_is_rejected() {
        let base = version();
        for field in 0..5 {
            let mut v = base.clone();
            match field {
                0 => v.identity.push('x'),
                1 => v.head = None,
                2 => v.branch.push('x'),
                3 => v.index_sha256 = "0".repeat(64),
                _ => v.selected_sha256 = "0".repeat(64),
            }
            assert_eq!(
                prepared().recheck(&v),
                Err(CommitEvidenceError::VersionChanged)
            );
        }
    }
    #[test]
    fn invalid_versions_cannot_be_prepared() {
        for field in 0..6 {
            let mut v = version();
            match field {
                0 => v.identity.clear(),
                1 => v.head = Some("invalid".into()),
                2 => v.branch = "HEAD".into(),
                3 => v.index_sha256.clear(),
                4 => v.selected_sha256 = "C".repeat(64),
                _ => v.branch = "refs/heads/".into(),
            }
            assert_eq!(
                PreparedCommit::new("m".into(), vec!["x".into()], v),
                Err(CommitEvidenceError::InvalidVersion)
            );
        }
        assert!(matches!(
            PreparedCommit::new("m".into(), vec!["../x".into()], version()),
            Err(CommitEvidenceError::Request(_))
        ));
    }
    #[test]
    fn binding_includes_every_reviewed_input() {
        let p = prepared();
        let parts = p.binding_parts();
        assert_eq!(
            parts,
            vec![
                b"WEXPC_GIT_COMMIT_V1".as_slice(),
                version().identity.as_bytes(),
                version().branch.as_bytes(),
                "a".repeat(40).as_bytes(),
                "b".repeat(64).as_bytes(),
                "c".repeat(64).as_bytes(),
                b"Exact message\n",
                b"src/a",
                b"src/b"
            ]
        );
    }
    #[test]
    fn readback_rejects_wrong_parent_or_merge() {
        for parents in [
            vec![],
            vec!["0".repeat(40)],
            vec!["a".repeat(40), "b".repeat(40)],
        ] {
            let mut actual = readback();
            actual.parents = parents;
            assert_eq!(
                prepared().verify_readback(&"e".repeat(40), &actual),
                Err(CommitEvidenceError::ParentMismatch)
            );
        }
    }
    #[test]
    fn readback_rejects_tree_and_message_changes() {
        assert_eq!(
            prepared().verify_readback(&"0".repeat(40), &readback()),
            Err(CommitEvidenceError::TreeMismatch)
        );
        let mut actual = readback();
        actual.message = "Exact message".into();
        assert_eq!(
            prepared().verify_readback(&"e".repeat(40), &actual),
            Err(CommitEvidenceError::MessageMismatch)
        );
    }
    #[test]
    fn readback_requires_nonempty_unique_in_scope_changes() {
        for paths in [
            vec![],
            vec!["src/secret".into()],
            vec!["src/a".into(), "src/a".into()],
            vec!["src/A".into()],
        ] {
            let mut actual = readback();
            actual.changed_paths = paths;
            assert_eq!(
                prepared().verify_readback(&"e".repeat(40), &actual),
                Err(CommitEvidenceError::PathsMismatch)
            );
        }
    }
    #[test]
    fn readback_requires_current_head_in_same_repository_and_branch() {
        for field in 0..3 {
            let mut actual = readback();
            match field {
                0 => actual.repository_identity.push('x'),
                1 => actual.branch.push('x'),
                _ => actual.head = "0".repeat(40),
            }
            assert_eq!(
                prepared().verify_readback(&"e".repeat(40), &actual),
                Err(CommitEvidenceError::HeadMismatch)
            );
        }
    }
    #[test]
    fn supports_unborn_branch_and_sha256_object_format() {
        for width in [40, 64] {
            let mut v = version();
            v.head = None;
            let p = PreparedCommit::new("Exact message\n".into(), vec!["src/a".into()], v).unwrap();
            let mut actual = readback();
            actual.parents.clear();
            actual.oid = "d".repeat(width);
            actual.head = actual.oid.clone();
            actual.tree = "e".repeat(width);
            assert_eq!(p.verify_readback(&"e".repeat(width), &actual), Ok(()));
        }
    }
    #[test]
    fn malformed_and_mixed_object_formats_fail() {
        for field in 0..4 {
            let mut actual = readback();
            match field {
                0 => actual.oid = "bad".into(),
                1 => actual.tree = "e".repeat(64),
                2 => actual.parents[0] = "a".repeat(64),
                _ => actual.oid = "D".repeat(40),
            }
            assert_eq!(
                prepared().verify_readback(&"e".repeat(40), &actual),
                Err(CommitEvidenceError::InvalidCommit)
            );
        }
    }

    #[test]
    fn parses_commit_object_without_trimming_message() {
        let raw = format!("tree {}\nparent {}\nauthor A <a@test> 1 +0000\ncommitter A <a@test> 1 +0000\ngpgsig signature\n continuation\n\nSubject\n\nBody\n", "e".repeat(40), "a".repeat(40));
        assert_eq!(
            parse_commit_object(raw.as_bytes()),
            Ok(CommitObject {
                tree: "e".repeat(40),
                parents: vec!["a".repeat(40)],
                message: "Subject\n\nBody\n".into()
            })
        );
    }

    #[test]
    fn malformed_objects_are_not_success_evidence() {
        for raw in [
            "".to_string(),
            format!("tree {}\nmessage", "e".repeat(40)),
            format!("tree {}\ntree {}\n\nx", "e".repeat(40), "e".repeat(40)),
            format!("tree {}\nparent {}\n\nx", "e".repeat(40), "a".repeat(64)),
            format!("tree {}\nencoding ISO-8859-1\n\nx", "e".repeat(40)),
            "x".repeat(65537),
        ] {
            assert_eq!(
                parse_commit_object(raw.as_bytes()),
                Err(CommitEvidenceError::InvalidCommit)
            );
        }
        assert_eq!(
            parse_commit_object(b"\xff"),
            Err(CommitEvidenceError::InvalidCommit)
        );
        assert_eq!(
            parse_commit_object(b"tree\0"),
            Err(CommitEvidenceError::InvalidCommit)
        );
        assert_eq!(
            parse_commit_object(format!("tree {}\n\nmessage", "e".repeat(40)).as_bytes()),
            Err(CommitEvidenceError::InvalidCommit)
        );
    }

    #[test]
    fn verifies_real_git_commit_and_detects_unreviewed_file() {
        use std::{
            fs,
            process::Command,
            time::{SystemTime, UNIX_EPOCH},
        };
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let root = std::env::temp_dir().join(format!(
            "wex-commit-evidence-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let _cleanup = Cleanup(root.clone());
        let git = |args: &[&str]| {
            let mut cmd = Command::new("git");
            cmd.env_clear()
                .env("PATH", std::env::var_os("PATH").unwrap_or_default())
                .env("HOME", &root)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env(
                    "GIT_CONFIG_GLOBAL",
                    if cfg!(windows) { "NUL" } else { "/dev/null" },
                );
            if let Some(system_root) = std::env::var_os("SystemRoot") {
                cmd.env("SystemRoot", system_root);
            }
            let out = cmd
                .current_dir(&root)
                .args([
                    "-c",
                    "user.name=WEX Test",
                    "-c",
                    "user.email=wex@example.invalid",
                    "-c",
                    "core.hooksPath=",
                    "-c",
                    "commit.gpgSign=false",
                ])
                .args(args)
                .output()
                .unwrap();
            assert!(out.status.success(), "Git failed: {:?}", out.stderr);
            out.stdout
        };
        git(&["init", "--quiet", "-b", "work"]);
        fs::write(root.join("a"), "first").unwrap();
        git(&["add", "--", "a"]);
        let expected_tree = String::from_utf8(git(&["write-tree"]))
            .unwrap()
            .trim()
            .to_string();
        let message = "Subject\n\nExact body\n";
        fs::write(root.join("message"), message).unwrap();
        let mut v = version();
        v.head = None;
        let p = PreparedCommit::new(message.into(), vec!["a".into()], v.clone()).unwrap();
        git(&["commit", "--quiet", "--cleanup=verbatim", "-F", "message"]);
        let oid = String::from_utf8(git(&["rev-parse", "HEAD"]))
            .unwrap()
            .trim()
            .to_string();
        let object = parse_commit_object(&git(&["cat-file", "commit", &oid])).unwrap();
        let changed_paths = git(&[
            "diff-tree",
            "--root",
            "--no-commit-id",
            "--name-only",
            "-r",
            "-z",
            &oid,
        ])
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| String::from_utf8(p.to_vec()).unwrap())
        .collect();
        let actual = CommitReadback {
            oid: oid.clone(),
            head: oid,
            parents: object.parents,
            tree: object.tree,
            message: object.message,
            changed_paths,
            repository_identity: v.identity.clone(),
            branch: v.branch.clone(),
        };
        assert_eq!(p.verify_readback(&expected_tree, &actual), Ok(()));
        let other = PreparedCommit::new(message.into(), vec!["other".into()], v).unwrap();
        assert_eq!(
            other.verify_readback(&expected_tree, &actual),
            Err(CommitEvidenceError::PathsMismatch)
        );
    }
}
