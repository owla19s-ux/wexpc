#![cfg(any(target_os = "linux", target_os = "android"))]
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use wex_access_store::{Grant, ReadSession, RegisteredReadError, Store, StoreError};
use wex_core::access::{Access, Denial};
use wex_local_resources::{FileReadError, ResourceError};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Sandbox(PathBuf);
impl Sandbox {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "wex-session-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("private")).unwrap();
        fs::create_dir(root.join("public")).unwrap();
        fs::write(root.join("public/file"), b"document").unwrap();
        Self(root)
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
    fn session(&self) -> ReadSession {
        ReadSession::open(self.path("authority"), &[&self.path("private")]).unwrap()
    }
    fn set(&self, id: &str, personal: bool) {
        let store = Store::open(self.path("authority")).unwrap();
        let revision = store.load().unwrap().revision;
        store
            .set(
                revision,
                Grant {
                    directory_id: id.into(),
                    access: Access::ReadWrite,
                    explicit_personal: personal,
                },
            )
            .unwrap();
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn denied(reason: Denial) -> RegisteredReadError {
    RegisteredReadError::File(FileReadError::Denied(reason))
}

#[test]
fn session_requires_host_policy_before_creating_an_authority_store() {
    let s = Sandbox::new();
    let authority = s.path("authority");
    assert!(matches!(
        ReadSession::open(&authority, &[]),
        Err(StoreError::Unavailable)
    ));
    assert!(!authority.exists());
    let root = s.path("private");
    let roots: Vec<&Path> = vec![&root; 16];
    assert!(matches!(
        ReadSession::open(&authority, &roots),
        Err(StoreError::LimitExceeded)
    ));
    assert!(!authority.exists());
}

#[test]
fn private_and_ancestor_registrations_do_not_enter_the_session() {
    let s = Sandbox::new();
    let mut session = s.session();
    fs::create_dir(s.path("private/child")).unwrap();
    symlink(s.path("private"), s.path("alias")).unwrap();
    for path in [
        s.path("private"),
        s.path("authority"),
        s.path("private/child"),
        s.0.clone(),
        s.path("alias"),
    ] {
        assert_eq!(
            session.register(path),
            Err(ResourceError::InternalDirectory)
        );
    }
    for _ in 0..128 {
        assert_eq!(
            session.register(s.path("private")),
            Err(ResourceError::InternalDirectory)
        );
    }
    // Failed registration must not consume the limit or expose an ID.
    let id = session.register(s.path("public")).unwrap();
    s.set(&id, true);
    assert_eq!(session.read(&id, "file", true), Ok(b"document".to_vec()));
}

#[test]
fn default_personal_protection_and_live_system_permission_are_enforced() {
    let s = Sandbox::new();
    let mut session = s.session();
    let id = session.register(s.path("public")).unwrap();
    assert_eq!(
        session.read(&id, "file", true),
        Err(denied(Denial::NoGrant))
    );
    s.set(&id, false);
    assert_eq!(
        session.read(&id, "file", true),
        Err(denied(Denial::PersonalNotAuthorized))
    );
    s.set(&id, true);
    assert_eq!(
        session.read(&id, "file", false),
        Err(denied(Denial::SystemUnavailable))
    );
    assert_eq!(session.read(&id, "file", true), Ok(b"document".to_vec()));
}

#[test]
fn external_revocation_affects_the_next_read_in_the_same_session() {
    let s = Sandbox::new();
    let mut session = s.session();
    let id = session.register(s.path("public")).unwrap();
    s.set(&id, true);
    assert_eq!(session.read(&id, "file", true), Ok(b"document".to_vec()));
    let admin = Store::open(s.path("authority")).unwrap();
    assert_eq!(admin.revoke(admin.load().unwrap().revision, &id), Ok(2));
    assert_eq!(
        session.read(&id, "file", true),
        Err(denied(Denial::NoGrant))
    );
}

#[test]
fn new_session_ids_do_not_restore_old_persisted_authority() {
    let s = Sandbox::new();
    let mut first = s.session();
    let old = first.register(s.path("public")).unwrap();
    s.set(&old, true);
    drop(first);
    let mut second = s.session();
    let new = second.register(s.path("public")).unwrap();
    assert_ne!(old, new);
    assert_eq!(
        second.read(&old, "file", true),
        Err(RegisteredReadError::File(FileReadError::Resource(
            ResourceError::UnknownDirectory
        )))
    );
    assert_eq!(
        second.read(&new, "file", true),
        Err(denied(Denial::NoGrant))
    );
    s.set(&new, true);
    assert_eq!(second.read(&new, "file", true), Ok(b"document".to_vec()));
}

#[test]
fn lost_internal_root_fails_closed_for_reads_and_new_registrations() {
    let s = Sandbox::new();
    let mut session = s.session();
    let id = session.register(s.path("public")).unwrap();
    s.set(&id, true);
    fs::rename(s.path("private"), s.path("moved")).unwrap();
    assert_eq!(
        session.read(&id, "file", true),
        Err(RegisteredReadError::File(FileReadError::Resource(
            ResourceError::Unavailable
        )))
    );
    fs::create_dir(s.path("other")).unwrap();
    assert_eq!(
        session.register(s.path("other")),
        Err(ResourceError::Unavailable)
    );
    fs::rename(s.path("moved"), s.path("private")).unwrap();
    assert_eq!(session.read(&id, "file", true), Ok(b"document".to_vec()));
}

#[test]
fn session_keeps_direct_file_limits_and_rejects_duplicate_registration() {
    let s = Sandbox::new();
    let mut session = s.session();
    let id = session.register(s.path("public")).unwrap();
    assert_eq!(
        session.register(s.path("public")),
        Err(ResourceError::AlreadyRegistered)
    );
    s.set(&id, true);
    fs::write(s.path("public/exact"), vec![7; 65536]).unwrap();
    fs::write(s.path("public/large"), vec![7; 65537]).unwrap();
    assert_eq!(session.read(&id, "exact", true).unwrap().len(), 65536);
    assert_eq!(
        session.read(&id, "large", true),
        Err(RegisteredReadError::File(FileReadError::TooLarge))
    );
    assert_eq!(
        session.read(&id, "../file", true),
        Err(RegisteredReadError::File(FileReadError::Resource(
            ResourceError::InvalidPath
        )))
    );
}
