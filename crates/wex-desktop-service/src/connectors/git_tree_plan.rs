//! Independent raw-byte Git object planning. No object/index/ref is written.
use super::git_tree_evidence::Files;
use super::{
    git_commit_admission::{git_commit_resource_id, verify_durable_admission, GitCommitAdmission},
    git_commit_snapshot::{
        read_prepared_files, verify_live_git_commit_dispatch, GitCommitSnapshotSource,
    },
};
use crate::{Result, ServiceError};
use std::collections::{BTreeMap, BTreeSet};
use wex_workspace_store::operations::OperationStore;

/// Data to persist later, not permission to write the repository.
#[derive(Debug)]
pub struct PlannedGitObject {
    kind: &'static str,
    oid: String,
    bytes: Vec<u8>,
}
impl PlannedGitObject {
    pub fn kind(&self) -> &str {
        self.kind
    }
    pub fn oid(&self) -> &str {
        &self.oid
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Immutable, attempt-bound independent expected tree built before writing.
/// Bytes are deliberately raw: Git filters/attributes/config are not executed.
#[derive(Debug)]
pub struct GitCommitTreePlan {
    tree: String,
    parent: String,
    objects: Vec<PlannedGitObject>,
    operation_id: String,
    attempt_id: String,
    generation: u64,
    intent_digest: String,
}
impl GitCommitTreePlan {
    pub fn tree(&self) -> &str {
        &self.tree
    }
    pub fn parent(&self) -> &str {
        &self.parent
    }
    pub fn objects(&self) -> &[PlannedGitObject] {
        &self.objects
    }
    pub(super) fn verify_binding(&self, admission: &GitCommitAdmission) -> Result<()> {
        let committed = admission.committed();
        if self.operation_id != committed.dispatch.intent.id
            || self.attempt_id != committed.dispatch.attempt_id
            || self.generation != committed.generation
            || self.intent_digest != committed.dispatch.intent.intent_digest
            || Some(self.parent.as_str()) != admission.prepared().expected_head()
        {
            return Err(invalid());
        }
        Ok(())
    }
}

/// Unix raw-file planning slice. Requires a live durable admission and shared
/// Host authority/resource ownership. Does not touch object storage or indexes.
/// Configured filter/encoding/line-ending and Windows mode semantics remain
/// future execution policy; this is not a replacement for ordinary git add.
pub fn prepare_admitted_git_tree(
    operations: &OperationStore,
    admission: &GitCommitAdmission,
    source: &GitCommitSnapshotSource<'_>,
    trusted_git_program: &std::path::Path,
) -> Result<GitCommitTreePlan> {
    if !cfg!(unix) {
        return Err(invalid());
    }
    verify_durable_admission(operations, admission.committed())?;
    let resource = git_commit_resource_id(source)?;
    let dispatch = &admission.committed().dispatch;
    let prepared = admission.prepared();
    verify_live_git_commit_dispatch(dispatch, &resource, source, prepared)?;
    let root = source
        .registry
        .export_binding(source.directory_id)
        .map_err(|_| invalid())?
        .path;
    let (parent_tree, mut files, parent_visits) =
        super::git_tree_evidence::read_approved_parent_files(
            dispatch,
            &resource,
            &root,
            prepared,
            trusted_git_program,
        )?;
    let parent = prepared.expected_head().ok_or_else(invalid)?;
    let width = parent.len();
    // Reject empty-directory/noncanonical structures that the file manifest
    // cannot round-trip, rather than silently dropping them during planning.
    if build_tree(&files, width)?.oid != parent_tree {
        return Err(invalid());
    }
    let selected = read_prepared_files(source, prepared)?;
    if selected.len() != prepared.paths().len() {
        return Err(invalid());
    }
    let mut objects = BTreeMap::new();
    for (path, file) in prepared.paths().iter().zip(selected) {
        let oid = object_oid("blob", &file.bytes, width)?;
        let mode = if file.executable { "100755" } else { "100644" };
        files.insert(path.clone(), (mode.into(), oid.clone()));
        objects.insert(("blob", oid), file.bytes);
    }
    let tree = build_tree(&files, width)?;
    if tree.visits + parent_visits > 64 {
        return Err(invalid());
    }
    if tree.oid == parent_tree {
        return Err(ServiceError::new(
            "GIT_NOTHING_TO_COMMIT",
            "批准范围的原始字节与模式没有变化",
        ));
    }
    for (oid, raw) in tree.objects {
        objects.insert(("tree", oid), raw);
    }
    if objects.values().map(Vec::len).sum::<usize>() > 16 * 1024 * 1024 {
        return Err(invalid());
    }
    verify_live_git_commit_dispatch(dispatch, &resource, source, prepared)?;
    verify_durable_admission(operations, admission.committed())?;
    Ok(GitCommitTreePlan {
        tree: tree.oid,
        parent: parent.into(),
        objects: objects
            .into_iter()
            .map(|((kind, oid), bytes)| PlannedGitObject { kind, oid, bytes })
            .collect(),
        operation_id: dispatch.intent.id.clone(),
        attempt_id: dispatch.attempt_id.clone(),
        generation: admission.committed().generation,
        intent_digest: dispatch.intent.intent_digest.clone(),
    })
}

fn invalid() -> ServiceError {
    ServiceError::new(
        "GIT_TREE_PLAN_UNAVAILABLE",
        "无法从批准的原始文件构造独立提交树",
    )
}

struct BuiltTree {
    oid: String,
    objects: BTreeMap<String, Vec<u8>>,
    visits: usize,
}
fn object_oid(kind: &str, bytes: &[u8], width: usize) -> Result<String> {
    let algorithm = match width {
        40 => &ring::digest::SHA1_FOR_LEGACY_USE_ONLY,
        64 => &ring::digest::SHA256,
        _ => return Err(invalid()),
    };
    let mut hash = ring::digest::Context::new(algorithm);
    hash.update(format!("{kind} {}\0", bytes.len()).as_bytes());
    hash.update(bytes);
    Ok(hash
        .finish()
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}
#[derive(Default)]
struct Node {
    files: BTreeMap<String, (String, String)>,
    dirs: BTreeMap<String, Node>,
}
impl Node {
    fn insert(&mut self, parts: &[&str], value: &(String, String)) -> Result<()> {
        let name = parts[0];
        if parts.len() == 1 {
            if self.dirs.contains_key(name)
                || self.files.insert(name.into(), value.clone()).is_some()
            {
                return Err(invalid());
            }
        } else {
            if self.files.contains_key(name) {
                return Err(invalid());
            }
            self.dirs
                .entry(name.into())
                .or_default()
                .insert(&parts[1..], value)?;
        }
        Ok(())
    }
    fn encode(&self, width: usize, built: &mut BuiltTree) -> Result<String> {
        if built.visits >= 64 {
            return Err(invalid());
        }
        built.visits += 1;
        let mut entries = Vec::new();
        let mut aliases = BTreeSet::new();
        for (name, (mode, oid)) in &self.files {
            if !aliases.insert(name.to_lowercase()) {
                return Err(invalid());
            }
            entries.push((
                name.as_bytes().to_vec(),
                name.clone(),
                mode.clone(),
                oid.clone(),
            ));
        }
        for (name, node) in &self.dirs {
            if !aliases.insert(name.to_lowercase()) {
                return Err(invalid());
            }
            let oid = node.encode(width, built)?;
            entries.push((
                format!("{name}/").into_bytes(),
                name.clone(),
                "40000".into(),
                oid,
            ));
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        let mut raw = Vec::new();
        for (_, name, mode, oid) in entries {
            raw.extend(format!("{mode} {name}\0").as_bytes());
            for index in (0..oid.len()).step_by(2) {
                raw.push(u8::from_str_radix(&oid[index..index + 2], 16).map_err(|_| invalid())?);
            }
        }
        if raw.len() > 65536 {
            return Err(invalid());
        }
        let oid = object_oid("tree", &raw, width)?;
        built.objects.insert(oid.clone(), raw);
        Ok(oid)
    }
}
fn build_tree(files: &Files, width: usize) -> Result<BuiltTree> {
    if !matches!(width, 40 | 64) || files.len() > 4096 {
        return Err(invalid());
    }
    let mut root = Node::default();
    for (path, value) in files {
        super::git_commit_policy::validate_commit_request("tree plan", std::slice::from_ref(path))
            .map_err(|_| invalid())?;
        let parts: Vec<_> = path.split('/').collect();
        if parts.len() > 33
            || !matches!(value.0.as_str(), "100644" | "100755")
            || value.1.len() != width
            || !value
                .1
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || value.1.bytes().all(|b| b == b'0')
        {
            return Err(invalid());
        }
        root.insert(&parts, value)?;
    }
    let mut built = BuiltTree {
        oid: String::new(),
        objects: BTreeMap::new(),
        visits: 0,
    };
    built.oid = root.encode(width, &mut built)?;
    Ok(built)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn planning_rejects_excessive_depth_tree_visits_and_oversized_tree_objects() {
        let deep = BTreeMap::from([(
            vec!["nested"; 34].join("/"),
            ("100644".into(), "11".repeat(20)),
        )]);
        assert!(build_tree(&deep, 40).is_err());
        let wide = (0..65)
            .map(|n| (format!("dir{n}/leaf"), ("100644".into(), "11".repeat(20))))
            .collect();
        assert!(build_tree(&wide, 40).is_err());
        let large = (0..700)
            .map(|n| {
                (
                    format!("{n:04}{}", "x".repeat(100)),
                    ("100644".into(), "11".repeat(20)),
                )
            })
            .collect();
        assert!(build_tree(&large, 40).is_err());
    }
    #[test]
    fn canonical_tree_uses_git_directory_sorting_and_preserves_raw_modes() {
        let files = BTreeMap::from([
            ("a.c".into(), ("100644".into(), "11".repeat(20))),
            ("a/child".into(), ("100755".into(), "22".repeat(20))),
            ("a0".into(), ("100644".into(), "33".repeat(20))),
        ]);
        let built = build_tree(&files, 40).unwrap();
        let root = built.objects.get(&built.oid).unwrap();
        assert!(root.starts_with(b"100644 a.c\0"));
        let names = root
            .windows(8)
            .filter(|s| s.starts_with(b"40000 a\0"))
            .count();
        assert_eq!(names, 1);
        assert_eq!(built.visits, 2);
        assert_eq!(built.objects.len(), 2);
        assert!(built
            .objects
            .values()
            .any(|raw| raw.starts_with(b"100755 child\0")));
    }
    #[test]
    fn planner_rejects_prefix_conflicts_aliases_and_invalid_object_ids() {
        for files in [
            BTreeMap::from([
                ("a".into(), ("100644".into(), "11".repeat(20))),
                ("a/child".into(), ("100644".into(), "22".repeat(20))),
            ]),
            BTreeMap::from([
                ("a/one".into(), ("100644".into(), "11".repeat(20))),
                ("A/two".into(), ("100644".into(), "22".repeat(20))),
            ]),
            BTreeMap::from([("a".into(), ("100644".into(), "invalid".into()))]),
        ] {
            assert!(build_tree(&files, 40).is_err());
        }
    }
    #[cfg(unix)]
    fn git_program() -> std::path::PathBuf {
        std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|p| p.join("git"))
            .find(|p| p.is_absolute() && p.is_file())
            .unwrap()
    }
    #[cfg(unix)]
    fn git(root: &std::path::Path, args: &[&str], input: Option<&[u8]>) -> String {
        use std::{
            io::Write,
            process::{Command, Stdio},
        };
        let mut child = Command::new(git_program())
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
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        if let Some(bytes) = input {
            child.stdin.take().unwrap().write_all(bytes).unwrap();
        } else {
            drop(child.stdin.take());
        }
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "Git fixture failed: {:?}",
            output.stderr
        );
        String::from_utf8(output.stdout).unwrap().trim().into()
    }
    #[cfg(unix)]
    fn object_files(root: &std::path::Path) -> BTreeMap<std::path::PathBuf, Vec<u8>> {
        fn walk(dir: &std::path::Path, files: &mut BTreeMap<std::path::PathBuf, Vec<u8>>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(&path, files);
                } else {
                    files.insert(path.clone(), std::fs::read(path).unwrap());
                }
            }
        }
        let mut files = BTreeMap::new();
        walk(&root.join(".git/objects"), &mut files);
        files
    }
    #[cfg(unix)]
    fn with_fixture(
        format: &str,
        changed: bool,
        run: impl FnOnce(
            &OperationStore,
            &GitCommitAdmission,
            &GitCommitSnapshotSource<'_>,
            &std::path::Path,
        ),
    ) {
        use wex_core::{
            access::{Access, DirectoryGrant},
            operation::{Action, ExecutionChecks, OperationIntent},
        };
        use wex_workspace_store::operations::OperationChange;
        let dir = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(
            root,
            &[
                "init",
                "--quiet",
                "-b",
                "work",
                &format!("--object-format={format}"),
            ],
            None,
        );
        std::fs::create_dir(root.join("src")).unwrap();
        std::fs::create_dir(root.join("a")).unwrap();
        for (path, bytes) in [
            ("src/a", b"before\0raw".as_slice()),
            ("unrelated", b"original"),
            ("a.c", b"sort"),
            ("a/child", b"sort"),
            ("a0", b"sort"),
            (".gitattributes", b"src/a filter=tripwire\n"),
        ] {
            std::fs::write(root.join(path), bytes).unwrap();
        }
        git(root, &["add", "--all"], None);
        git(root, &["commit", "--quiet", "-m", "initial"], None);
        // A naive git add on the selected file now fails. Native planning must
        // never run this filter or consult the dirty real index for its tree.
        std::fs::write(root.join("unrelated"), "staged but unapproved").unwrap();
        git(root, &["add", "--", "unrelated"], None);
        git(root, &["config", "filter.tripwire.clean", "false"], None);
        git(root, &["config", "filter.tripwire.required", "true"], None);
        if changed {
            std::fs::write(root.join("src/a"), b"after\r\n\0raw\n").unwrap();
        }
        let mut registry = wex_local_resources::DirectoryRegistry::new();
        let id = registry
            .register(root, wex_local_resources::Protection::General)
            .unwrap();
        let grants = [DirectoryGrant {
            directory_id: &id,
            access: Access::ReadWrite,
            explicit_personal: false,
        }];
        let source = GitCommitSnapshotSource {
            registry: &registry,
            directory_id: &id,
            system_access: true,
            grants: &grants,
            internal_roots: &[],
        };
        let prepared =
            crate::prepare_git_commit_from_registry(&source, "planned\n", &["src/a".into()])
                .unwrap();
        let resource = git_commit_resource_id(&source).unwrap();
        let intent = OperationIntent {
            id: "op".into(),
            actor_id: "agent".into(),
            conversation_id: "chat".into(),
            context_id: None,
            connection_id: "local".into(),
            capability_id: "git_commit".into(),
            resource_id: resource.clone(),
            action: Action::Write,
            expected_version: prepared.expected_head().map(str::to_owned),
            intent_digest: crate::git_commit_intent_digest(&resource, root, &prepared).unwrap(),
        };
        let store = OperationStore::open(data.path().join("operations")).unwrap();
        let view = store.load().unwrap();
        let view = store
            .apply(view.revision, OperationChange::Propose(intent.clone()))
            .unwrap();
        let view = store
            .apply(
                view.revision,
                OperationChange::Approve {
                    id: "op".into(),
                    exact_intent: intent,
                    authority_revision: 1,
                },
            )
            .unwrap();
        let leases = wex_core::dispatch_leases::ResourceLeases::default();
        let admission = crate::begin_git_commit(
            &store,
            &leases,
            crate::GitCommitBegin {
                expected_store_revision: view.revision,
                operation_id: "op",
                attempt_id: "attempt",
                checks: ExecutionChecks {
                    authority_revision: 1,
                    connection_enabled: true,
                    authenticated: true,
                    platform_available: true,
                    resource_allowed: true,
                },
                source: &source,
                prepared,
            },
        )
        .unwrap();
        run(&store, &admission, &source, root);
    }
    #[cfg(unix)]
    #[test]
    fn raw_sha1_and_sha256_plans_preserve_staged_work_and_feed_real_verified_settlement() {
        for format in ["sha1", "sha256"] {
            with_fixture(format, true, |store, admission, source, root| {
                let index = std::fs::read(root.join(".git/index")).unwrap();
                let objects = object_files(root);
                let parent = git(root, &["rev-parse", "HEAD"], None);
                let plan =
                    prepare_admitted_git_tree(store, admission, source, &git_program()).unwrap();
                assert_eq!(plan.parent(), parent);
                assert_eq!(std::fs::read(root.join(".git/index")).unwrap(), index);
                assert_eq!(git(root, &["rev-parse", "HEAD"], None), parent);
                assert_eq!(object_files(root), objects);
                assert!(
                    plan.objects()
                        .iter()
                        .any(|object| object.kind() == "blob"
                            && object.bytes() == b"after\r\n\0raw\n")
                );
                for object in plan.objects() {
                    assert_eq!(
                        git(
                            root,
                            &[
                                "hash-object",
                                "--no-filters",
                                "-t",
                                object.kind(),
                                "--stdin"
                            ],
                            Some(object.bytes())
                        ),
                        object.oid()
                    );
                }
                // ONLY the test fixture persists objects and updates a ref.
                for object in plan.objects() {
                    assert_eq!(
                        git(
                            root,
                            &[
                                "hash-object",
                                "--no-filters",
                                "-w",
                                "-t",
                                object.kind(),
                                "--stdin"
                            ],
                            Some(object.bytes())
                        ),
                        object.oid()
                    );
                }
                let oid = git(
                    root,
                    &[
                        "commit-tree",
                        plan.tree(),
                        "-p",
                        plan.parent(),
                        "-m",
                        "planned",
                    ],
                    None,
                );
                git(
                    root,
                    &["update-ref", "refs/heads/work", &oid, plan.parent()],
                    None,
                );
                let receipt = crate::read_and_settle_planned_git_commit(
                    store,
                    admission,
                    source,
                    &git_program(),
                    &plan,
                    &oid,
                )
                .unwrap();
                assert_eq!(receipt.commit_oid(), oid);
                assert_eq!(
                    git(root, &["show", &format!("{oid}:unrelated")], None),
                    "original"
                );
                assert_eq!(std::fs::read(root.join(".git/index")).unwrap(), index);
            });
        }
    }
    #[cfg(unix)]
    #[test]
    fn unchanged_selected_files_do_not_import_unrelated_staged_changes() {
        with_fixture("sha1", false, |store, admission, source, root| {
            let before = object_files(root);
            assert_eq!(
                prepare_admitted_git_tree(store, admission, source, &git_program())
                    .unwrap_err()
                    .code,
                "GIT_NOTHING_TO_COMMIT"
            );
            assert_eq!(object_files(root), before);
            assert!(matches!(
                store.load().unwrap().ledger.records[0].state,
                wex_core::operation::OperationState::InFlight { .. }
            ));
        });
    }
    #[cfg(unix)]
    #[test]
    fn changed_approved_bytes_or_revoked_write_access_cannot_produce_plan() {
        with_fixture("sha1", true, |store, admission, source, root| {
            let before = object_files(root);
            std::fs::write(root.join("src/a"), "changed after approval").unwrap();
            assert_eq!(
                prepare_admitted_git_tree(store, admission, source, &git_program())
                    .unwrap_err()
                    .code,
                "VERSION_CONFLICT"
            );
            assert_eq!(object_files(root), before);
        });
        with_fixture("sha1", true, |store, admission, source, _root| {
            let grants = [wex_core::access::DirectoryGrant {
                directory_id: source.directory_id,
                access: wex_core::access::Access::Read,
                explicit_personal: false,
            }];
            let revoked = GitCommitSnapshotSource {
                registry: source.registry,
                directory_id: source.directory_id,
                system_access: true,
                grants: &grants,
                internal_roots: &[],
            };
            assert_eq!(
                prepare_admitted_git_tree(store, admission, &revoked, &git_program())
                    .unwrap_err()
                    .code,
                "PERMISSION_DENIED"
            );
        });
    }
}
