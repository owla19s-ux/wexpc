//! Authenticated tree evidence for a trusted post-dispatch Git read.
use super::{
    git_commit_binding::verify_git_commit_approval,
    git_commit_evidence::{parse_commit_object, CommitObject, PreparedCommit},
    git_commit_policy::validate_commit_request,
    git_commit_reader::read_authenticated_object,
};
use crate::{Result, ServiceError};
use std::collections::BTreeMap;
use std::{
    path::Path,
    time::{Duration, Instant},
};
use wex_core::operation::Dispatch;

/// Object-authenticated commit and complete changed regular-file paths.
/// Does not prove the current repository identity/HEAD or the expected tree.
#[derive(Debug)]
pub struct GitCommitChanges {
    oid: String,
    object: CommitObject,
    paths: Vec<String>,
}
impl GitCommitChanges {
    pub fn oid(&self) -> &str {
        &self.oid
    }
    pub fn object(&self) -> &CommitObject {
        &self.object
    }
    pub fn paths(&self) -> &[String] {
        &self.paths
    }
}

/// Trusted post-dispatch read. No index, worktree or ref is written.
/// Host must retain its resource lease and independently observe current
/// repository identity/branch/HEAD and the pre-write isolated expected tree.
pub fn read_approved_git_commit_changes(
    dispatch: &Dispatch,
    resource_id: &str,
    root: &Path,
    prepared: &PreparedCommit,
    trusted_git_program: &Path,
    oid: &str,
) -> Result<GitCommitChanges> {
    verify_git_commit_approval(dispatch, resource_id, root, prepared)?;
    let deadline = Instant::now() + Duration::from_secs(15);
    let object = parse_commit_object(&read_authenticated_object(
        root,
        trusted_git_program,
        oid,
        "commit",
        deadline,
    )?)
    .map_err(|_| unavailable())?;
    let parents: Vec<_> = prepared
        .expected_head()
        .into_iter()
        .map(str::to_owned)
        .collect();
    if object.parents != parents {
        return Err(unavailable());
    }
    let parent_tree = if let Some(parent) = prepared.expected_head() {
        Some(
            parse_commit_object(&read_authenticated_object(
                root,
                trusted_git_program,
                parent,
                "commit",
                deadline,
            )?)
            .map_err(|_| unavailable())?
            .tree,
        )
    } else {
        None
    };
    let mut reader = TreeReader {
        root,
        program: trusted_git_program,
        deadline,
        visits: 0,
    };
    let before = match parent_tree {
        Some(tree) => reader.snapshot(&tree)?,
        None => BTreeMap::new(),
    };
    let after = reader.snapshot(&object.tree)?;
    let paths = changed_paths(&before, &after);
    validate_commit_request("tree evidence", &paths).map_err(|_| unavailable())?;
    if paths.iter().any(|p| !prepared.paths().contains(p)) {
        return Err(unavailable());
    }
    Ok(GitCommitChanges {
        oid: oid.into(),
        object,
        paths,
    })
}

// Authenticated parent manifest for independent in-memory tree planning.
pub(super) fn read_approved_parent_files(
    dispatch: &Dispatch,
    resource_id: &str,
    root: &Path,
    prepared: &PreparedCommit,
    program: &Path,
) -> Result<(String, Files, usize)> {
    verify_git_commit_approval(dispatch, resource_id, root, prepared)?;
    let parent = prepared.expected_head().ok_or_else(unavailable)?;
    let deadline = Instant::now() + Duration::from_secs(15);
    let object = parse_commit_object(&read_authenticated_object(
        root, program, parent, "commit", deadline,
    )?)
    .map_err(|_| unavailable())?;
    let mut reader = TreeReader {
        root,
        program,
        deadline,
        visits: 0,
    };
    let files = reader.snapshot(&object.tree)?;
    Ok((object.tree, files, reader.visits))
}

pub(super) type Files = BTreeMap<String, (String, String)>;
fn changed_paths(before: &Files, after: &Files) -> Vec<String> {
    before
        .keys()
        .chain(after.keys())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .filter(|p| before.get(*p) != after.get(*p))
        .cloned()
        .collect()
}

