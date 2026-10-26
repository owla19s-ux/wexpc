//! Immutable original-candidate evidence, never recovery/replay authorization.
use super::{
    git_commit_admission::{verify_durable_admission, GitCommitAdmission},
    git_commit_candidate::GitCommitCandidate,
};
use crate::{Result, ServiceError};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    path::{Path, PathBuf},
};
use wex_workspace_store::operations::OperationStore;

const LIMIT: u64 = 256 * 1024;
fn unavailable() -> ServiceError {
    ServiceError::new(
        "GIT_CANDIDATE_JOURNAL_ERROR",
        "原始提交候选记录不可用；不得派发或自动重试写入",
    )
}
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ObjectRef {
    kind: String,
    oid: String,
}
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema_version: u32,
    operation_id: String,
    attempt_id: String,
    generation: u64,
    resource_id: String,
    intent_digest: String,
    parent: String,
    tree: String,
    candidate_oid: String,
    commit_base64: String,
    paths: Vec<String>,
    objects: Vec<ObjectRef>,
}
impl Record {
    fn key(&self) -> String {
        super::intent_binding::digest_arguments(
            "git_original_candidate",
            "private_journal",
            &[
                self.operation_id.as_bytes(),
                self.attempt_id.as_bytes(),
                self.resource_id.as_bytes(),
            ],
        )
    }
}
fn id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}
fn hex(value: &str, width: usize) -> bool {
    value.len() == width
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn validate(record: &Record) -> Result<Vec<u8>> {
    let width = record.candidate_oid.len();
    if record.schema_version != 1
        || record.generation == 0
        || !id(&record.operation_id)
        || !id(&record.attempt_id)
        || !hex(&record.resource_id, 64)
        || !hex(&record.intent_digest, 64)
        || !matches!(width, 40 | 64)
        || [&record.parent, &record.tree, &record.candidate_oid]
            .iter()
            .any(|oid| !hex(oid, width) || oid.bytes().all(|b| b == b'0'))
        || record.commit_base64.len() > 88 * 1024
        || record.objects.len() > 193
    {
        return Err(unavailable());
    }
    let raw = base64::engine::general_purpose::STANDARD
        .decode(&record.commit_base64)
        .map_err(|_| unavailable())?;
    let object = super::git_tree_plan::PlannedGitObject::commit(raw.clone(), width)?;
    let parsed =
        super::git_commit_evidence::parse_commit_object(&raw).map_err(|_| unavailable())?;
    if object.oid() != record.candidate_oid
        || parsed.tree != record.tree
        || parsed.parents != [record.parent.clone()]
    {
        return Err(unavailable());
    }
    super::git_commit_policy::validate_commit_request(&parsed.message, &record.paths)
        .map_err(|_| unavailable())?;
    let mut seen = std::collections::BTreeSet::new();
    for object in &record.objects {
        if !matches!(object.kind.as_str(), "blob" | "tree" | "commit")
            || !hex(&object.oid, width)
            || object.oid.bytes().all(|b| b == b'0')
            || !seen.insert((object.kind.as_str(), object.oid.as_str()))
        {
            return Err(unavailable());
        }
    }
    if !seen.contains(&("tree", record.tree.as_str()))
        || !seen.contains(&("commit", record.candidate_oid.as_str()))
        || seen.iter().filter(|(kind, _)| *kind == "commit").count() != 1
    {
        return Err(unavailable());
    }
    Ok(raw)
}

/// Read-only recovery facts. Never reconstructs a candidate, approval or lease.
#[derive(Debug)]
pub struct GitCandidateEvidence {
    record: Record,
    raw: Vec<u8>,
}
impl GitCandidateEvidence {
    pub fn operation_id(&self) -> &str {
        &self.record.operation_id
    }
    pub fn attempt_id(&self) -> &str {
        &self.record.attempt_id
    }
    pub fn generation(&self) -> u64 {
        self.record.generation
    }
    pub fn resource_id(&self) -> &str {
        &self.record.resource_id
    }
    pub fn intent_digest(&self) -> &str {
        &self.record.intent_digest
    }
    pub fn candidate_oid(&self) -> &str {
        &self.record.candidate_oid
    }
    pub fn tree(&self) -> &str {
        &self.record.tree
    }
    pub fn parent(&self) -> &str {
        &self.record.parent
    }
    pub fn paths(&self) -> &[String] {
        &self.record.paths
    }
    pub fn commit_bytes(&self) -> &[u8] {
        &self.raw
    }
}

/// Publication confirmation, not authority to write/replay a Git operation.
#[derive(Debug)]
pub struct SavedGitCandidate {
    reference: String,
    candidate_oid: String,
}
impl SavedGitCandidate {
    pub fn reference(&self) -> &str {
        &self.reference
    }
    pub fn candidate_oid(&self) -> &str {
        &self.candidate_oid
    }
}

/// Open only an already durably created, trusted Host app-private directory.
/// Never accept a model/project path. No root creation or Windows ACL fallback.
pub struct GitCandidateJournal {
    root: PathBuf,
    directory: File,
}
impl GitCandidateJournal {
    pub fn open(root: &Path) -> Result<Self> {
        if !root.is_absolute() {
            return Err(unavailable());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let directory = std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(root)
                .map_err(|_| unavailable())?;
            let journal = Self {
                root: root.into(),
                directory,
            };
            journal.check_root()?;
            Ok(journal)
        }
        #[cfg(not(unix))]
        {
            Err(unavailable())
        }
    }
    fn check_root(&self) -> Result<()> {
        let opened = self.directory.metadata().map_err(|_| unavailable())?;
        let named = std::fs::symlink_metadata(&self.root).map_err(|_| unavailable())?;
        if !opened.is_dir() || !named.is_dir() || named.file_type().is_symlink() {
            return Err(unavailable());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if opened.dev() != named.dev()
                || opened.ino() != named.ino()
                || opened.uid() != unsafe { libc::geteuid() }
                || opened.mode() & 0o077 != 0
            {
                return Err(unavailable());
            }
            Ok(())
        }
        #[cfg(not(unix))]
        {
            Err(unavailable())
        }
    }
    /// Confirms internal evidence publication only. A repeated save never proves
    /// that external I/O has not happened; a writer must still enforce its own
    /// one-dispatch state and live authority/repository checks.
    pub fn persist(
        &self,
        operations: &OperationStore,
        admission: &GitCommitAdmission,
        candidate: &GitCommitCandidate,
    ) -> Result<SavedGitCandidate> {
        candidate.tree_plan().verify_binding(admission)?;
        verify_durable_admission(operations, admission.committed())?;
        let committed = admission.committed();
        let dispatch = &committed.dispatch;
        let record = Record {
            schema_version: 1,
            operation_id: dispatch.intent.id.clone(),
            attempt_id: dispatch.attempt_id.clone(),
            generation: committed.generation,
            resource_id: dispatch.intent.resource_id.clone(),
            intent_digest: dispatch.intent.intent_digest.clone(),
            parent: candidate.tree_plan().parent().into(),
            tree: candidate.tree_plan().tree().into(),
            candidate_oid: candidate.oid().into(),
            commit_base64: base64::engine::general_purpose::STANDARD
                .encode(candidate.commit().bytes()),
            paths: admission.prepared().paths().to_vec(),
            objects: candidate
                .objects()
                .map(|object| ObjectRef {
                    kind: object.kind().into(),
                    oid: object.oid().into(),
                })
                .collect(),
        };
        self.save(&record)?;
        // A saved record alone must not let a replaced/Unknown session proceed.
        verify_durable_admission(operations, committed)?;
        Ok(SavedGitCandidate {
            reference: record.key(),
            candidate_oid: record.candidate_oid,
        })
    }
    pub fn read(&self, reference: &str) -> Result<GitCandidateEvidence> {
        let (_, record, _) = self.read_record(reference)?;
        let raw = validate(&record)?;
        Ok(GitCandidateEvidence { record, raw })
    }
    pub fn read_attempt(
        &self,
        operation_id: &str,
        attempt_id: &str,
        resource_id: &str,
    ) -> Result<GitCandidateEvidence> {
        if !id(operation_id) || !id(attempt_id) || !hex(resource_id, 64) {
            return Err(unavailable());
        }
        let key = super::intent_binding::digest_arguments(
            "git_original_candidate",
            "private_journal",
            &[
                operation_id.as_bytes(),
                attempt_id.as_bytes(),
                resource_id.as_bytes(),
            ],
        );
        self.read(&key)
    }
    /// Stable lookup of the original unresolved attempt only. Does not inspect
    /// Git, settle Unknown or produce execution/replay permission.
    pub fn read_unresolved_operation(
        &self,
        operations: &OperationStore,
        operation_id: &str,
    ) -> Result<GitCandidateEvidence> {
        use wex_core::operation::{Action, OperationState};
        if !id(operation_id) {
            return Err(unavailable());
        }
        let view = operations.load().map_err(|_| unavailable())?;
        let record = view
            .ledger
            .records
            .iter()
            .find(|record| record.intent.id == operation_id)
            .ok_or_else(unavailable)?;
        let attempt_id = match &record.state {
            OperationState::InFlight { attempt_id }
            | OperationState::OutcomeUnknown { attempt_id } => attempt_id,
            _ => return Err(unavailable()),
        };
        let evidence = self.read_attempt(operation_id, attempt_id, &record.intent.resource_id)?;
        if record.intent.action != Action::Write
            || record.intent.capability_id != "git_commit"
            || record.intent.intent_digest != evidence.intent_digest()
            || record.intent.expected_version.as_deref() != Some(evidence.parent())
            || evidence.generation() > view.generation
        {
            return Err(unavailable());
        }
        let current = operations.load().map_err(|_| unavailable())?;
        if current.revision != view.revision || current.generation != view.generation {
            return Err(unavailable());
        }
        Ok(evidence)
    }
    fn read_record(&self, reference: &str) -> Result<(File, Record, Vec<u8>)> {
        if !hex(reference, 64) {
            return Err(unavailable());
        }
        self.check_root()?;
        #[cfg(unix)]
        {
            let (file, bytes) = native::read(&self.directory, &format!("{reference}.json"))?;
            let record: Record = serde_json::from_slice(&bytes).map_err(|_| unavailable())?;
            validate(&record)?;
            if record.key() != reference {
                return Err(unavailable());
            }
            self.check_root()?;
            Ok((file, record, bytes))
        }
        #[cfg(not(unix))]
        {
            Err(unavailable())
        }
    }
    fn save(&self, record: &Record) -> Result<()> {
        validate(record)?;
        self.check_root()?;
        let bytes = serde_json::to_vec(record).map_err(|_| unavailable())?;
        if bytes.len() as u64 > LIMIT {
            return Err(unavailable());
        }
        #[cfg(unix)]
        {
            let reference = record.key();
            match native::publish(&self.directory, &format!("{reference}.json"), &bytes) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    let (file, existing, _) = self.read_record(&reference)?;
                    if existing != *record {
                        return Err(unavailable());
                    }
                    // Idempotent internal publication still confirms durability;
                    // this is never permission to repeat the external Git write.
                    file.sync_all().map_err(|_| unavailable())?;
                    self.directory.sync_all().map_err(|_| unavailable())?;
                }
                Err(_) => return Err(unavailable()),
            }
            let (_, existing, _) = self.read_record(&reference)?;
            if existing != *record {
                return Err(unavailable());
            }
            self.check_root()
        }
        #[cfg(not(unix))]
        {
            Err(unavailable())
        }
    }
}

