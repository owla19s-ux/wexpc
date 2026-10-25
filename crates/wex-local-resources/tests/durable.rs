#![cfg(any(target_os = "linux", windows))]
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
use wex_core::access::{Access, DirectoryGrant};
use wex_local_resources::{DirectoryRegistry, Protection, ResourceError};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Sandbox(PathBuf);
impl Sandbox {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "wex-durable-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        fs::create_dir(p.join("docs")).unwrap();
        fs::write(p.join("docs/a"), b"hello").unwrap();
        Self(p)
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn restart_restores_same_identity_but_never_creates_a_grant() {
    let s = Sandbox::new();
    let mut r = DirectoryRegistry::new();
    let id = r.register(s.0.join("docs"), Protection::Protected).unwrap();
    let saved = r.export_binding(&id).unwrap();
    drop(r);
    let mut r = DirectoryRegistry::new();
    r.restore_binding(&saved, &[]).unwrap();
    assert!(r.read_file(&id, "a", true, &[]).is_err());
    assert_eq!(
        r.read_file(
            &id,
            "a",
            true,
            &[DirectoryGrant {
                directory_id: &id,
                access: Access::Read,
                explicit_personal: true
            }]
        )
        .unwrap(),
        b"hello"
    );
}
#[test]
fn replacement_or_birth_timestamp_change_cannot_reuse_a_saved_grant() {
    let s = Sandbox::new();
    let mut r = DirectoryRegistry::new();
    let id = r.register(s.0.join("docs"), Protection::Protected).unwrap();
    let saved = r.export_binding(&id).unwrap();
    drop(r);
    let mut changed = saved.clone();
    changed.stamp[3] ^= 1;
    assert_eq!(
        DirectoryRegistry::new().restore_binding(&changed, &[]),
        Err(ResourceError::Unavailable)
    );
    fs::rename(s.0.join("docs"), s.0.join("old")).unwrap();
    fs::create_dir(s.0.join("docs")).unwrap();
    assert_eq!(
        DirectoryRegistry::new().restore_binding(&saved, &[]),
        Err(ResourceError::Unavailable)
    );
}
#[test]
fn restored_bindings_still_apply_current_internal_exclusions() {
    let s = Sandbox::new();
    let mut r = DirectoryRegistry::new();
    let id = r.register(s.0.join("docs"), Protection::Protected).unwrap();
    let saved = r.export_binding(&id).unwrap();
    drop(r);
    assert_eq!(
        DirectoryRegistry::new().restore_binding(&saved, &[s.0.as_path()]),
        Err(ResourceError::InternalDirectory)
    );
}
#[cfg(unix)]
#[test]
fn a_symlink_replacement_cannot_restore_a_saved_binding() {
    let s = Sandbox::new();
    let mut r = DirectoryRegistry::new();
    let id = r.register(s.0.join("docs"), Protection::Protected).unwrap();
    let saved = r.export_binding(&id).unwrap();
    drop(r);
    fs::rename(s.0.join("docs"), s.0.join("old")).unwrap();
    std::os::unix::fs::symlink(s.0.join("old"), s.0.join("docs")).unwrap();
    assert_eq!(
        DirectoryRegistry::new().restore_binding(&saved, &[]),
        Err(ResourceError::Unavailable)
    );
}
