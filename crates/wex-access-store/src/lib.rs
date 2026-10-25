//! Private, bounded authorization snapshots and bound-resource preflight.
//! Bounded authorized direct-file reads; no write execution API.
//! Only a trusted user-facing authorization service may mutate this store.
mod read_session;
pub use read_session::ReadSession;

use serde::{Deserialize, Serialize};

mod platform;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use wex_core::access::{decide_local_access, Access, Decision, DirectoryGrant, LocalTarget};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisteredAccessError {
    Store(StoreError),
    Resource(wex_local_resources::ResourceError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisteredReadError {
    Store(StoreError),
    File(wex_local_resources::FileReadError),
}

const MAX_BYTES: u64 = 65536;
const MAX_GRANTS: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    pub directory_id: String,
    pub access: Access,
    pub explicit_personal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub revision: u64,
    pub grants: Vec<Grant>,
    pub directories: Vec<wex_local_resources::SavedDirectory>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreError {
    Unavailable,
    Busy,
    InvalidSnapshot,
    InvalidGrant,
    Conflict,
    LimitExceeded,
    RevisionExhausted,
    /// Replacement happened but durability could not be confirmed. The store
    /// stays quarantined; trusted recovery must inspect the committed state.
    /// Never automatically repeat a mutation whose result is unknown.
    CommitUnknown,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum WireAccess {
    Read,
    Write,
    ReadWrite,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireGrant {
    directory_id: String,
    access: WireAccess,
    explicit_personal: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireDirectory {
    id: String,
    path: PathBuf,
    protected: bool,
    platform: String,
    stamp: [u64; 4],
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireSnapshot {
    schema_version: u32,
    revision: u64,
    grants: Vec<WireGrant>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    directories: Option<Vec<WireDirectory>>,
}

/// A private root supplied by the platform, never a Space or AI-selected path.
/// Directory identity and personal classification must come from a trusted
/// resource resolver. This store does not resolve filesystem targets.
#[derive(Debug)]
pub struct Store {
    root: PathBuf,
    internal_roots: Vec<PathBuf>,
}

struct Lock {
    file: Option<File>,
    path: PathBuf,
    remove_on_drop: bool,
}
impl Drop for Lock {
    fn drop(&mut self) {
        drop(self.file.take());
        if self.remove_on_drop {
            let _ = fs::remove_file(&self.path);
        }
    }
}
struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn private_create(path: &Path) -> std::io::Result<File> {
    platform::create_private_file(path)
}

impl Store {
    pub fn open(root: impl AsRef<Path>) -> Result<Self, StoreError> {
        Self::open_with_internal_roots(root, &[])
    }

    /// Compose trusted Host deny roots, never paths from an AI or Space.
    /// The authority root is always included; at most 15 additional roots fit.
    /// Existence is checked on each authorized access, not cached at creation.
    /// Configuration is process-local and must be supplied on every reopen.
    pub fn open_with_internal_roots(
        root: impl AsRef<Path>,
        internal_roots: &[&Path],
    ) -> Result<Self, StoreError> {
        if internal_roots.len() > 15 {
            return Err(StoreError::LimitExceeded);
        }
        for path in internal_roots {
            let text = path.to_str().ok_or(StoreError::Unavailable)?;
            if !path.is_absolute() || text.len() > 4096 || text.contains('\0') {
                return Err(StoreError::Unavailable);
            }
        }
        if !platform::supported() {
            return Err(StoreError::Unavailable);
        }
        let root = root.as_ref();
        if !root.is_absolute() {
            return Err(StoreError::Unavailable);
        }
        platform::create_private_dir(root).map_err(|_| StoreError::Unavailable)?;
        let store = Self {
            root: root.to_path_buf(),
            internal_roots: std::iter::once(root.to_path_buf())
                .chain(internal_roots.iter().map(|path| path.to_path_buf()))
                .collect(),
        };
        store.verify_root()?;
        Ok(store)
    }

    fn verify_root(&self) -> Result<(), StoreError> {
        let metadata = fs::symlink_metadata(&self.root).map_err(|_| StoreError::Unavailable)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(StoreError::Unavailable);
        }
        if !platform::is_private(&self.root) {
            return Err(StoreError::Unavailable);
        }
        Ok(())
    }

    fn lock(&self) -> Result<Lock, StoreError> {
        self.verify_root()?;
        let path = self.root.join("grants.lock");
        match private_create(&path) {
            Ok(file) => Ok(Lock {
                file: Some(file),
                path,
                remove_on_drop: true,
            }),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                Err(StoreError::Busy)
            }
            Err(_) => Err(StoreError::Unavailable),
        }
    }

    fn read_locked(&self) -> Result<Snapshot, StoreError> {
        // An orphan temporary snapshot may be an interrupted revocation.
        // Never silently load an older grant set while it is present.
        match fs::symlink_metadata(self.root.join("grants.tmp")) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            _ => return Err(StoreError::Unavailable),
        }
        let path = self.root.join("grants.json");
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Snapshot {
                    revision: 0,
                    grants: Vec::new(),
                    directories: Vec::new(),
                });
            }
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {}
            _ => return Err(StoreError::Unavailable),
        }
        let file = File::open(path).map_err(|_| StoreError::Unavailable)?;
        let mut bytes = Vec::new();
        file.take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| StoreError::Unavailable)?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(StoreError::InvalidSnapshot);
        }
        let wire: WireSnapshot =
            serde_json::from_slice(&bytes).map_err(|_| StoreError::InvalidSnapshot)?;
        if !matches!(wire.schema_version, 1 | 2)
            || wire.grants.len() > MAX_GRANTS
            || (wire.schema_version == 2 && wire.directories.is_none())
            || (wire.schema_version == 1 && wire.directories.is_some())
        {
            return Err(StoreError::InvalidSnapshot);
        }
        let mut grants: Vec<Grant> = Vec::new();
        for entry in wire.grants {
            if !valid_id(&entry.directory_id)
                || grants.iter().any(|g| g.directory_id == entry.directory_id)
            {
                return Err(StoreError::InvalidSnapshot);
            }
            grants.push(Grant {
                directory_id: entry.directory_id,
                access: match entry.access {
                    WireAccess::Read => Access::Read,
                    WireAccess::Write => Access::Write,
                    WireAccess::ReadWrite => Access::ReadWrite,
                },
                explicit_personal: entry.explicit_personal,
            });
        }
        let mut directories: Vec<wex_local_resources::SavedDirectory> = Vec::new();
        for d in wire.directories.unwrap_or_default() {
            if directories.len() >= MAX_GRANTS
                || d.id.len() != 64
                || !valid_id(&d.id)
                || !d.path.is_absolute()
                || d.path
                    .to_str()
                    .is_none_or(|p| p.len() > 4096 || p.contains('\0'))
                || d.platform.len() > 32
                || d.stamp[3] >= 1_000_000_000
                || !grants.iter().any(|g| g.directory_id == d.id)
                || directories.iter().any(|b| b.id == d.id || b.path == d.path)
            {
                return Err(StoreError::InvalidSnapshot);
            }
            directories.push(wex_local_resources::SavedDirectory {
                id: d.id,
                path: d.path,
                protection: if d.protected {
                    wex_local_resources::Protection::Protected
                } else {
                    wex_local_resources::Protection::General
                },
                platform: d.platform,
                stamp: d.stamp,
            });
        }
        Ok(Snapshot {
            revision: wire.revision,
            grants,
            directories,
        })
    }

    fn encode_snapshot(state: &Snapshot) -> Result<Vec<u8>, StoreError> {
        let wire = WireSnapshot {
            schema_version: 2,
            directories: Some(
                state
                    .directories
                    .iter()
                    .map(|d| WireDirectory {
                        id: d.id.clone(),
                        path: d.path.clone(),
                        protected: d.protection == wex_local_resources::Protection::Protected,
                        platform: d.platform.clone(),
                        stamp: d.stamp,
                    })
                    .collect(),
            ),
            revision: state.revision,
            grants: state
                .grants
                .iter()
                .map(|g| WireGrant {
                    directory_id: g.directory_id.clone(),
                    access: match g.access {
                        Access::Read => WireAccess::Read,
                        Access::Write => WireAccess::Write,
                        Access::ReadWrite => WireAccess::ReadWrite,
                    },
                    explicit_personal: g.explicit_personal,
                })
                .collect(),
        };
        let bytes = serde_json::to_vec(&wire).map_err(|_| StoreError::Unavailable)?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(StoreError::LimitExceeded);
        }
        Ok(bytes)
    }

    fn save_locked(&self, state: &Snapshot) -> Result<(), StoreError> {
        let bytes = Self::encode_snapshot(state)?;
        let path = self.root.join("grants.tmp");
        // A pre-existing temporary file is never removed or restored here.
        let mut file = private_create(&path).map_err(|_| StoreError::Unavailable)?;
        let temporary = Temporary(path);
        file.write_all(&bytes)
            .map_err(|_| StoreError::Unavailable)?;
        file.sync_all().map_err(|_| StoreError::Unavailable)?;
        drop(file);
        // Never delete the old snapshot to emulate replacement on a platform
        // where atomic replacement is unavailable.
        platform::replace_file(&temporary.0, &self.root.join("grants.json"))
            .map_err(|_| StoreError::Unavailable)?;
        #[cfg(unix)]
        File::open(&self.root)
            .and_then(|dir| dir.sync_all())
            .map_err(|_| StoreError::CommitUnknown)?;
        Ok(())
    }

    fn commit(&self, state: &Snapshot, mut lock: Lock) -> Result<u64, StoreError> {
        match self.save_locked(state) {
            Ok(()) => Ok(state.revision),
            Err(error) => {
                // A failed revoke/downgrade must not leave usable old authority.
                // Keep the marker until a trusted recovery flow inspects state.
                lock.remove_on_drop = false;
                Err(error)
            }
        }
    }

    pub fn load(&self) -> Result<Snapshot, StoreError> {
        let _lock = self.lock()?;
        self.read_locked()
    }

    /// Replace the complete grant for this ID, including personal consent.
    /// Call only after an explicit user edit; never expose this as an AI tool.
    pub fn set(&self, expected_revision: u64, grant: Grant) -> Result<u64, StoreError> {
        if !valid_id(&grant.directory_id) {
            return Err(StoreError::InvalidGrant);
        }
        let mut lock = self.lock()?;
        let mut state = self.read_locked()?;
        if state.revision != expected_revision {
            return Err(StoreError::Conflict);
        }
        if let Some(existing) = state
            .grants
            .iter_mut()
            .find(|g| g.directory_id == grant.directory_id)
        {
            *existing = grant;
        } else {
            if state.grants.len() >= MAX_GRANTS {
                return Err(StoreError::LimitExceeded);
            }
            state.grants.push(grant);
        }
        state.revision = match state.revision.checked_add(1) {
            Some(revision) => revision,
            None => {
                lock.remove_on_drop = false;
                return Err(StoreError::RevisionExhausted);
            }
        };
        self.commit(&state, lock)
    }

    pub fn revoke(&self, expected_revision: u64, directory_id: &str) -> Result<u64, StoreError> {
        if !valid_id(directory_id) {
            return Err(StoreError::InvalidGrant);
        }
        let mut lock = self.lock()?;
        let mut state = self.read_locked()?;
        if state.revision != expected_revision {
            return Err(StoreError::Conflict);
        }
        let old_len = state.grants.len();
        state.grants.retain(|g| g.directory_id != directory_id);
        state.directories.retain(|d| d.id != directory_id);
        if old_len == state.grants.len() {
            return Ok(state.revision);
        }
        state.revision = match state.revision.checked_add(1) {
            Some(revision) => revision,
            None => {
                lock.remove_on_drop = false;
                return Err(StoreError::RevisionExhausted);
            }
        };
        self.commit(&state, lock)
    }

    /// Atomic trusted native user selection plus explicit scope grant.
    /// Never expose path-based authorization as an AI tool.
    pub fn authorize_directory(
        &self,
        expected_revision: u64,
        path: impl AsRef<Path>,
        access: Access,
    ) -> Result<wex_local_resources::SavedDirectory, StoreError> {
        let lock = self.lock()?;
        let mut state = self.read_locked()?;
        if state.revision != expected_revision {
            return Err(StoreError::Conflict);
        }
        if state.grants.len() >= MAX_GRANTS {
            return Err(StoreError::LimitExceeded);
        }
        let roots: Vec<_> = self.internal_roots.iter().map(PathBuf::as_path).collect();
        let mut registry = wex_local_resources::DirectoryRegistry::new();
        let id = registry
            .register_with_exclusions(path, wex_local_resources::Protection::Protected, &roots)
            .map_err(|_| StoreError::InvalidGrant)?;
        let saved = registry
            .export_binding(&id)
            .map_err(|_| StoreError::InvalidGrant)?;
        if state
            .directories
            .iter()
            .any(|b| b.path == saved.path || b.stamp[..2] == saved.stamp[..2])
        {
            return Err(StoreError::InvalidGrant);
        }
        state.grants.push(Grant {
            directory_id: id,
            access,
            explicit_personal: true,
        });
        state.directories.push(saved.clone());
        state.revision = state
            .revision
            .checked_add(1)
            .ok_or(StoreError::RevisionExhausted)?;
        // Adding a new scope has not reduced existing authority or written any
        // bytes. Reject capacity/encoding errors before entering the commit
        // quarantine path, just like the pre-existing grant-count bound.
        Self::encode_snapshot(&state)?;
        self.commit(&state, lock)?;
        Ok(saved)
    }

    /// Restore the selected native object and hold current grants through I/O.
    /// Only a direct regular leaf is read; nested scopes cannot be traversed.
    /// Unrelated missing directories do not revoke this directory.
    pub fn read_authorized(
        &self,
        directory_id: &str,
        leaf: &str,
        system_access: bool,
    ) -> Result<Vec<u8>, RegisteredReadError> {
        self.read_authorized_at(None, directory_id, leaf, system_access)
    }

    /// An expected revision binds a reviewed request to its unchanged grant set.
    pub fn read_authorized_at(
        &self,
        expected_revision: Option<u64>,
        directory_id: &str,
        leaf: &str,
        system_access: bool,
    ) -> Result<Vec<u8>, RegisteredReadError> {
        self.with_snapshot(|state, grants| {
            if expected_revision.is_some_and(|r| r != state.revision) {
                return Err(wex_local_resources::FileReadError::Changed);
            }
            let saved = state
                .directories
                .iter()
                .find(|d| d.id == directory_id)
                .ok_or(wex_local_resources::FileReadError::Resource(
                    wex_local_resources::ResourceError::UnknownDirectory,
                ))?;
            let decision = decide_local_access(
                LocalTarget {
                    directory_id,
                    personal: saved.protection == wex_local_resources::Protection::Protected,
                    system_access,
                },
                Access::Read,
                grants,
            );
            if let Decision::Denied(reason) = decision {
                return Err(wex_local_resources::FileReadError::Denied(reason));
            }
            // Direct-leaf reads cannot enter a nested registered directory.
            // Each explicitly selected root has its own exact grant. Do not
            // couple parent/child or unrelated directory availability here.
            let roots: Vec<_> = self.internal_roots.iter().map(PathBuf::as_path).collect();
            let mut registry = wex_local_resources::DirectoryRegistry::new();
            registry
                .restore_binding(saved, &roots)
                .map_err(wex_local_resources::FileReadError::Resource)?;
            registry.read_file_with_exclusions(directory_id, leaf, system_access, grants, &roots)
        })
    }

    /// Read-only validation of a proposed existing-file update under the live
    /// persistent grant lock. A successful result is NOT an executable token:
    /// commit must re-open the same root and re-check the version atomically.
    pub fn prepare_authorized_replace_at(
        &self, expected_revision: Option<u64>, directory_id: &str, leaf: &str,
        expected_bytes: &[u8], system_access: bool,
    ) -> Result<(), RegisteredReadError> {
        self.with_snapshot(|state, grants| {
            if expected_revision.is_some_and(|r| r != state.revision) {
                return Err(wex_local_resources::FileReadError::Changed);
            }
            let saved = state.directories.iter().find(|d| d.id == directory_id)
                .ok_or(wex_local_resources::FileReadError::Resource(
                    wex_local_resources::ResourceError::UnknownDirectory))?;
            let roots: Vec<_> = self.internal_roots.iter().map(PathBuf::as_path).collect();
            let mut registry = wex_local_resources::DirectoryRegistry::new();
            registry.restore_binding(saved, &roots)
                .map_err(wex_local_resources::FileReadError::Resource)?;
            registry.prepare_file_replace_with_exclusions(
                directory_id, leaf, expected_bytes, system_access, grants, &roots)
        })
    }

    /// Exclusive creation under current per-conversation write authority.
    /// Authorization, directory identity and file I/O share the store lock.
    /// A failed create can leave a partial file; consumers must not auto-retry.
    pub fn create_authorized_at(
        &self,
        expected_revision: Option<u64>,
        directory_id: &str,
        leaf: &str,
        bytes: &[u8],
        system_access: bool,
    ) -> Result<(), RegisteredReadError> {
        self.with_snapshot(|state, grants| {
            if expected_revision.is_some_and(|r| r != state.revision) {
                return Err(wex_local_resources::FileReadError::Changed);
            }
            let saved = state.directories.iter().find(|d| d.id == directory_id)
                .ok_or(wex_local_resources::FileReadError::Resource(
                    wex_local_resources::ResourceError::UnknownDirectory))?;
            let roots: Vec<_> = self.internal_roots.iter().map(PathBuf::as_path).collect();
            let mut registry = wex_local_resources::DirectoryRegistry::new();
            registry.restore_binding(saved, &roots)
                .map_err(wex_local_resources::FileReadError::Resource)?;
            registry.create_file_with_exclusions(
                directory_id, leaf, bytes, system_access, grants, &roots,
            )
        })
    }

    /// Hold the live grant transaction until the read returns.
    pub fn read_registered(
        &self,
        registry: &wex_local_resources::DirectoryRegistry,
        directory_id: &str,
        leaf: &str,
        system_access: bool,
    ) -> Result<Vec<u8>, RegisteredReadError> {
        self.with_current_grants(|grants| {
            registry.read_file_with_exclusions(
                directory_id,
                leaf,
                system_access,
                grants,
                &self
                    .internal_roots
                    .iter()
                    .map(PathBuf::as_path)
                    .collect::<Vec<_>>(),
            )
        })
    }

    fn with_current_grants<T>(
        &self,
        action: impl FnOnce(&[DirectoryGrant<'_>]) -> Result<T, wex_local_resources::FileReadError>,
    ) -> Result<T, RegisteredReadError> {
        self.with_snapshot(|_, grants| action(grants))
    }

    fn with_snapshot<T>(
        &self,
        action: impl FnOnce(
            &Snapshot,
            &[DirectoryGrant<'_>],
        ) -> Result<T, wex_local_resources::FileReadError>,
    ) -> Result<T, RegisteredReadError> {
        let _lock = self.lock().map_err(RegisteredReadError::Store)?;
        let state = self.read_locked().map_err(RegisteredReadError::Store)?;
        let grants: Vec<_> = state
            .grants
            .iter()
            .map(|g| DirectoryGrant {
                directory_id: &g.directory_id,
                access: g.access,
                explicit_personal: g.explicit_personal,
            })
            .collect();
        action(&state, &grants).map_err(RegisteredReadError::File)
    }

    /// Resolve current metadata and trusted personal classification, then apply
    /// fresh stored grants. Never use the result as a later file-I/O token.
    pub fn check_registered(
        &self,
        registry: &wex_local_resources::DirectoryRegistry,
        directory_id: &str,
        relative: &str,
        system_access: bool,
        requested: Access,
    ) -> Result<Decision, RegisteredAccessError> {
        let state = self.load().map_err(RegisteredAccessError::Store)?;
        let grants: Vec<_> = state
            .grants
            .iter()
            .map(|g| DirectoryGrant {
                directory_id: &g.directory_id,
                access: g.access,
                explicit_personal: g.explicit_personal,
            })
            .collect();
        registry
            .check_with_exclusions(
                directory_id,
                relative,
                system_access,
                requested,
                &grants,
                &self
                    .internal_roots
                    .iter()
                    .map(PathBuf::as_path)
                    .collect::<Vec<_>>(),
            )
            .map_err(RegisteredAccessError::Resource)
    }

    /// Read current authority every time. Err is a denial, never a fallback
    /// to cached grants. Allowed is not a durable token for later file I/O.
    pub fn check(
        &self,
        target: LocalTarget<'_>,
        requested: Access,
    ) -> Result<Decision, StoreError> {
        let state = self.load()?;
        let grants: Vec<_> = state
            .grants
            .iter()
            .map(|g| DirectoryGrant {
                directory_id: &g.directory_id,
                access: g.access,
                explicit_personal: g.explicit_personal,
            })
            .collect();
        Ok(decide_local_access(target, requested, &grants))
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn grant_read_transaction_serializes_edits_and_releases_on_error() {
        for fail in [false, true] {
            let root =
                std::env::temp_dir().join(format!("wex-read-lock-{}-{fail}", std::process::id()));
            let first = Store::open(&root).unwrap();
            first
                .set(
                    0,
                    Grant {
                        directory_id: "docs".into(),
                        access: Access::Read,
                        explicit_personal: true,
                    },
                )
                .unwrap();
            let second = Store::open(&root).unwrap();
            let result = first.with_current_grants(|grants| {
                assert_eq!(grants.len(), 1);
                assert_eq!(second.revoke(1, "docs"), Err(StoreError::Busy));
                if fail {
                    Err(wex_local_resources::FileReadError::TooLarge)
                } else {
                    Ok(Vec::<u8>::new())
                }
            });
            if fail {
                assert_eq!(
                    result,
                    Err(RegisteredReadError::File(
                        wex_local_resources::FileReadError::TooLarge
                    ))
                );
            } else {
                assert_eq!(result, Ok(Vec::new()));
            }
            assert_eq!(second.revoke(1, "docs"), Ok(2));
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn actual_save_failure_keeps_store_quarantined_after_reopen() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "wex-commit-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let store = Store::open(&root).unwrap();
        store
            .set(
                0,
                Grant {
                    directory_id: "docs".into(),
                    access: Access::ReadWrite,
                    explicit_personal: false,
                },
            )
            .unwrap();
        let lock = store.lock().unwrap();
        let mut state = store.read_locked().unwrap();
        state.grants.clear();
        state.revision += 1;
        // Real filesystem failure between transaction read and commit.
        fs::write(root.join("grants.tmp"), b"interrupted writer").unwrap();
        assert_eq!(store.commit(&state, lock), Err(StoreError::Unavailable));
        fs::remove_file(root.join("grants.tmp")).unwrap();
        assert_eq!(Store::open(&root).unwrap().load(), Err(StoreError::Busy));
        let _ = fs::remove_dir_all(root);
    }
}
