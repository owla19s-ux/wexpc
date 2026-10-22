#![cfg(windows)]

//! Windows storage contract tests for the grant authority store.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use wex_access_store::{Grant, Store, StoreError};
use wex_core::access::Access;

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Sandbox(PathBuf);

impl Sandbox {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "wex-store-win-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        Self(path)
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn open_creates_a_private_root() {
    let sandbox = Sandbox::new();
    {
        let store = Store::open(&sandbox.0).expect("open");
        drop(store);
    }
    // verify_root runs again on reopen: a root that inherited a broad ACE
    // would be rejected here.
    let reopened = Store::open(&sandbox.0);
    assert!(reopened.is_ok(), "reopened: {reopened:?}");
}

#[test]
fn set_persists_through_atomic_replacement() {
    let sandbox = Sandbox::new();
    let store = Store::open(&sandbox.0).expect("open");
    let revision = store
        .set(
            0,
            Grant {
                directory_id: "dir-1".to_owned(),
                access: Access::Read,
                explicit_personal: true,
            },
        )
        .expect("set");
    assert_eq!(revision, 1);
    // Second write goes through MoveFileExW over an existing snapshot.
    let revision = store
        .set(
            1,
            Grant {
                directory_id: "dir-2".to_owned(),
                access: Access::Read,
                explicit_personal: false,
            },
        )
        .expect("second set");
    assert_eq!(revision, 2);
    let snapshot = store.load().expect("load");
    assert_eq!(snapshot.revision, 2);
    assert_eq!(snapshot.grants.len(), 2);
}

#[test]
fn broad_ace_on_the_root_is_rejected() {
    let sandbox = Sandbox::new();
    fs::create_dir_all(&sandbox.0).expect("mkdir");
    let status = Command::new("icacls")
        .arg(&sandbox.0)
        .arg("/grant")
        .arg("*S-1-1-0:(OI)(CI)F")
        .status()
        .expect("icacls");
    assert!(status.success());
    assert!(matches!(
        Store::open(&sandbox.0),
        Err(StoreError::Unavailable)
    ));
}
