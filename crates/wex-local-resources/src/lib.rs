//! Trusted Linux/Android directory bindings and metadata-only preflight checks.
//! Includes bounded direct-file reads; no durable registry, FFI or write API.
//! A successful check is NOT a token for a later ordinary path-based open.
mod exclusion;
mod platform;
mod read;
pub use read::FileReadError;

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use wex_core::access::{
    decide_local_access, Access, Decision, Denial, DirectoryGrant, LocalTarget,
};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Protection {
    #[default]
    Protected,
    /// Only a trusted, explicit user classification may select this.
    General,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceError {
    Unsupported,
    InvalidPath,
    Unavailable,
    UnknownDirectory,
    AlreadyRegistered,
    LimitExceeded,
    OutsideDirectory,
    ProtectedBoundary,
    UnsupportedTarget,
    InternalDirectory,
}

/// Object identity: device + inode on Unix, volume serial + file index on
/// Windows. Two objects compare equal only when they are the same object.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Identity {
    device: u64,
    inode: u64,
}

struct Binding {
    id: String,
    path: PathBuf,
    identity: Identity,
    protection: Protection,
    // Retain the directory object so a removed inode cannot be recycled while
    // this binding exists. No content is read through this descriptor.
    _pin: File,
}

impl Binding {
    fn verify(&self) -> Result<(), ResourceError> {
        let path = fs::canonicalize(&self.path).map_err(|_| ResourceError::Unavailable)?;
        let metadata = fs::symlink_metadata(&self.path).map_err(|_| ResourceError::Unavailable)?;
        if path != self.path
            || !metadata.is_dir()
            || platform::is_link(&metadata)
            || platform::identity_of_path(&self.path)? != self.identity
        {
            return Err(ResourceError::Unavailable);
        }
        Ok(())
    }
}

/// In-memory trusted host state. Recreating it deliberately creates fresh IDs;
/// persisted grants alone cannot restore a binding or reclassify a directory.
/// Do not expose register/classification as an AI tool or a Space association.
#[derive(Default)]
pub struct DirectoryRegistry {
    bindings: Vec<Binding>,
}