#[cfg(unix)]
mod native {
    use super::*;
    use std::{
        ffi::CString,
        io::{self, Read, Write},
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::fs::MetadataExt,
        },
    };
    fn open(directory: &File, name: &str, flags: i32) -> io::Result<File> {
        let name = CString::new(name)?;
        let fd = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                flags | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
                0o600,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { File::from_raw_fd(fd) })
    }
    fn stamp(file: &File) -> io::Result<[i128; 7]> {
        let m = file.metadata()?;
        Ok([
            m.dev() as i128,
            m.ino() as i128,
            m.len() as i128,
            m.mtime() as i128,
            m.mtime_nsec() as i128,
            m.ctime() as i128,
            m.ctime_nsec() as i128,
        ])
    }
    pub(super) fn read(directory: &File, name: &str) -> Result<(File, Vec<u8>)> {
        let file = open(directory, name, libc::O_RDONLY).map_err(|_| unavailable())?;
        let metadata = file.metadata().map_err(|_| unavailable())?;
        if !metadata.is_file()
            || metadata.len() > LIMIT
            || metadata.nlink() != 1
            || metadata.mode() & 0o077 != 0
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.dev() != directory.metadata().map_err(|_| unavailable())?.dev()
        {
            return Err(unavailable());
        }
        let before = stamp(&file).map_err(|_| unavailable())?;
        let mut bytes = Vec::new();
        (&file)
            .take(LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| unavailable())?;
        let named = open(directory, name, libc::O_RDONLY).map_err(|_| unavailable())?;
        if bytes.len() as u64 > LIMIT
            || stamp(&file).map_err(|_| unavailable())? != before
            || stamp(&named).map_err(|_| unavailable())? != before
        {
            return Err(unavailable());
        }
        Ok((file, bytes))
    }
    pub(super) fn publish(directory: &File, name: &str, bytes: &[u8]) -> io::Result<()> {
        let temporary = CString::new(format!(".pending-{}", uuid::Uuid::new_v4().simple()))?;
        let target = CString::new(name)?;
        let mut file = open(
            directory,
            temporary.to_str().map_err(io::Error::other)?,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
        )?;
        struct Cleanup<'a>(&'a File, CString);
        impl Drop for Cleanup<'_> {
            fn drop(&mut self) {
                unsafe {
                    libc::unlinkat(self.0.as_raw_fd(), self.1.as_ptr(), 0);
                }
            }
        }
        let cleanup = Cleanup(directory, temporary);
        file.write_all(bytes)?;
        file.sync_all()?;
        if unsafe {
            libc::linkat(
                directory.as_raw_fd(),
                cleanup.1.as_ptr(),
                directory.as_raw_fd(),
                target.as_ptr(),
                0,
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        if unsafe { libc::unlinkat(directory.as_raw_fd(), cleanup.1.as_ptr(), 0) } != 0 {
            return Err(io::Error::last_os_error());
        }
        drop(cleanup);
        directory.sync_all()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record() -> Record {
        let raw = format!("tree {}\nparent {}\nauthor WEX <a@example.invalid> 1 +0000\ncommitter WEX <a@example.invalid> 1 +0000\n\nmessage\n",
            "a".repeat(40), "b".repeat(40)).into_bytes();
        let object =
            super::super::git_tree_plan::PlannedGitObject::commit(raw.clone(), 40).unwrap();
        Record {
            schema_version: 1,
            operation_id: "op".into(),
            attempt_id: "attempt".into(),
            generation: 1,
            resource_id: "1".repeat(64),
            intent_digest: "2".repeat(64),
            parent: "b".repeat(40),
            tree: "a".repeat(40),
            candidate_oid: object.oid().into(),
            commit_base64: base64::engine::general_purpose::STANDARD.encode(raw),
            paths: vec!["src/a".into()],
            objects: vec![
                ObjectRef {
                    kind: "tree".into(),
                    oid: "a".repeat(40),
                },
                ObjectRef {
                    kind: "commit".into(),
                    oid: object.oid().into(),
                },
            ],
        }
    }
    #[test]
    fn journal_validation_authenticates_exact_commit_and_original_scope() {
        let original = record();
        assert!(validate(&original).unwrap().ends_with(b"message\n"));
        let mut tampered = record();
        tampered.commit_base64 = base64::engine::general_purpose::STANDARD.encode(b"different");
        assert!(validate(&tampered).is_err());
        let mut tampered = record();
        tampered.paths = vec!["../outside".into()];
        assert!(validate(&tampered).is_err());
        let mut tampered = record();
        tampered.objects.clear();
        assert!(validate(&tampered).is_err());
    }
    #[cfg(unix)]
    fn private_root(parent: &Path, name: &str) -> PathBuf {
        use std::os::unix::fs::DirBuilderExt;
        let root = parent.join(name);
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&root)
            .unwrap();
        root
    }
    #[cfg(unix)]
    #[test]
    fn immutable_private_record_survives_reopen_and_never_overwrites_original_attempt() {
        use std::os::unix::fs::MetadataExt;
        let data = tempfile::tempdir().unwrap();
        let root = private_root(data.path(), "journal");
        let original = record();
        let key = original.key();
        let journal = GitCandidateJournal::open(&root).unwrap();
        journal.save(&original).unwrap();
        let path = root.join(format!("{key}.json"));
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().mode() & 0o077, 0);
        assert_eq!(std::fs::metadata(&path).unwrap().nlink(), 1);
        journal.save(&original).unwrap();
        let mut conflict = record();
        conflict.generation += 1;
        assert!(journal.save(&conflict).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        drop(journal);
        let reopened = GitCandidateJournal::open(&root).unwrap();
        let evidence = reopened.read(&key).unwrap();
        assert_eq!(evidence.commit_bytes(), validate(&original).unwrap());
        assert_eq!(evidence.attempt_id(), "attempt");
        assert_eq!(evidence.generation(), 1);
    }
    #[cfg(unix)]
    #[test]
    fn renamed_root_or_public_root_never_receives_new_evidence() {
        use std::os::unix::fs::PermissionsExt;
        let data = tempfile::tempdir().unwrap();
        let root = private_root(data.path(), "journal");
        let journal = GitCandidateJournal::open(&root).unwrap();
        std::fs::rename(&root, data.path().join("moved")).unwrap();
        private_root(data.path(), "journal");
        assert!(journal.save(&record()).is_err());
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
        assert_eq!(
            std::fs::read_dir(data.path().join("moved"))
                .unwrap()
                .count(),
            0
        );
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(GitCandidateJournal::open(&root).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn symlink_hardlink_invalid_json_and_oversized_record_are_rejected() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let data = tempfile::tempdir().unwrap();
        let root = private_root(data.path(), "journal");
        let journal = GitCandidateJournal::open(&root).unwrap();
        let original = record();
        let key = original.key();
        journal.save(&original).unwrap();
        let path = root.join(format!("{key}.json"));
        let outside = data.path().join("outside");
        std::fs::hard_link(&path, &outside).unwrap();
        assert!(journal.read(&key).is_err());
        std::fs::remove_file(&outside).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&outside, &bytes).unwrap();
        symlink(&outside, &path).unwrap();
        assert!(journal.read(&key).is_err());
        std::fs::remove_file(&path).unwrap();
        for bytes in [b"not json".to_vec(), vec![b'x'; LIMIT as usize + 1]] {
            std::fs::write(&path, bytes).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            assert!(journal.read(&key).is_err());
            std::fs::remove_file(&path).unwrap();
        }
        assert!(journal.read("../escape").is_err());
    }
    #[cfg(unix)]
    #[test]
    fn concurrent_original_attempt_publication_has_only_one_winner() {
        let data = tempfile::tempdir().unwrap();
        let root = private_root(data.path(), "journal");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let workers: Vec<_> = (1..=2)
            .map(|generation| {
                let root = root.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let journal = GitCandidateJournal::open(&root).unwrap();
                    let mut record = record();
                    record.generation = generation;
                    barrier.wait();
                    journal.save(&record).is_ok()
                })
            })
            .collect();
        assert_eq!(
            workers
                .into_iter()
                .map(|worker| usize::from(worker.join().unwrap()))
                .sum::<usize>(),
            1
        );
        let journal = GitCandidateJournal::open(&root).unwrap();
        let evidence = journal
            .read_attempt("op", "attempt", &"1".repeat(64))
            .unwrap();
        assert!(matches!(evidence.generation(), 1 | 2));
    }
    #[cfg(unix)]
    #[test]
    fn interrupted_two_link_publication_remains_unavailable_without_auto_repair() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let data = tempfile::tempdir().unwrap();
        let root = private_root(data.path(), "journal");
        let original = record();
        let key = original.key();
        let pending = root.join(".pending-interrupted");
        std::fs::write(&pending, serde_json::to_vec(&original).unwrap()).unwrap();
        std::fs::set_permissions(&pending, std::fs::Permissions::from_mode(0o600)).unwrap();
        let published = root.join(format!("{key}.json"));
        std::fs::hard_link(&pending, &published).unwrap();
        let journal = GitCandidateJournal::open(&root).unwrap();
        assert!(journal.read(&key).is_err());
        assert!(journal.save(&original).is_err());
        assert!(pending.exists());
        assert_eq!(std::fs::metadata(&published).unwrap().nlink(), 2);
    }
    #[cfg(unix)]
    #[test]
    fn cold_ledger_lookup_preserves_unknown_and_rejects_mismatched_original_intent() {
        use wex_core::operation::{Action, ExecutionChecks, OperationIntent, OperationState};
        use wex_workspace_store::operations::OperationChange;
        fn inflight(root: &Path, record: &Record, digest: String) -> OperationStore {
            let store = OperationStore::open(root).unwrap();
            let intent = OperationIntent {
                id: record.operation_id.clone(),
                actor_id: "agent".into(),
                conversation_id: "chat".into(),
                context_id: None,
                connection_id: "local".into(),
                capability_id: "git_commit".into(),
                resource_id: record.resource_id.clone(),
                action: Action::Write,
                expected_version: Some(record.parent.clone()),
                intent_digest: digest,
            };
            let view = store.load().unwrap();
            let view = store
                .apply(view.revision, OperationChange::Propose(intent.clone()))
                .unwrap();
            let view = store
                .apply(
                    view.revision,
                    OperationChange::Approve {
                        id: record.operation_id.clone(),
                        exact_intent: intent,
                        authority_revision: 1,
                    },
                )
                .unwrap();
            store
                .begin(
                    view.revision,
                    &record.operation_id,
                    &record.attempt_id,
                    ExecutionChecks {
                        authority_revision: 1,
                        connection_enabled: true,
                        authenticated: true,
                        platform_available: true,
                        resource_allowed: true,
                    },
                )
                .unwrap();
            store
        }
        let data = tempfile::tempdir().unwrap();
        let root = private_root(data.path(), "journal");
        let original = record();
        let journal = GitCandidateJournal::open(&root).unwrap();
        journal.save(&original).unwrap();
        let operations_root = data.path().join("operations");
        drop(inflight(
            &operations_root,
            &original,
            original.intent_digest.clone(),
        ));
        let restarted = OperationStore::open(&operations_root).unwrap();
        let before = restarted.load().unwrap();
        assert!(matches!(
            before.ledger.records[0].state,
            OperationState::OutcomeUnknown { .. }
        ));
        assert!(before.generation > original.generation);
        let evidence = journal.read_unresolved_operation(&restarted, "op").unwrap();
        assert_eq!(evidence.candidate_oid(), original.candidate_oid);
        assert_eq!(restarted.load().unwrap(), before);
        let unrelated = inflight(
            &data.path().join("other-operations"),
            &original,
            "3".repeat(64),
        );
        assert!(journal.read_unresolved_operation(&unrelated, "op").is_err());
    }
}
