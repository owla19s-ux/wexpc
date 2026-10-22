#![cfg(any(target_os = "linux", target_os = "android"))]
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use wex_core::access::{Access, Decision, Denial, DirectoryGrant};
use wex_local_resources::{DirectoryRegistry, FileReadError, Protection, ResourceError};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Sandbox(PathBuf);
impl Sandbox {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "wex-exclude-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        for name in ["private", "public", "private-other"] {
            fs::create_dir(p.join(name)).unwrap();
            fs::write(p.join(name).join("file"), b"data").unwrap();
        }
        fs::create_dir(p.join("private/child")).unwrap();
        fs::write(p.join("private/child/file"), b"secret").unwrap();
        Self(p)
    }
    fn private(&self) -> PathBuf {
        self.0.join("private")
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn checked(
    r: &DirectoryRegistry,
    id: &str,
    leaf: &str,
    roots: &[&Path],
) -> Result<Decision, ResourceError> {
    r.check_with_exclusions(
        id,
        leaf,
        true,
        Access::Read,
        &[DirectoryGrant {
            directory_id: id,
            access: Access::ReadWrite,
            explicit_personal: true,
        }],
        roots,
    )
}
fn read(
    r: &DirectoryRegistry,
    id: &str,
    leaf: &str,
    roots: &[&Path],
) -> Result<Vec<u8>, FileReadError> {
    r.read_file_with_exclusions(
        id,
        leaf,
        true,
        &[DirectoryGrant {
            directory_id: id,
            access: Access::ReadWrite,
            explicit_personal: true,
        }],
        roots,
    )
}

#[test]
fn explicit_personal_grant_never_overrides_internal_root() {
    let s = Sandbox::new();
    let mut r = DirectoryRegistry::new();
    let id = r.register(s.private(), Protection::Protected).unwrap();
    assert_eq!(
        checked(&r, &id, "file", &[&s.private()]),
        Err(ResourceError::InternalDirectory)
    );
    assert_eq!(
        read(&r, &id, "file", &[&s.private()]),
        Err(FileReadError::Resource(ResourceError::InternalDirectory))
    );
}

#[test]
fn ancestor_and_child_bindings_are_both_excluded_before_target_probe() {
    let s = Sandbox::new();
    let mut r = DirectoryRegistry::new();
    let ancestor = r.register(&s.0, Protection::General).unwrap();
    let child = r
        .register(s.private().join("child"), Protection::General)
        .unwrap();
    for id in [&ancestor, &child] {
        assert_eq!(
            checked(&r, id, "missing", &[&s.private()]),
            Err(ResourceError::InternalDirectory)
        );
        assert_eq!(
            read(&r, id, "missing", &[&s.private()]),
            Err(FileReadError::Resource(ResourceError::InternalDirectory))
        );
    }
}

#[test]
fn symlink_alias_is_canonicalized_but_cannot_bypass_exclusion() {
    let s = Sandbox::new();
    symlink(s.private(), s.0.join("alias")).unwrap();
    let mut r = DirectoryRegistry::new();
    let id = r.register(s.0.join("alias"), Protection::General).unwrap();
    assert_eq!(
        read(&r, &id, "file", &[&s.private()]),
        Err(FileReadError::Resource(ResourceError::InternalDirectory))
    );
}

#[test]
fn component_neighbor_and_unrelated_user_files_remain_readable() {
    let s = Sandbox::new();
    let mut r = DirectoryRegistry::new();
    for dir in ["private-other", "public"] {
        let id = r.register(s.0.join(dir), Protection::General).unwrap();
        assert_eq!(
            checked(&r, &id, "file", &[&s.private()]),
            Ok(Decision::Allowed)
        );
        assert_eq!(read(&r, &id, "file", &[&s.private()]), Ok(b"data".to_vec()));
    }
}

#[test]
fn malformed_unavailable_or_file_exclusion_is_never_ignored() {
    let s = Sandbox::new();
    let mut r = DirectoryRegistry::new();
    let id = r.register(s.0.join("public"), Protection::General).unwrap();
    for root in [
        PathBuf::from("relative"),
        s.0.join("missing"),
        s.private().join("file"),
    ] {
        assert!(checked(&r, &id, "file", &[&root]).is_err());
        assert!(read(&r, &id, "file", &[&root]).is_err());
    }
}

#[test]
fn authorization_denial_precedes_exclusion_metadata_or_capacity() {
    let s = Sandbox::new();
    let mut r = DirectoryRegistry::new();
    let id = r
        .register(s.0.join("public"), Protection::Protected)
        .unwrap();
    let missing = s.0.join("missing");
    let roots: Vec<&Path> = vec![&missing; 17];
    assert_eq!(
        r.check_with_exclusions(&id, "missing", true, Access::Read, &[], &roots),
        Ok(Decision::Denied(Denial::NoGrant))
    );
    assert_eq!(
        r.read_file_with_exclusions(&id, "missing", true, &[], &roots),
        Err(FileReadError::Denied(Denial::NoGrant))
    );
    assert_eq!(
        r.read_file_with_exclusions(&id, "missing", false, &[], &roots),
        Err(FileReadError::Denied(Denial::SystemUnavailable))
    );
}

#[test]
fn exclusion_work_is_bounded_to_sixteen_trusted_roots() {
    let s = Sandbox::new();
    let mut r = DirectoryRegistry::new();
    let id = r.register(s.0.join("public"), Protection::General).unwrap();
    let root = s.private();
    let sixteen: Vec<&Path> = vec![&root; 16];
    assert_eq!(read(&r, &id, "file", &sixteen), Ok(b"data".to_vec()));
    let seventeen: Vec<&Path> = vec![&root; 17];
    assert_eq!(
        checked(&r, &id, "file", &seventeen),
        Err(ResourceError::LimitExceeded)
    );
}