impl DirectoryRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        &mut self,
        path: impl AsRef<Path>,
        protection: Protection,
    ) -> Result<String, ResourceError> {
        self.register_with_exclusions(path, protection, &[])
    }

    /// Trusted user-selection only. Reject private boundaries before retaining
    /// a binding; registration does not create or replace an authorization.
    pub fn register_with_exclusions(
        &mut self,
        path: impl AsRef<Path>,
        protection: Protection,
        internal_roots: &[&Path],
    ) -> Result<String, ResourceError> {
        if !platform::supported() {
            return Err(ResourceError::Unsupported);
        }
        if self.bindings.len() >= 128 {
            return Err(ResourceError::LimitExceeded);
        }
        let path = path.as_ref();
        let text = path.to_str().ok_or(ResourceError::InvalidPath)?;
        if !path.is_absolute() || text.len() > 4096 || text.contains('\0') {
            return Err(ResourceError::InvalidPath);
        }
        for binding in &self.bindings {
            binding.verify()?;
        }
        let path = fs::canonicalize(path).map_err(|_| ResourceError::Unavailable)?;
        let pin = platform::open_directory(&path)?;
        let metadata = pin.metadata().map_err(|_| ResourceError::Unavailable)?;
        if !metadata.is_dir() {
            return Err(ResourceError::UnsupportedTarget);
        }
        let identity = platform::identity_of_file(&pin)?;
        if self
            .bindings
            .iter()
            .any(|b| b.path == path || b.identity == identity)
        {
            return Err(ResourceError::AlreadyRegistered);
        }
        let id = platform::random_id()?;
        // No time/counter fallback or caller-selected ID, even on collision.
        if self.bindings.iter().any(|b| b.id == id) {
            return Err(ResourceError::Unavailable);
        }
        let binding = Binding {
            id: id.clone(),
            path,
            identity,
            protection,
            _pin: pin,
        };
        binding.verify()?;
        exclusion::guard(&binding, internal_roots)?;
        self.bindings.push(binding);
        Ok(id)
    }

    /// Existing ordinary file/directory metadata only. Every relative target
    /// must be checked independently; Allowed never authorizes recursive I/O.
    pub fn check(
        &self,
        directory_id: &str,
        relative: &str,
        system_access: bool,
        requested: Access,
        grants: &[DirectoryGrant<'_>],
    ) -> Result<Decision, ResourceError> {
        self.check_with_exclusions(
            directory_id,
            relative,
            system_access,
            requested,
            grants,
            &[],
        )
    }

    /// Trusted Host deny roots only; never accept them from an AI/Space request.
    /// Authorization denial precedes even exclusion metadata. A matching root
    /// cannot be enabled by a grant or explicit personal consent.
    pub fn check_with_exclusions(
        &self,
        directory_id: &str,
        relative: &str,
        system_access: bool,
        requested: Access,
        grants: &[DirectoryGrant<'_>],
        internal_roots: &[&Path],
    ) -> Result<Decision, ResourceError> {
        if !platform::supported() {
            return Err(ResourceError::Unsupported);
        }
        let binding = self
            .bindings
            .iter()
            .find(|b| b.id == directory_id)
            .ok_or(ResourceError::UnknownDirectory)?;
        if !system_access {
            return Ok(Decision::Denied(Denial::SystemUnavailable));
        }
        if relative.len() > 4096
            || relative.contains('\0')
            || (!relative.is_empty()
                && (relative.split('/').count() > 64
                    || relative
                        .split('/')
                        .any(|p| p.is_empty() || p == "." || p == "..")))
        {
            return Err(ResourceError::InvalidPath);
        }
        // Denial must precede resource metadata: otherwise missing/existing
        // paths would expose an unauthorized existence/type oracle.
        let decision = decide_local_access(
            LocalTarget {
                directory_id: &binding.id,
                personal: binding.protection == Protection::Protected,
                system_access,
            },
            requested,
            grants,
        );
        if decision != Decision::Allowed {
            return Ok(decision);
        }
        exclusion::guard(binding, internal_roots)?;
        let protected: Vec<_> = self
            .bindings
            .iter()
            .filter(|b| b.id != binding.id && b.protection == Protection::Protected)
            .collect();
        let pending_target = binding.path.join(relative);
        // Reject known protected scopes before probing their descendants.
        if protected
            .iter()
            .any(|p| pending_target.starts_with(&p.path) || p.path.starts_with(&pending_target))
        {
            return Err(ResourceError::ProtectedBoundary);
        }
        // A lost protection root must not silently unprotect another binding.
        for registered in &self.bindings {
            registered.verify()?;
        }
        // Never follow an AI-selected symlink to discover whether its
        // destination exists. All target symlink components are unsupported.
        let mut component = binding.path.clone();
        for part in relative.split('/').filter(|p| !p.is_empty()) {
            component.push(part);
            let metadata =
                fs::symlink_metadata(&component).map_err(|_| ResourceError::Unavailable)?;
            if platform::is_link(&metadata) {
                return Err(ResourceError::OutsideDirectory);
            }
        }
        let target = fs::canonicalize(pending_target).map_err(|_| ResourceError::Unavailable)?;
        if !target.starts_with(&binding.path) {
            return Err(ResourceError::OutsideDirectory);
        }
        let metadata = fs::metadata(&target).map_err(|_| ResourceError::Unavailable)?;
        if !metadata.is_file() && !metadata.is_dir() {
            return Err(ResourceError::UnsupportedTarget);
        }
        // A hard-linked file can have aliases in a protected directory. The
        // authoritative handle-level check also runs in `platform::read_leaf`.
        if metadata.is_file() && !platform::has_single_link(&metadata) {
            return Err(ResourceError::UnsupportedTarget);
        }
        // Component boundaries, not textual ID/path prefixes. A parent grant's
        // personal flag never substitutes for this distinct root's consent.
        if protected
            .iter()
            .any(|p| target.starts_with(&p.path) || p.path.starts_with(&target))
        {
            return Err(ResourceError::ProtectedBoundary);
        }
        // Also recognize physical ancestor aliases (e.g. mounted aliases).
        for ancestor in target.ancestors() {
            let object = platform::identity_of_path(ancestor)?;
            if protected.iter().any(|p| p.identity == object) {
                return Err(ResourceError::ProtectedBoundary);
            }
        }
        // Catch ordinary directory replacement during metadata resolution.
        for registered in &self.bindings {
            registered.verify()?;
        }
        Ok(decision)
    }
}
