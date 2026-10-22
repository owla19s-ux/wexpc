#![cfg(any(target_os = "linux", target_os = "android"))]
use std::fs;
use wex_access_store::{Grant, RegisteredReadError, Store, StoreError};
use wex_core::access::{Access, Denial};
use wex_local_resources::{DirectoryRegistry, FileReadError, Protection};

#[test]
fn stored_read_observes_revocation_and_unavailable_authority() {
    let root = std::env::temp_dir().join(format!("wex-stored-read-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let docs = root.join("docs");
    fs::create_dir(&docs).unwrap();
    fs::write(docs.join("file"), b"hello").unwrap();
    let result = std::panic::catch_unwind(|| {
        let mut r = DirectoryRegistry::new();
        let id = r.register(&docs, Protection::Protected).unwrap();
        let a = Store::open(root.join("private")).unwrap();
        let b = Store::open(root.join("private")).unwrap();
        a.set(
            0,
            Grant {
                directory_id: id.clone(),
                access: Access::Read,
                explicit_personal: true,
            },
        )
        .unwrap();
        assert_eq!(
            b.read_registered(&r, &id, "file", true),
            Ok(b"hello".to_vec())
        );
        a.revoke(1, &id).unwrap();
        assert_eq!(
            b.read_registered(&r, &id, "file", true),
            Err(RegisteredReadError::File(FileReadError::Denied(
                Denial::NoGrant
            )))
        );
        fs::write(root.join("private/grants.lock"), b"").unwrap();
        assert_eq!(
            b.read_registered(&r, &id, "file", true),
            Err(RegisteredReadError::Store(StoreError::Busy))
        );
    });
    fs::remove_dir_all(root).unwrap();
    result.unwrap();
}
