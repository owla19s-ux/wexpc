//! Trusted in-process composition for bounded local reads, not an AI tool.
use crate::{RegisteredReadError, Store, StoreError};
use std::path::Path;
use wex_local_resources::{DirectoryRegistry, Protection, ResourceError};

/// Owns live directory identities and a fixed Host deny policy.
/// Dropping the session releases all bindings; reopening generates fresh IDs.
/// Authority remains solely in Store and is reloaded on every read.
pub struct ReadSession {
    store: Store,
    registry: DirectoryRegistry,
}

impl ReadSession {
    /// Host must supply at least one private data root, never an AI/Space path.
    /// A missing configuration cannot silently select the lower-level default.
    pub fn open(
        authority_root: impl AsRef<Path>,
        internal_roots: &[&Path],
    ) -> Result<Self, StoreError> {
        if internal_roots.is_empty() || !cfg!(any(target_os = "linux", target_os = "android")) {
            return Err(StoreError::Unavailable);
        }
        let store = Store::open_with_internal_roots(authority_root, internal_roots)?;
        Ok(Self {
            store,
            registry: DirectoryRegistry::new(),
        })
    }

    /// Trusted explicit user selection only. Default personal protection cannot
    /// be disabled here; registration neither grants access nor reads contents.
    pub fn register(&mut self, path: impl AsRef<Path>) -> Result<String, ResourceError> {
        let roots: Vec<_> = self
            .store
            .internal_roots
            .iter()
            .map(|root| root.as_path())
            .collect();
        self.registry
            .register_with_exclusions(path, Protection::Protected, &roots)
    }

    /// Host supplies current system access on every call; never trust an
    /// AI-supplied permission flag. Grant edits take effect on the next read.
    /// This synchronous I/O belongs on a worker, not the UI thread.
    pub fn read(
        &self,
        directory_id: &str,
        leaf: &str,
        system_access: bool,
    ) -> Result<Vec<u8>, RegisteredReadError> {
        self.store
            .read_registered(&self.registry, directory_id, leaf, system_access)
    }
}
