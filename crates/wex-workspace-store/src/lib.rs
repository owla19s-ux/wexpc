//! Trusted Android-first local organization metadata persistence.
//! This is not the user's Space UI, a resource owner, or a sharing service.
//! Space / directory-group / Context identities are organizational only:
//! never filesystem paths, Connector access, approvals, or AI grants.
//! Only a trusted Host supplies the private storage root.
pub mod catalog;
pub mod operations;

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const SNAPSHOT_LIMIT: u64 = 262_144;
const SPACE_LIMIT: usize = 64;
const GROUP_LIMIT: usize = 256;
const CONTEXT_LIMIT: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Space {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectoryGroup {
    pub id: String,
    pub space_id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextRecord {
    pub id: String,
    /// Local organization container; not a resource, permission or owner.
    pub space_id: String,
    /// None means ungrouped in its container (no synthetic default group).
    pub group_id: Option<String>,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    pub schema_version: u32,
    pub revision: u64,
    pub next_id: u64,
    pub spaces: Vec<Space>,
    pub groups: Vec<DirectoryGroup>,
    pub contexts: Vec<ContextRecord>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            schema_version: 2,
            revision: 0,
            next_id: 1,
            spaces: Vec::new(),
            groups: Vec::new(),
            contexts: Vec::new(),
        }
    }
}

/// Typed, read-only projection for a bounded local organization manager.
/// This is not the JSON persistence schema; callers cannot mutate a Snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OrganizationView {
    pub revision: u64,
    pub spaces: Vec<Space>,
    pub groups: Vec<DirectoryGroup>,
    pub contexts: Vec<ContextRecord>,
}

impl From<&Snapshot> for OrganizationView {
    fn from(snapshot: &Snapshot) -> Self {
        Self {
            revision: snapshot.revision,
            spaces: snapshot.spaces.clone(),
            groups: snapshot.groups.clone(),
            contexts: snapshot.contexts.clone(),
        }
    }
}

