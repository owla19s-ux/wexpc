#![cfg(any(target_os = "linux", windows))]
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
use wex_access_store::{Grant, Store, StoreError};
use wex_core::access::Access;
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Sandbox(PathBuf);
impl Sandbox {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "wex-store-durable-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        for name in ["a", "b", "private"] {
            fs::create_dir(p.join(name)).unwrap();
            fs::write(p.join(name).join("file"), name).unwrap();
        }
        Self(p)
    }
    fn store(&self) -> Store {
        Store::open_with_internal_roots(self.0.join("auth"), &[self.0.join("private").as_path()])
            .unwrap()
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn multi_directory_authorization_survives_restart_and_revoke_is_live() {
    let s = Sandbox::new();
    let store = s.store();
    let a = store
        .authorize_directory(0, s.0.join("a"), Access::Read)
        .unwrap();
    let b = store
        .authorize_directory(1, s.0.join("b"), Access::Write)
        .unwrap();
    drop(store);
    let store = s.store();
    assert_eq!(store.load().unwrap().directories.len(), 2);
    assert_eq!(store.read_authorized(&a.id, "file", true).unwrap(), b"a");
    assert!(store.read_authorized(&b.id, "file", true).is_err());
    store
        .set(
            2,
            Grant {
                directory_id: b.id.clone(),
                access: Access::ReadWrite,
                explicit_personal: true,
            },
        )
        .unwrap();
    assert_eq!(store.read_authorized(&b.id, "file", true).unwrap(), b"b");
    assert_eq!(store.revoke(2, &a.id), Err(StoreError::Conflict));
    store.revoke(3, &a.id).unwrap();
    assert!(store.read_authorized(&a.id, "file", true).is_err());
    assert_eq!(store.read_authorized(&b.id, "file", true).unwrap(), b"b");
    assert!(store.read_authorized(&b.id, "file", false).is_err());
}
#[test]
fn replaced_directory_or_internal_path_never_gains_old_authority() {
    let s = Sandbox::new();
    let store = s.store();
    assert!(store
        .authorize_directory(0, s.0.join("private"), Access::Read)
        .is_err());
    assert_eq!(store.load().unwrap().revision, 0);
    let a = store
        .authorize_directory(0, s.0.join("a"), Access::Read)
        .unwrap();
    fs::rename(s.0.join("a"), s.0.join("old")).unwrap();
    fs::create_dir(s.0.join("a")).unwrap();
    fs::write(s.0.join("a/file"), b"different").unwrap();
    assert!(s.store().read_authorized(&a.id, "file", true).is_err());
    store.revoke(1, &a.id).unwrap();
    assert!(store.load().unwrap().directories.is_empty());
}
#[test]
fn offline_unrelated_directory_does_not_block_other_authorized_reads() {
    let s = Sandbox::new();
    let store = s.store();
    let a = store
        .authorize_directory(0, s.0.join("a"), Access::Read)
        .unwrap();
    let b = store
        .authorize_directory(1, s.0.join("b"), Access::Read)
        .unwrap();
    fs::rename(s.0.join("a"), s.0.join("offline")).unwrap();
    assert!(store.read_authorized(&a.id, "file", true).is_err());
    assert_eq!(store.read_authorized(&b.id, "file", true).unwrap(), b"b");
}
#[test]
fn schema_one_preserves_grants_and_new_binding_commits_as_schema_two() {
    let s = Sandbox::new();
    let store = s.store();
    fs::write(s.0.join("auth/grants.json"),br#"{"schema_version":1,"revision":7,"grants":[{"directory_id":"old","access":"read","explicit_personal":true}]}"#).unwrap();
    assert_eq!(store.load().unwrap().grants[0].directory_id, "old");
    assert!(store.load().unwrap().directories.is_empty());
    store
        .authorize_directory(7, s.0.join("a"), Access::Read)
        .unwrap();
    let wire: serde_json::Value =
        serde_json::from_slice(&fs::read(s.0.join("auth/grants.json")).unwrap()).unwrap();
    assert_eq!(wire["schema_version"], 2);
    assert_eq!(store.load().unwrap().grants.len(), 2);
}
#[test]
fn write_only_denial_precedes_any_filesystem_probe_and_stale_approval_cannot_read() {
    use wex_access_store::RegisteredReadError;
    use wex_core::access::Denial;
    use wex_local_resources::FileReadError;
    let s = Sandbox::new();
    let store = s.store();
    let a = store
        .authorize_directory(0, s.0.join("a"), Access::Write)
        .unwrap();
    fs::rename(s.0.join("a"), s.0.join("offline")).unwrap();
    assert_eq!(
        store.read_authorized(&a.id, "file", true),
        Err(RegisteredReadError::File(FileReadError::Denied(
            Denial::OperationNotAuthorized
        )))
    );
    assert_eq!(
        store.read_authorized_at(Some(0), &a.id, "file", true),
        Err(RegisteredReadError::File(FileReadError::Changed))
    );
}

#[test]
fn explicitly_authorized_nested_directories_remain_independent_scopes() {
    let s = Sandbox::new();
    fs::create_dir(s.0.join("a/nested")).unwrap();
    fs::write(s.0.join("a/nested/file"), b"nested").unwrap();
    let store = s.store();
    let parent = store
        .authorize_directory(0, s.0.join("a"), Access::Read)
        .unwrap();
    let child = store
        .authorize_directory(1, s.0.join("a/nested"), Access::Read)
        .unwrap();
    assert_eq!(
        store.read_authorized(&parent.id, "file", true).unwrap(),
        b"a"
    );
    assert_eq!(
        store.read_authorized(&child.id, "file", true).unwrap(),
        b"nested"
    );
    store.revoke(2, &child.id).unwrap();
    assert!(store
        .read_authorized(&parent.id, "nested/file", true)
        .is_err());
    assert_eq!(
        store.read_authorized(&parent.id, "file", true).unwrap(),
        b"a"
    );
}

#[test]
fn capacity_rejected_addition_does_not_quarantine_existing_authority() {
    let s = Sandbox::new();
    let store = s.store();
    let first = store
        .authorize_directory(0, s.0.join("a"), Access::Read)
        .unwrap();
    let mut long = s.0.join("b");
    for _ in 0..13 {
        long.push("a".repeat(180));
        fs::create_dir(&long).unwrap();
    }
    let mut revision = 1;
    let mut rejected = false;
    for n in 0..40 {
        let path = long.join(format!("directory-{n}"));
        fs::create_dir(&path).unwrap();
        match store.authorize_directory(revision, path, Access::Read) {
            Ok(_) => revision += 1,
            Err(StoreError::LimitExceeded) => {
                rejected = true;
                break;
            }
            Err(e) => panic!("unexpected {e:?}"),
        }
    }
    assert!(
        rejected,
        "exercise real serialized capacity, not the grant-count bound"
    );
    let reopened = s.store();
    assert_eq!(
        reopened.read_authorized(&first.id, "file", true).unwrap(),
        b"a"
    );
    assert_eq!(reopened.revoke(revision, &first.id), Ok(revision + 1));
}

#[test]
fn exclusive_authorized_create_is_durable_and_never_overwrites() {
    let s = Sandbox::new();
    let store = s.store();
    let directory = store.authorize_directory(0, s.0.join("a"), Access::Write).unwrap();
    assert!(store.create_authorized_at(Some(1), &directory.id, "new.txt", b"created", true).is_ok());
    assert_eq!(fs::read(s.0.join("a/new.txt")).unwrap(), b"created");
    assert!(store.create_authorized_at(Some(1), &directory.id, "new.txt", b"replace", true).is_err());
    assert_eq!(fs::read(s.0.join("a/new.txt")).unwrap(), b"created");
    assert!(store.create_authorized_at(Some(0), &directory.id, "other.txt", b"stale", true).is_err());
    assert!(!s.0.join("a/other.txt").exists());
    assert!(store.create_authorized_at(Some(1), &directory.id, "../escape", b"bad", true).is_err());
    assert!(!s.0.join("escape").exists());
    store.revoke(1, &directory.id).unwrap();
    assert!(store.create_authorized_at(Some(2), &directory.id, "after.txt", b"bad", true).is_err());
    assert!(!s.0.join("a/after.txt").exists());
}

#[test]
fn read_only_grant_and_other_conversation_cannot_create_files() {
    let s = Sandbox::new();
    let store = s.store();
    let directory = store.authorize_directory(0, s.0.join("a"), Access::Read).unwrap();
    assert!(store.create_authorized_at(Some(1), &directory.id, "denied.txt", b"data", true).is_err());
    assert!(!s.0.join("a/denied.txt").exists());
    let different_store = Store::open(s.0.join("other-auth")).unwrap();
    assert!(different_store.create_authorized_at(None, &directory.id, "foreign.txt", b"data", true).is_err());
    assert!(!s.0.join("a/foreign.txt").exists());
}

#[test]
fn create_denies_internal_roots_and_requires_exact_directory_binding() {
    let s = Sandbox::new();
    let store = s.store();
    // An application-managed directory must never become a user-selected
    // write target, even if a model proposes a plausible directory name.
    assert!(store.authorize_directory(0, s.0.join("private"), Access::Write).is_err());
    let directory = store.authorize_directory(0, s.0.join("a"), Access::Write).unwrap();
    assert!(store.create_authorized_at(Some(1), &directory.id, "direct.txt", b"ok", true).is_ok());
    assert!(store.create_authorized_at(Some(1), &directory.id, "nested/direct.txt", b"bad", true).is_err());
    assert!(store.create_authorized_at(Some(1), &directory.id, ".", b"bad", true).is_err());
    assert!(store.create_authorized_at(Some(1), &directory.id, "..", b"bad", true).is_err());
    assert!(!s.0.join("a/nested").exists());
    assert!(store.create_authorized_at(Some(1), &directory.id, "system-off.txt", b"bad", false).is_err());
    assert!(!s.0.join("a/system-off.txt").exists());
}

#[cfg(unix)]
#[test]
fn exclusive_create_rejects_existing_symbolic_link_without_following_it() {
    use std::os::unix::fs::symlink;
    let s = Sandbox::new();
    let store = s.store();
    let directory = store.authorize_directory(0, s.0.join("a"), Access::Write).unwrap();
    let target = s.0.join("b/file");
    let original = fs::read(&target).unwrap();
    symlink(&target, s.0.join("a/link.txt")).unwrap();
    assert!(store.create_authorized_at(Some(1), &directory.id, "link.txt", b"overwrite", true).is_err());
    assert_eq!(fs::read(&target).unwrap(), original);
}

#[test]
fn versioned_replace_preflight_requires_live_write_grant_and_exact_bytes() {
    let s = Sandbox::new();
    let store = s.store();
    let directory = store.authorize_directory(0, s.0.join("a"), Access::Read).unwrap();
    assert!(store.prepare_authorized_replace_at(Some(1), &directory.id, "file", b"a", true).is_err());
    store.set(1, Grant { directory_id: directory.id.clone(),
        access: Access::ReadWrite, explicit_personal: true }).unwrap();
    assert!(store.prepare_authorized_replace_at(Some(2), &directory.id, "file", b"a", true).is_ok());
    assert!(store.prepare_authorized_replace_at(Some(2), &directory.id, "file", b"stale", true).is_err());
    assert!(store.prepare_authorized_replace_at(Some(1), &directory.id, "file", b"a", true).is_err());
    assert!(store.prepare_authorized_replace_at(Some(2), &directory.id, "../file", b"a", true).is_err());
    assert_eq!(fs::read(s.0.join("a/file")).unwrap(), b"a");
    store.revoke(2, &directory.id).unwrap();
    assert!(store.prepare_authorized_replace_at(Some(3), &directory.id, "file", b"a", true).is_err());
}
