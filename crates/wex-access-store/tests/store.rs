#![cfg(unix)]

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use wex_access_store::{Grant, Store, StoreError};
use wex_core::access::{Access, Decision, Denial, LocalTarget};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Sandbox(PathBuf);
impl Sandbox {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "wex-access-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn store(&self) -> Store {
        Store::open(self.0.join("private")).unwrap()
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn grant(id: &str, access: Access, personal: bool) -> Grant {
    Grant {
        directory_id: id.into(),
        access,
        explicit_personal: personal,
    }
}
fn check(store: &Store, id: &str, personal: bool, requested: Access) -> Decision {
    store
        .check(
            LocalTarget {
                directory_id: id,
                personal,
                system_access: true,
            },
            requested,
        )
        .unwrap()
}

#[test]
fn empty_store_does_not_grant_access() {
    let root = Sandbox::new();
    let store = root.store();
    assert_eq!(store.load().unwrap().revision, 0);
    assert_eq!(
        check(&store, "docs", false, Access::Read),
        Decision::Denied(Denial::NoGrant)
    );
}

#[test]
fn private_grants_survive_reopen_and_remain_independent() {
    let root = Sandbox::new();
    let store = root.store();
    assert_eq!(store.set(0, grant("docs", Access::Read, false)).unwrap(), 1);
    assert_eq!(
        store.set(1, grant("notes", Access::Write, false)).unwrap(),
        2
    );
    let reopened = root.store();
    assert_eq!(
        check(&reopened, "docs", false, Access::Read),
        Decision::Allowed
    );
    assert_eq!(
        check(&reopened, "docs", false, Access::Write),
        Decision::Denied(Denial::OperationNotAuthorized)
    );
    assert_eq!(
        check(&reopened, "notes", false, Access::Read),
        Decision::Denied(Denial::OperationNotAuthorized)
    );
}

#[test]
fn replacing_a_grant_can_reduce_access() {
    let root = Sandbox::new();
    let store = root.store();
    store
        .set(0, grant("docs", Access::ReadWrite, false))
        .unwrap();
    store.set(1, grant("docs", Access::Read, false)).unwrap();
    assert_eq!(store.load().unwrap().grants.len(), 1);
    assert_eq!(
        check(&store, "docs", false, Access::Write),
        Decision::Denied(Denial::OperationNotAuthorized)
    );
}

#[test]
fn revocation_is_persistent_and_other_instances_reload() {
    let root = Sandbox::new();
    let first = root.store();
    let second = root.store();
    first
        .set(0, grant("docs", Access::ReadWrite, false))
        .unwrap();
    assert_eq!(
        check(&second, "docs", false, Access::Read),
        Decision::Allowed
    );
    first.revoke(1, "docs").unwrap();
    assert_eq!(
        check(&second, "docs", false, Access::Read),
        Decision::Denied(Denial::NoGrant)
    );
    assert_eq!(
        check(&root.store(), "docs", false, Access::Read),
        Decision::Denied(Denial::NoGrant)
    );
}

#[test]
fn stale_editor_cannot_restore_revoked_authority() {
    let root = Sandbox::new();
    let store = root.store();
    store.set(0, grant("docs", Access::Read, false)).unwrap();
    store.revoke(1, "docs").unwrap();
    assert_eq!(
        store.set(1, grant("docs", Access::ReadWrite, false)),
        Err(StoreError::Conflict)
    );
    assert_eq!(
        check(&store, "docs", false, Access::Read),
        Decision::Denied(Denial::NoGrant)
    );
}

#[test]
fn personal_consent_and_system_capability_are_checked_separately() {
    let root = Sandbox::new();
    let store = root.store();
    store.set(0, grant("docs", Access::Read, false)).unwrap();
    assert_eq!(
        check(&store, "docs", true, Access::Read),
        Decision::Denied(Denial::PersonalNotAuthorized)
    );
    store.set(1, grant("docs", Access::Read, true)).unwrap();
    assert_eq!(check(&store, "docs", true, Access::Read), Decision::Allowed);
    assert_eq!(
        store
            .check(
                LocalTarget {
                    directory_id: "docs",
                    personal: true,
                    system_access: false,
                },
                Access::Read
            )
            .unwrap(),
        Decision::Denied(Denial::SystemUnavailable)
    );
}

#[test]
fn corrupt_or_unknown_snapshots_never_reset_to_empty_success() {
    let root = Sandbox::new();
    let store = root.store();
    for text in [
        "{",
        r#"{"schema_version":2,"revision":0,"grants":[]}"#,
        r#"{"schema_version":1,"revision":0,"revision":1,"grants":[]}"#,
        r#"{"schema_version":1,"revision":0,"grants":[],"extra":true}"#,
        r#"{"schema_version":1,"revision":0,"grants":[{"directory_id":"x","access":"read","explicit_personal":false},{"directory_id":"x","access":"write","explicit_personal":false}]}"#,
    ] {
        fs::write(root.0.join("private/grants.json"), text).unwrap();
        assert_eq!(store.load(), Err(StoreError::InvalidSnapshot));
        assert!(store
            .check(
                LocalTarget {
                    directory_id: "x",
                    personal: false,
                    system_access: true,
                },
                Access::Read
            )
            .is_err());
    }
}

#[test]
fn invalid_identifiers_and_oversized_files_are_rejected() {
    let root = Sandbox::new();
    let store = root.store();
    for id in ["", "a/b", "..", "空", &"a".repeat(65)] {
        assert_eq!(
            store.set(0, grant(id, Access::Read, true)),
            Err(StoreError::InvalidGrant)
        );
    }
    fs::write(root.0.join("private/grants.json"), vec![b' '; 65537]).unwrap();
    assert_eq!(store.load(), Err(StoreError::InvalidSnapshot));
}

#[test]
fn busy_or_stale_lock_never_returns_cached_allow() {
    let root = Sandbox::new();
    let store = root.store();
    store.set(0, grant("docs", Access::Read, false)).unwrap();
    fs::write(root.0.join("private/grants.lock"), b"").unwrap();
    assert_eq!(store.load(), Err(StoreError::Busy));
    assert_eq!(store.revoke(1, "docs"), Err(StoreError::Busy));
    assert!(store
        .check(
            LocalTarget {
                directory_id: "docs",
                personal: false,
                system_access: true,
            },
            Access::Read
        )
        .is_err());
}

#[test]
fn failed_save_does_not_replace_previous_permissions() {
    let root = Sandbox::new();
    let store = root.store();
    store.set(0, grant("docs", Access::Read, false)).unwrap();
    fs::write(root.0.join("private/grants.tmp"), b"crash residue").unwrap();
    assert_eq!(
        store.set(1, grant("docs", Access::ReadWrite, false)),
        Err(StoreError::Unavailable)
    );
    assert_eq!(store.load(), Err(StoreError::Unavailable));
    // Simulate a trusted repair after inspecting the last durable snapshot.
    // Production never removes crash residue automatically.
    fs::remove_file(root.0.join("private/grants.tmp")).unwrap();
    assert_eq!(
        check(&store, "docs", false, Access::Write),
        Decision::Denied(Denial::OperationNotAuthorized)
    );
    assert_eq!(store.load().unwrap().revision, 1);
}

#[cfg(unix)]
#[test]
fn private_modes_and_symlink_rejection() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let root = Sandbox::new();
    let store = root.store();
    store.set(0, grant("docs", Access::Read, false)).unwrap();
    let directory = root.0.join("private");
    assert_eq!(
        fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(directory.join("grants.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let link = root.0.join("alias");
    symlink(&directory, &link).unwrap();
    assert!(matches!(Store::open(link), Err(StoreError::Unavailable)));
    fs::remove_file(directory.join("grants.json")).unwrap();
    let outside = root.0.join("outside.json");
    fs::write(&outside, b"{}").unwrap();
    symlink(outside, directory.join("grants.json")).unwrap();
    assert_eq!(store.load(), Err(StoreError::Unavailable));
}

#[test]
fn failed_revocation_quarantines_old_authority() {
    let root = Sandbox::new();
    let store = root.store();
    store
        .set(0, grant("docs", Access::ReadWrite, false))
        .unwrap();
    fs::write(root.0.join("private/grants.tmp"), b"crash residue").unwrap();
    assert_eq!(store.revoke(1, "docs"), Err(StoreError::Unavailable));
    assert_eq!(store.load(), Err(StoreError::Unavailable));
    assert_eq!(root.store().load(), Err(StoreError::Unavailable));
    assert!(store
        .check(
            LocalTarget {
                directory_id: "docs",
                personal: false,
                system_access: true,
            },
            Access::Read
        )
        .is_err());
}

#[test]
fn capacity_rejects_new_ids_but_allows_downgrade() {
    let root = Sandbox::new();
    let store = root.store();
    let grants: Vec<_> = (0..128)
        .map(|i| {
            serde_json::json!({
                "directory_id":format!("id-{i}"), "access":"read_write", "explicit_personal":false,
            })
        })
        .collect();
    fs::write(
        root.0.join("private/grants.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema_version":1, "revision":128, "grants":grants,
        }))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        store.set(128, grant("overflow", Access::Read, false)),
        Err(StoreError::LimitExceeded)
    );
    assert_eq!(
        store.set(128, grant("id-0", Access::Read, false)).unwrap(),
        129
    );
    assert_eq!(
        check(&store, "id-0", false, Access::Write),
        Decision::Denied(Denial::OperationNotAuthorized)
    );
}

#[test]
fn revision_overflow_never_wraps_to_an_old_edit_version() {
    let root = Sandbox::new();
    let store = root.store();
    fs::write(
        root.0.join("private/grants.json"),
        format!(
            r#"{{"schema_version":1,"revision":{},"grants":[]}}"#,
            u64::MAX,
        ),
    )
    .unwrap();
    assert_eq!(
        store.set(u64::MAX, grant("docs", Access::Read, false)),
        Err(StoreError::RevisionExhausted)
    );
    assert_eq!(store.load(), Err(StoreError::Busy));
}

#[test]
fn concurrent_editors_cannot_both_commit_the_same_revision() {
    use std::sync::{Arc, Barrier};
    let root = Sandbox::new();
    let store = root.store();
    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> = ["a", "b"]
        .into_iter()
        .map(|id| {
            let barrier = barrier.clone();
            let path = root.0.join("private");
            std::thread::spawn(move || {
                let store = Store::open(path).unwrap();
                barrier.wait();
                store.set(0, grant(id, Access::ReadWrite, false))
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert!(results
        .iter()
        .all(|r| matches!(r, Ok(1) | Err(StoreError::Busy | StoreError::Conflict))));
    let snapshot = store.load().unwrap();
    assert_eq!(snapshot.revision, 1);
    assert_eq!(snapshot.grants.len(), 1);
}

#[test]
fn orphan_temporary_snapshot_blocks_checks_without_any_new_edit() {
    let root = Sandbox::new();
    let store = root.store();
    store
        .set(0, grant("docs", Access::ReadWrite, false))
        .unwrap();
    fs::write(root.0.join("private/grants.tmp"), b"interrupted revoke").unwrap();
    assert_eq!(root.store().load(), Err(StoreError::Unavailable));
    assert!(store
        .check(
            LocalTarget {
                directory_id: "docs",
                personal: false,
                system_access: true,
            },
            Access::Read
        )
        .is_err());
}

#[test]
fn blocked_downgrade_does_not_fall_back_to_old_write_authority() {
    let root = Sandbox::new();
    let store = root.store();
    store
        .set(0, grant("docs", Access::ReadWrite, false))
        .unwrap();
    fs::write(root.0.join("private/grants.tmp"), b"interrupted writer").unwrap();
    assert_eq!(
        store.set(1, grant("docs", Access::Read, false)),
        Err(StoreError::Unavailable)
    );
    assert!(root
        .store()
        .check(
            LocalTarget {
                directory_id: "docs",
                personal: false,
                system_access: true,
            },
            Access::Write
        )
        .is_err());
}

#[test]
fn private_root_permissions_are_rechecked_after_open() {
    use std::os::unix::fs::PermissionsExt;
    let root = Sandbox::new();
    let store = root.store();
    store.set(0, grant("docs", Access::Read, false)).unwrap();
    fs::set_permissions(root.0.join("private"), fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(store.load(), Err(StoreError::Unavailable));
}
