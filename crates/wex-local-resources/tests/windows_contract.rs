#![cfg(windows)]

//! Windows contract tests.
//!
//! Mirrors the Unix suite: the same authorization, boundary, identity and size
//! rules must hold on a second platform. The backend opens every leaf relative
//! to the pinned directory handle (`NtCreateFile` + `RootDirectory`), so these
//! tests exercise a genuinely different mechanism than `openat`.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use wex_core::access::{Access, Decision, DirectoryGrant};
use wex_local_resources::{DirectoryRegistry, FileReadError, Protection, ResourceError};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Sandbox(PathBuf);

impl Sandbox {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "wex-win-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(path.join("docs")).unwrap();
        fs::write(path.join("docs").join("note.txt"), b"windows content").unwrap();
        Self(path)
    }

    fn docs(&self) -> PathBuf {
        self.0.join("docs")
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn grant(id: &str) -> DirectoryGrant<'_> {
    DirectoryGrant {
        directory_id: id,
        access: Access::Read,
        explicit_personal: true,
    }
}

#[test]
fn registration_returns_opaque_identity() {
    let root = Sandbox::new();
    let mut registry = DirectoryRegistry::new();
    let id = registry.register(root.docs(), Protection::General).unwrap();
    assert_eq!(id.len(), 64);
    assert!(id.bytes().all(|b| b.is_ascii_hexdigit()));
}

#[test]
fn duplicate_registration_is_rejected() {
    let root = Sandbox::new();
    let mut registry = DirectoryRegistry::new();
    registry.register(root.docs(), Protection::General).unwrap();
    assert_eq!(
        registry.register(root.docs(), Protection::General),
        Err(ResourceError::AlreadyRegistered)
    );
}

#[test]
fn reads_a_small_leaf_through_the_directory_handle() {
    let root = Sandbox::new();
    let mut registry = DirectoryRegistry::new();
    let id = registry.register(root.docs(), Protection::General).unwrap();
    let bytes = registry
        .read_file(&id, "note.txt", true, &[grant(&id)])
        .expect("read");
    assert_eq!(bytes, b"windows content");
}

#[test]
fn rejects_multi_component_leaf_names() {
    let root = Sandbox::new();
    let mut registry = DirectoryRegistry::new();
    let id = registry.register(root.docs(), Protection::General).unwrap();
    assert_eq!(
        registry.read_file(&id, "..\\note.txt", true, &[grant(&id)]),
        Err(FileReadError::Resource(ResourceError::InvalidPath))
    );
}

#[test]
fn rejects_oversized_leaf() {
    let root = Sandbox::new();
    fs::write(root.docs().join("big.bin"), vec![7u8; 65537]).unwrap();
    let mut registry = DirectoryRegistry::new();
    let id = registry.register(root.docs(), Protection::General).unwrap();
    assert_eq!(
        registry.read_file(&id, "big.bin", true, &[grant(&id)]),
        Err(FileReadError::TooLarge)
    );
}

#[test]
fn traversal_components_are_invalid() {
    let root = Sandbox::new();
    fs::write(root.0.join("outside.txt"), b"secret").unwrap();
    let mut registry = DirectoryRegistry::new();
    let id = registry.register(root.docs(), Protection::General).unwrap();
    assert_eq!(
        registry.check(&id, "../outside.txt", true, Access::Read, &[grant(&id)]),
        Err(ResourceError::InvalidPath)
    );
}

#[test]
fn nested_protected_directory_is_a_boundary() {
    let root = Sandbox::new();
    fs::create_dir_all(root.0.join("outer").join("inner")).unwrap();
    let mut registry = DirectoryRegistry::new();
    // Protected by default; a nested General directory must not be carved out
    // of it.
    let outer = registry
        .register(root.0.join("outer"), Protection::default())
        .unwrap();
    let inner = registry
        .register(root.0.join("outer").join("inner"), Protection::General)
        .unwrap();
    fs::write(root.0.join("outer").join("inner").join("file"), b"x").unwrap();
    // Registration succeeds; the boundary is enforced on access.
    assert_eq!(
        registry.check(&inner, "file", true, Access::Read, &[grant(&inner)]),
        Err(ResourceError::ProtectedBoundary)
    );
    let _ = outer;
}

#[test]
fn unknown_directory_is_rejected_before_touching_the_disk() {
    let registry = DirectoryRegistry::new();
    assert_eq!(
        registry.read_file("missing", "note.txt", true, &[grant("missing")]),
        Err(FileReadError::Resource(ResourceError::UnknownDirectory))
    );
}

#[test]
fn check_allows_a_registered_leaf_with_a_grant() {
    let root = Sandbox::new();
    let mut registry = DirectoryRegistry::new();
    let id = registry.register(root.docs(), Protection::General).unwrap();
    let decision = registry
        .check(&id, "note.txt", true, Access::Read, &[grant(&id)])
        .unwrap();
    assert_eq!(decision, Decision::Allowed);
}

#[test]
fn internal_roots_are_rejected_by_identity_not_by_text_prefix() {
    let root = Sandbox::new();
    let internal = root.0.join("docs").join("store");
    fs::create_dir_all(&internal).unwrap();
    let mut registry = DirectoryRegistry::new();
    // The chosen directory contains the Host's internal root.
    assert_eq!(
        registry.register_with_exclusions(root.docs(), Protection::General, &[&internal]),
        Err(ResourceError::InternalDirectory)
    );
}

#[test]
fn disjoint_internal_root_is_allowed() {
    let root = Sandbox::new();
    let internal = root.0.join("store");
    fs::create_dir_all(&internal).unwrap();
    let mut registry = DirectoryRegistry::new();
    let id = registry
        .register_with_exclusions(root.docs(), Protection::General, &[&internal])
        .expect("register");
    assert_eq!(id.len(), 64);
}

#[test]
fn platform_backend_is_reported_as_available() {
    assert!(wex_local_resources::platform_supported());
}