/// V1's organization containers are preserved as-is during explicit upgrade.
/// They are NEVER guessed to be the user's "personal/work/public" categories.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyContextRecord {
    id: String,
    group_id: String,
    name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacySnapshot {
    schema_version: u32,
    revision: u64,
    next_id: u64,
    spaces: Vec<Space>,
    groups: Vec<DirectoryGroup>,
    contexts: Vec<LegacyContextRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacySummary {
    pub revision: u64,
    pub spaces: usize,
    pub groups: usize,
    pub contexts: usize,
}

fn decode_legacy(bytes: &[u8]) -> Result<(Snapshot, LegacySummary), OrganizationError> {
    let legacy: LegacySnapshot =
        serde_json::from_slice(bytes).map_err(|_| OrganizationError::InvalidSnapshot)?;
    if legacy.schema_version != 1 {
        return Err(OrganizationError::InvalidSnapshot);
    }
    let summary = LegacySummary {
        revision: legacy.revision,
        spaces: legacy.spaces.len(),
        groups: legacy.groups.len(),
        contexts: legacy.contexts.len(),
    };
    let contexts = legacy
        .contexts
        .into_iter()
        .map(|context| {
            let group = legacy
                .groups
                .iter()
                .find(|group| group.id == context.group_id)
                .ok_or(OrganizationError::InvalidSnapshot)?;
            Ok(ContextRecord {
                id: context.id,
                space_id: group.space_id.clone(),
                group_id: Some(context.group_id),
                name: context.name,
            })
        })
        .collect::<Result<Vec<_>, OrganizationError>>()?;
    let converted = Snapshot {
        schema_version: 2,
        revision: legacy.revision,
        next_id: legacy.next_id,
        spaces: legacy.spaces,
        groups: legacy.groups,
        contexts,
    };
    if !validate(&converted) {
        return Err(OrganizationError::InvalidSnapshot);
    }
    Ok((converted, summary))
}

fn snapshot_version(bytes: &[u8]) -> Result<u64, OrganizationError> {
    let document: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| OrganizationError::InvalidSnapshot)?;
    document
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or(OrganizationError::InvalidSnapshot)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    CreateSpace {
        name: String,
    },
    RenameSpace {
        id: String,
        name: String,
    },
    DeleteEmptySpace {
        id: String,
    },
    CreateGroup {
        space_id: String,
        name: String,
    },
    RenameGroup {
        id: String,
        name: String,
    },
    DeleteEmptyGroup {
        id: String,
    },
    CreateContext {
        space_id: String,
        group_id: Option<String>,
        name: String,
    },
    RenameContext {
        id: String,
        name: String,
    },
    MoveContext {
        id: String,
        space_id: String,
        group_id: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrganizationError {
    Unavailable,
    Busy,
    InvalidSnapshot,
    InvalidInput,
    NotFound,
    DuplicateName,
    NotEmpty,
    Conflict,
    LimitExceeded,
    RevisionExhausted,
    CommitUnknown,
    /// A valid V1 store must be explicitly upgraded before ordinary writes.
    MigrationRequired,
}

#[derive(Debug)]
pub struct OrganizationStore {
    root: PathBuf,
    stem: &'static str,
}

struct Lock {
    path: PathBuf,
    file: Option<File>,
    release: bool,
}

impl Drop for Lock {
    fn drop(&mut self) {
        drop(self.file.take());
        if self.release {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn private_file(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.trim() == name
        && name.len() <= 128
        && !name.chars().any(char::is_control)
}

fn valid_id(id: &str) -> bool {
    id.len() == 16
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn take_id(snapshot: &mut Snapshot) -> Result<String, OrganizationError> {
    let current = snapshot.next_id;
    snapshot.next_id = current
        .checked_add(1)
        .ok_or(OrganizationError::RevisionExhausted)?;
    Ok(format!("{current:016x}"))
}

fn validate(snapshot: &Snapshot) -> bool {
    if snapshot.schema_version != 2
        || snapshot.next_id == 0
        || snapshot.spaces.len() > SPACE_LIMIT
        || snapshot.groups.len() > GROUP_LIMIT
        || snapshot.contexts.len() > CONTEXT_LIMIT
    {
        return false;
    }
    let mut ids = HashSet::new();
    let mut max_id = 0u64;
    for (id, name) in snapshot
        .spaces
        .iter()
        .map(|s| (&s.id, &s.name))
        .chain(snapshot.groups.iter().map(|g| (&g.id, &g.name)))
        .chain(snapshot.contexts.iter().map(|c| (&c.id, &c.name)))
    {
        if !valid_id(id) || !valid_name(name) || !ids.insert(id.as_str()) {
            return false;
        }
        let Ok(numeric) = u64::from_str_radix(id, 16) else {
            return false;
        };
        if numeric == 0 {
            return false;
        }
        max_id = max_id.max(numeric);
    }
    if snapshot.next_id <= max_id {
        return false;
    }
    for (i, space) in snapshot.spaces.iter().enumerate() {
        if snapshot.spaces[..i]
            .iter()
            .any(|prior| prior.name == space.name)
        {
            return false;
        }
    }
    for (i, group) in snapshot.groups.iter().enumerate() {
        if !snapshot
            .spaces
            .iter()
            .any(|space| space.id == group.space_id)
            || snapshot.groups[..i]
                .iter()
                .any(|prior| prior.space_id == group.space_id && prior.name == group.name)
        {
            return false;
        }
    }
    for (i, context) in snapshot.contexts.iter().enumerate() {
        if !snapshot
            .spaces
            .iter()
            .any(|space| space.id == context.space_id)
        {
            return false;
        }
        if let Some(group_id) = &context.group_id {
            if !valid_id(group_id)
                || !snapshot
                    .groups
                    .iter()
                    .any(|group| group.id == *group_id && group.space_id == context.space_id)
            {
                return false;
            }
        }
        if snapshot.contexts[..i].iter().any(|prior| {
            prior.space_id == context.space_id
                && prior.group_id == context.group_id
                && prior.name == context.name
        }) {
            return false;
        }
    }
    true
}

fn apply_change(snapshot: &mut Snapshot, change: Change) -> Result<(), OrganizationError> {
    match change {
        Change::CreateSpace { name } => {
            if !valid_name(&name) {
                return Err(OrganizationError::InvalidInput);
            }
            if snapshot.spaces.iter().any(|s| s.name == name) {
                return Err(OrganizationError::DuplicateName);
            }
            if snapshot.spaces.len() >= SPACE_LIMIT {
                return Err(OrganizationError::LimitExceeded);
            }
            let id = take_id(snapshot)?;
            snapshot.spaces.push(Space { id, name });
        }
        Change::RenameSpace { id, name } => {
            if !valid_id(&id) || !valid_name(&name) {
                return Err(OrganizationError::InvalidInput);
            }
            if snapshot.spaces.iter().any(|s| s.id != id && s.name == name) {
                return Err(OrganizationError::DuplicateName);
            }
            let space = snapshot
                .spaces
                .iter_mut()
                .find(|s| s.id == id)
                .ok_or(OrganizationError::NotFound)?;
            space.name = name;
        }
        Change::DeleteEmptySpace { id } => {
            if !valid_id(&id) {
                return Err(OrganizationError::InvalidInput);
            }
            if snapshot.groups.iter().any(|g| g.space_id == id)
                || snapshot.contexts.iter().any(|c| c.space_id == id)
            {
                return Err(OrganizationError::NotEmpty);
            }
            let before = snapshot.spaces.len();
            snapshot.spaces.retain(|s| s.id != id);
            if before == snapshot.spaces.len() {
                return Err(OrganizationError::NotFound);
            }
        }
        Change::CreateGroup { space_id, name } => {
            if !valid_id(&space_id) || !valid_name(&name) {
                return Err(OrganizationError::InvalidInput);
            }
            if !snapshot.spaces.iter().any(|s| s.id == space_id) {
                return Err(OrganizationError::NotFound);
            }
            if snapshot
                .groups
                .iter()
                .any(|g| g.space_id == space_id && g.name == name)
            {
                return Err(OrganizationError::DuplicateName);
            }
            if snapshot.groups.len() >= GROUP_LIMIT {
                return Err(OrganizationError::LimitExceeded);
            }
            let id = take_id(snapshot)?;
            snapshot.groups.push(DirectoryGroup { id, space_id, name });
        }
        Change::RenameGroup { id, name } => {
            if !valid_id(&id) || !valid_name(&name) {
                return Err(OrganizationError::InvalidInput);
            }
            let space_id = snapshot
                .groups
                .iter()
                .find(|g| g.id == id)
                .ok_or(OrganizationError::NotFound)?
                .space_id
                .clone();
            if snapshot
                .groups
                .iter()
                .any(|g| g.id != id && g.space_id == space_id && g.name == name)
            {
                return Err(OrganizationError::DuplicateName);
            }
            let group = snapshot
                .groups
                .iter_mut()
                .find(|g| g.id == id)
                .ok_or(OrganizationError::NotFound)?;
            group.name = name;
        }
        Change::DeleteEmptyGroup { id } => {
            if !valid_id(&id) {
                return Err(OrganizationError::InvalidInput);
            }
            if snapshot
                .contexts
                .iter()
                .any(|c| c.group_id.as_deref() == Some(id.as_str()))
            {
                return Err(OrganizationError::NotEmpty);
            }
            let before = snapshot.groups.len();
            snapshot.groups.retain(|g| g.id != id);
            if before == snapshot.groups.len() {
                return Err(OrganizationError::NotFound);
            }
        }
        Change::CreateContext {
            space_id,
            group_id,
            name,
        } => {
            if !valid_id(&space_id)
                || group_id.as_ref().is_some_and(|id| !valid_id(id))
                || !valid_name(&name)
            {
                return Err(OrganizationError::InvalidInput);
            }
            if !snapshot.spaces.iter().any(|space| space.id == space_id) {
                return Err(OrganizationError::NotFound);
            }
            if let Some(id) = &group_id {
                if !snapshot
                    .groups
                    .iter()
                    .any(|group| group.id == *id && group.space_id == space_id)
                {
                    return Err(OrganizationError::InvalidInput);
                }
            }
            if snapshot.contexts.iter().any(|context| {
                context.space_id == space_id && context.group_id == group_id && context.name == name
            }) {
                return Err(OrganizationError::DuplicateName);
            }
            if snapshot.contexts.len() >= CONTEXT_LIMIT {
                return Err(OrganizationError::LimitExceeded);
            }
            let id = take_id(snapshot)?;
            snapshot.contexts.push(ContextRecord {
                id,
                space_id,
                group_id,
                name,
            });
        }
        Change::RenameContext { id, name } => {
            if !valid_id(&id) || !valid_name(&name) {
                return Err(OrganizationError::InvalidInput);
            }
            let current = snapshot
                .contexts
                .iter()
                .find(|context| context.id == id)
                .ok_or(OrganizationError::NotFound)?;
            let space_id = current.space_id.clone();
            let group_id = current.group_id.clone();
            if snapshot.contexts.iter().any(|context| {
                context.id != id
                    && context.space_id == space_id
                    && context.group_id == group_id
                    && context.name == name
            }) {
                return Err(OrganizationError::DuplicateName);
            }
            let context = snapshot
                .contexts
                .iter_mut()
                .find(|context| context.id == id)
                .ok_or(OrganizationError::NotFound)?;
            context.name = name;
        }
        Change::MoveContext {
            id,
            space_id,
            group_id,
        } => {
            if !valid_id(&id)
                || !valid_id(&space_id)
                || group_id.as_ref().is_some_and(|group| !valid_id(group))
            {
                return Err(OrganizationError::InvalidInput);
            }
            if !snapshot.spaces.iter().any(|space| space.id == space_id) {
                return Err(OrganizationError::NotFound);
            }
            if let Some(group) = &group_id {
                if !snapshot
                    .groups
                    .iter()
                    .any(|existing| existing.id == *group && existing.space_id == space_id)
                {
                    return Err(OrganizationError::InvalidInput);
                }
            }
            let name = snapshot
                .contexts
                .iter()
                .find(|context| context.id == id)
                .ok_or(OrganizationError::NotFound)?
                .name
                .clone();
            if snapshot.contexts.iter().any(|context| {
                context.id != id
                    && context.space_id == space_id
                    && context.group_id == group_id
                    && context.name == name
            }) {
                return Err(OrganizationError::DuplicateName);
            }
            let context = snapshot
                .contexts
                .iter_mut()
                .find(|context| context.id == id)
                .ok_or(OrganizationError::NotFound)?;
            context.space_id = space_id;
            context.group_id = group_id;
        }
    }
    Ok(())
}

impl OrganizationStore {
    /// Root MUST be an out-of-band trusted Host app-private path, not Space,
    /// Address or remote request data. Unix private-ACL implementation only.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, OrganizationError> {
        Self::open_named(root, "workspace")
    }

    fn open_named(root: impl AsRef<Path>, stem: &'static str) -> Result<Self, OrganizationError> {
        let root = root.as_ref();
        let text = root.to_str().ok_or(OrganizationError::Unavailable)?;
        if !cfg!(unix)
            || !root.is_absolute()
            || text.len() > 4096
            || text.contains('\0')
            || text
                .split('/')
                .skip(1)
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(OrganizationError::Unavailable);
        }
        // The mode is Unix-only; Windows inherits the parent ACL.
        #[cfg_attr(not(unix), allow(unused_mut))]
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        match builder.create(root) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(OrganizationError::Unavailable),
        }
        let store = Self {
            root: root.to_path_buf(),
            stem,
        };
        store.verify_root()?;
        Ok(store)
    }

    fn verify_root(&self) -> Result<(), OrganizationError> {
        let meta = fs::symlink_metadata(&self.root).map_err(|_| OrganizationError::Unavailable)?;
        if !meta.is_dir() || meta.file_type().is_symlink() {
            return Err(OrganizationError::Unavailable);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if meta.permissions().mode() & 0o077 != 0 {
                return Err(OrganizationError::Unavailable);
            }
        }
        Ok(())
    }

    fn lock(&self) -> Result<Lock, OrganizationError> {
        self.verify_root()?;
        let path = self.root.join(format!("{}.lock", self.stem));
        let file = private_file(&path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                OrganizationError::Busy
            } else {
                OrganizationError::Unavailable
            }
        })?;
        Ok(Lock {
            path,
            file: Some(file),
            release: true,
        })
    }

    fn read_bytes_locked(&self) -> Result<Option<Vec<u8>>, OrganizationError> {
        match fs::symlink_metadata(self.root.join(format!("{}.tmp", self.stem))) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            _ => return Err(OrganizationError::Unavailable),
        }
        let path = self.root.join(format!("{}.json", self.stem));
        let meta = match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Ok(meta) => meta,
            Err(_) => return Err(OrganizationError::Unavailable),
        };
        if !meta.is_file() || meta.file_type().is_symlink() {
            return Err(OrganizationError::InvalidSnapshot);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if meta.permissions().mode() & 0o077 != 0 {
                return Err(OrganizationError::InvalidSnapshot);
            }
        }
        let file = File::open(path).map_err(|_| OrganizationError::Unavailable)?;
        let mut bytes = Vec::new();
        file.take(SNAPSHOT_LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| OrganizationError::Unavailable)?;
        if bytes.len() as u64 > SNAPSHOT_LIMIT {
            return Err(OrganizationError::InvalidSnapshot);
        }
        Ok(Some(bytes))
    }

    fn read_locked(&self) -> Result<Snapshot, OrganizationError> {
        let Some(bytes) = self.read_bytes_locked()? else {
            return Ok(Snapshot::default());
        };
        match snapshot_version(&bytes)? {
            1 => {
                // Validate V1 before reporting it as upgradeable; never auto-upgrade.
                let _ = decode_legacy(&bytes)?;
                Err(OrganizationError::MigrationRequired)
            }
            2 => {
                let snapshot: Snapshot = serde_json::from_slice(&bytes)
                    .map_err(|_| OrganizationError::InvalidSnapshot)?;
                if !validate(&snapshot) {
                    return Err(OrganizationError::InvalidSnapshot);
                }
                Ok(snapshot)
            }
            _ => Err(OrganizationError::InvalidSnapshot),
        }
    }

    fn save_locked(&self, snapshot: &Snapshot) -> Result<(), OrganizationError> {
        let bytes = serde_json::to_vec(snapshot).map_err(|_| OrganizationError::Unavailable)?;
        self.save_bytes_locked(&bytes)
    }

    fn save_bytes_locked(&self, bytes: &[u8]) -> Result<(), OrganizationError> {
        if bytes.len() as u64 > SNAPSHOT_LIMIT {
            return Err(OrganizationError::LimitExceeded);
        }
        let temporary = self.root.join(format!("{}.tmp", self.stem));
        let mut file = private_file(&temporary).map_err(|_| OrganizationError::Unavailable)?;
        file.write_all(bytes)
            .map_err(|_| OrganizationError::Unavailable)?;
        file.sync_all()
            .map_err(|_| OrganizationError::Unavailable)?;
        drop(file);
        fs::rename(&temporary, self.root.join(format!("{}.json", self.stem)))
            .map_err(|_| OrganizationError::Unavailable)?;
        File::open(&self.root)
            .and_then(|dir| dir.sync_all())
            .map_err(|_| OrganizationError::CommitUnknown)
    }

    /// Reports validated legacy state without creating categories or touching disk.
    /// The caller must obtain explicit user consent before calling migrate_v1.
    pub fn legacy_summary(&self) -> Result<Option<LegacySummary>, OrganizationError> {
        let _lock = self.lock()?;
        let Some(bytes) = self.read_bytes_locked()? else {
            return Ok(None);
        };
        match snapshot_version(&bytes)? {
            1 => Ok(Some(decode_legacy(&bytes)?.1)),
            2 => {
                // Reject malformed V2 rather than misreporting "no migration".
                let _ = self.read_locked()?;
                Ok(None)
            }
            _ => Err(OrganizationError::InvalidSnapshot),
        }
    }

    /// Explicit V1 -> V2 conversion. Preserves IDs and legacy container labels;
    /// never guesses "personal/work/public" or changes any access grants.
    /// The original complete V1 bytes are retained in a private .v1.bak file.
    pub fn migrate_v1(
        &self,
        expected_revision: u64,
    ) -> Result<OrganizationView, OrganizationError> {
        let mut lock = self.lock()?;
        let bytes = self
            .read_bytes_locked()?
            .ok_or(OrganizationError::InvalidInput)?;
        if snapshot_version(&bytes)? != 1 {
            return Err(OrganizationError::InvalidInput);
        }
        let (mut snapshot, legacy) = decode_legacy(&bytes)?;
        if legacy.revision != expected_revision {
            return Err(OrganizationError::Conflict);
        }
        snapshot.revision = snapshot
            .revision
            .checked_add(1)
            .ok_or(OrganizationError::RevisionExhausted)?;
        let encoded = serde_json::to_vec(&snapshot).map_err(|_| OrganizationError::Unavailable)?;
        if encoded.len() as u64 > SNAPSHOT_LIMIT {
            return Err(OrganizationError::LimitExceeded);
        }
        // Never overwrite an earlier backup after an interrupted attempt.
        let backup_path = self.root.join("workspace.v1.bak");
        let mut backup = private_file(&backup_path).map_err(|_| OrganizationError::Unavailable)?;
        let result = (|| {
            backup
                .write_all(&bytes)
                .and_then(|()| backup.sync_all())
                .map_err(|_| OrganizationError::Unavailable)?;
            File::open(&self.root)
                .and_then(|dir| dir.sync_all())
                .map_err(|_| OrganizationError::CommitUnknown)?;
            self.save_locked(&snapshot)?;
            Ok(OrganizationView::from(&snapshot))
        })();
        if result.is_err() {
            // A partial backup or unknown commit requires trusted review.
            lock.release = false;
        }
        result
    }

    /// Read-only, typed management projection. Never expose storage JSON.
    pub fn load(&self) -> Result<OrganizationView, OrganizationError> {
        let _lock = self.lock()?;
        Ok(OrganizationView::from(&self.read_locked()?))
    }

    /// Fetch visible organization records within one local container.
    /// Caller/Host still MUST authorize who may see these records; no public sharing.
    pub fn contexts_in_space(
        &self,
        space_id: &str,
    ) -> Result<Vec<ContextRecord>, OrganizationError> {
        if !valid_id(space_id) {
            return Err(OrganizationError::InvalidInput);
        }
        let _lock = self.lock()?;
        let snapshot = self.read_locked()?;
        if !snapshot.spaces.iter().any(|space| space.id == space_id) {
            return Err(OrganizationError::NotFound);
        }
        Ok(snapshot
            .contexts
            .into_iter()
            .filter(|context| context.space_id == space_id)
            .collect())
    }

    /// Expected revision CAS across processes; reloads authoritative disk state.
    /// Organization changes do not touch files, AI grants, shares or execution.
    pub fn apply(
        &self,
        expected_revision: u64,
        change: Change,
    ) -> Result<OrganizationView, OrganizationError> {
        let mut lock = self.lock()?;
        let mut snapshot = self.read_locked()?;
        if snapshot.revision != expected_revision {
            return Err(OrganizationError::Conflict);
        }
        apply_change(&mut snapshot, change)?;
        snapshot.revision = snapshot
            .revision
            .checked_add(1)
            .ok_or(OrganizationError::RevisionExhausted)?;
        if !validate(&snapshot) {
            return Err(OrganizationError::InvalidSnapshot);
        }
        match self.save_locked(&snapshot) {
            Ok(()) => Ok(OrganizationView::from(&snapshot)),
            Err(error) => {
                // Do not automatically retry or delete potentially forensic state.
                lock.release = false;
                Err(error)
            }
        }
    }
}