struct TreeReader<'a> {
    root: &'a Path,
    program: &'a Path,
    deadline: Instant,
    visits: usize,
}
impl TreeReader<'_> {
    fn snapshot(&mut self, tree: &str) -> Result<Files> {
        let mut files = BTreeMap::new();
        let mut aliases = std::collections::BTreeSet::new();
        self.walk(tree, "", 0, &mut files, &mut aliases)?;
        Ok(files)
    }
    fn walk(
        &mut self,
        tree: &str,
        prefix: &str,
        depth: usize,
        files: &mut Files,
        aliases: &mut std::collections::BTreeSet<String>,
    ) -> Result<()> {
        if depth > 32 || self.visits >= 64 || Instant::now() >= self.deadline {
            return Err(unavailable());
        }
        self.visits += 1;
        let raw = read_authenticated_object(self.root, self.program, tree, "tree", self.deadline)?;
        for entry in parse_tree(&raw, tree.len() / 2)? {
            let path = format!("{prefix}{}", entry.name);
            if path.len() > 1024 || !aliases.insert(path.to_lowercase()) {
                return Err(unavailable());
            }
            if entry.mode == "40000" {
                self.walk(&entry.oid, &format!("{path}/"), depth + 1, files, aliases)?;
            } else {
                if files.len() >= 4096 {
                    return Err(unavailable());
                }
                files.insert(path, (entry.mode, entry.oid));
            }
        }
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Entry {
    mode: String,
    name: String,
    oid: String,
}

fn unavailable() -> ServiceError {
    ServiceError::new(
        "GIT_TREE_EVIDENCE_UNAVAILABLE",
        "无法完整核验提交变更范围；不得宣布成功或重派写入",
    )
}

fn parse_tree(mut bytes: &[u8], oid_bytes: usize) -> Result<Vec<Entry>> {
    if !matches!(oid_bytes, 20 | 32) || bytes.len() > 65536 {
        return Err(unavailable());
    }
    let mut result = Vec::new();
    let mut names = std::collections::BTreeSet::new();
    while !bytes.is_empty() {
        let space = bytes
            .iter()
            .position(|b| *b == b' ')
            .ok_or_else(unavailable)?;
        let mode = std::str::from_utf8(&bytes[..space]).map_err(|_| unavailable())?;
        if !matches!(mode, "40000" | "100644" | "100755") {
            return Err(unavailable());
        }
        bytes = &bytes[space + 1..];
        let end = bytes.iter().position(|b| *b == 0).ok_or_else(unavailable)?;
        let name = std::str::from_utf8(&bytes[..end]).map_err(|_| unavailable())?;
        if name.contains('/')
            || validate_commit_request("tree entry", &[name.into()]).is_err()
            || !names.insert(name.to_lowercase())
        {
            return Err(unavailable());
        }
        bytes = &bytes[end + 1..];
        if bytes.len() < oid_bytes || bytes[..oid_bytes].iter().all(|b| *b == 0) {
            return Err(unavailable());
        }
        let oid = bytes[..oid_bytes]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        result.push(Entry {
            mode: mode.into(),
            name: name.into(),
            oid,
        });
        bytes = &bytes[oid_bytes..];
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(mode: &str, name: &[u8], oid_bytes: usize) -> Vec<u8> {
        let mut raw = format!("{mode} ").into_bytes();
        raw.extend_from_slice(name);
        raw.push(0);
        raw.extend(vec![0xab; oid_bytes]);
        raw
    }
    #[test]
    fn binary_tree_preserves_names_modes_and_hash_width() {
        for width in [20, 32] {
            let raw = entry("100755", "src文件".as_bytes(), width);
            assert_eq!(
                parse_tree(&raw, width).unwrap(),
                vec![Entry {
                    mode: "100755".into(),
                    name: "src文件".into(),
                    oid: "ab".repeat(width),
                }]
            );
        }
    }
    #[test]
    fn tree_rejects_truncation_aliases_unsupported_modes_and_ambiguous_names() {
        for (mode, name) in [
            ("100644", b"../a".as_slice()),
            ("120000", b"link"),
            ("160000", b"module"),
            ("100664", b"a"),
            ("100644", b".git"),
            ("100644", b"a/b"),
            ("100644", &[0xff]),
        ] {
            assert!(parse_tree(&entry(mode, name, 20), 20).is_err());
        }
        let raw = entry("100644", b"a", 20);
        assert!(parse_tree(&raw[..raw.len() - 1], 20).is_err());
        let mut duplicate = raw.clone();
        duplicate.extend(entry("100755", b"A", 20));
        assert!(parse_tree(&duplicate, 20).is_err());
        assert!(parse_tree(&raw, 19).is_err());
    }
    fn program() -> std::path::PathBuf {
        std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|p| p.join(if cfg!(windows) { "git.exe" } else { "git" }))
            .find(|p| p.is_absolute() && p.is_file())
            .expect("Git required")
    }
    fn git(root: &Path, args: &[&str]) -> String {
        let output = std::process::Command::new(program())
            .current_dir(root)
            .args([
                "-c",
                "user.name=WEX",
                "-c",
                "user.email=wex@example.invalid",
                "-c",
                "commit.gpgSign=false",
                "-c",
                "core.hooksPath=",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "Git fixture failed: {:?}",
            output.stderr
        );
        String::from_utf8(output.stdout).unwrap().trim().into()
    }
    fn setup(format: &str) -> (tempfile::TempDir, String, String) {
        let dir = tempfile::tempdir().unwrap();
        git(
            dir.path(),
            &[
                "init",
                "--quiet",
                "-b",
                "work",
                &format!("--object-format={format}"),
            ],
        );
        std::fs::create_dir(dir.path().join("dir")).unwrap();
        for (name, text) in [
            ("a", "initial"),
            ("dir/old", "rename"),
            ("unchanged", "keep"),
        ] {
            std::fs::write(dir.path().join(name), text).unwrap();
        }
        git(dir.path(), &["add", "--all"]);
        git(dir.path(), &["commit", "--quiet", "-m", "initial"]);
        let parent = git(dir.path(), &["rev-parse", "HEAD"]);
        std::fs::write(dir.path().join("a"), "modified").unwrap();
        std::fs::rename(dir.path().join("dir/old"), dir.path().join("dir/new")).unwrap();
        git(dir.path(), &["add", "--all"]);
        git(dir.path(), &["commit", "--quiet", "-m", "second"]);
        let oid = git(dir.path(), &["rev-parse", "HEAD"]);
        (dir, parent, oid)
    }
    fn approved(root: &Path, parent: &str, paths: &[&str]) -> (PreparedCommit, Dispatch) {
        use wex_core::operation::{Action, OperationIntent};
        let prepared = PreparedCommit::new(
            "second\n".into(),
            paths.iter().map(|s| (*s).into()).collect(),
            crate::RepositoryVersion {
                identity: "trusted_host_identity".into(),
                head: Some(parent.into()),
                branch: "refs/heads/work".into(),
                index_sha256: "a".repeat(64),
                selected_sha256: "b".repeat(64),
            },
        )
        .unwrap();
        let intent = OperationIntent {
            id: "op".into(),
            actor_id: "agent".into(),
            conversation_id: "chat".into(),
            context_id: None,
            connection_id: "local".into(),
            capability_id: "git_commit".into(),
            resource_id: "repo".into(),
            action: Action::Write,
            expected_version: Some(parent.into()),
            intent_digest: crate::git_commit_intent_digest("repo", root, &prepared).unwrap(),
        };
        (
            prepared,
            Dispatch {
                intent,
                attempt_id: "attempt".into(),
            },
        )
    }
    #[test]
    fn real_sha1_and_sha256_trees_include_both_rename_sides_and_only_actual_changes() {
        for format in ["sha1", "sha256"] {
            let (dir, parent, oid) = setup(format);
            let (prepared, dispatch) = approved(
                dir.path(),
                &parent,
                &["a", "dir/new", "dir/old", "unchanged"],
            );
            let evidence = read_approved_git_commit_changes(
                &dispatch,
                "repo",
                dir.path(),
                &prepared,
                &program(),
                &oid,
            )
            .unwrap();
            assert_eq!(evidence.oid(), oid);
            assert_eq!(evidence.object().parents, vec![parent]);
            assert_eq!(evidence.object().message, "second\n");
            assert_eq!(evidence.paths(), &["a", "dir/new", "dir/old"]);
        }
    }
    #[test]
    fn unapproved_rename_side_and_wrong_parent_are_rejected() {
        let (dir, parent, oid) = setup("sha1");
        let (prepared, dispatch) = approved(dir.path(), &parent, &["a", "dir/new"]);
        assert!(read_approved_git_commit_changes(
            &dispatch,
            "repo",
            dir.path(),
            &prepared,
            &program(),
            &oid
        )
        .is_err());
        let (prepared, dispatch) = approved(dir.path(), &oid, &["a", "dir/new", "dir/old"]);
        assert!(read_approved_git_commit_changes(
            &dispatch,
            "repo",
            dir.path(),
            &prepared,
            &program(),
            &oid
        )
        .is_err());
    }
    #[test]
    fn replacement_tree_is_ignored_and_corrupted_loose_tree_is_rejected() {
        let (dir, parent, oid) = setup("sha1");
        let tree = git(dir.path(), &["rev-parse", "HEAD^{tree}"]);
        let old_tree = git(dir.path(), &["rev-parse", &format!("{parent}^{{tree}}")]);
        let (prepared, dispatch) = approved(dir.path(), &parent, &["a", "dir/new", "dir/old"]);
        git(dir.path(), &["replace", &tree, &old_tree]);
        let evidence = read_approved_git_commit_changes(
            &dispatch,
            "repo",
            dir.path(),
            &prepared,
            &program(),
            &oid,
        )
        .unwrap();
        assert_eq!(evidence.paths(), &["a", "dir/new", "dir/old"]);
        let object_path = |id: &str| {
            dir.path()
                .join(".git/objects")
                .join(&id[..2])
                .join(&id[2..])
        };
        let target = object_path(&tree);
        #[cfg(windows)]
        {
            let mut permissions = std::fs::metadata(&target).unwrap().permissions();
            permissions.set_readonly(false);
            std::fs::set_permissions(&target, permissions).unwrap();
        }
        std::fs::remove_file(&target).unwrap();
        std::fs::copy(object_path(&old_tree), &target).unwrap();
        assert!(read_approved_git_commit_changes(
            &dispatch,
            "repo",
            dir.path(),
            &prepared,
            &program(),
            &oid
        )
        .is_err());
    }
    #[test]
    fn whole_tree_budget_is_shared_by_parent_and_candidate() {
        let (dir, _parent, _oid) = setup("sha1");
        for n in 0..33 {
            let path = dir.path().join(format!("many/{n}"));
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(path.join("leaf"), "same").unwrap();
        }
        git(dir.path(), &["add", "--all"]);
        git(dir.path(), &["commit", "--quiet", "-m", "wide parent"]);
        let parent = git(dir.path(), &["rev-parse", "HEAD"]);
        std::fs::write(dir.path().join("a"), "new").unwrap();
        git(dir.path(), &["add", "--all"]);
        git(dir.path(), &["commit", "--quiet", "-m", "second"]);
        let oid = git(dir.path(), &["rev-parse", "HEAD"]);
        let (prepared, dispatch) = approved(dir.path(), &parent, &["a"]);
        assert!(read_approved_git_commit_changes(
            &dispatch,
            "repo",
            dir.path(),
            &prepared,
            &program(),
            &oid
        )
        .is_err());
    }
    #[test]
    #[cfg(unix)]
    fn owner_execute_only_change_is_included_even_when_blob_oid_is_unchanged() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, _parent, _oid) = setup("sha1");
        git(dir.path(), &["config", "core.filemode", "true"]);
        let parent = git(dir.path(), &["rev-parse", "HEAD"]);
        std::fs::set_permissions(dir.path().join("a"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
        git(dir.path(), &["add", "--", "a"]);
        git(dir.path(), &["commit", "--quiet", "-m", "second"]);
        let oid = git(dir.path(), &["rev-parse", "HEAD"]);
        let (prepared, dispatch) = approved(dir.path(), &parent, &["a"]);
        assert_eq!(
            read_approved_git_commit_changes(
                &dispatch,
                "repo",
                dir.path(),
                &prepared,
                &program(),
                &oid
            )
            .unwrap()
            .paths(),
            &["a"]
        );
    }
}
