#![cfg(any(target_os = "linux", target_os = "android"))]
use std::fs;
use wex_access_store::{Grant, RegisteredAccessError, Store, StoreError};
use wex_core::access::{Access, Decision, Denial};
use wex_local_resources::{DirectoryRegistry, Protection, ResourceError};

#[test]
fn registered_checks_reload_revocation_and_deny_fabricated_identity() {
    let root = std::env::temp_dir().join(format!("wex-bound-store-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let docs = root.join("docs");
    fs::create_dir(&docs).unwrap();
    fs::write(docs.join("file"), b"content").unwrap();
    let result = std::panic::catch_unwind(|| {
        let mut registry = DirectoryRegistry::new();
        let id = registry.register(&docs, Protection::Protected).unwrap();
        let first = Store::open(root.join("private")).unwrap();
        let second = Store::open(root.join("private")).unwrap();
        first
            .set(
                0,
                Grant {
                    directory_id: id.clone(),
                    access: Access::Read,
                    explicit_personal: true,
                },
            )
            .unwrap();
        assert_eq!(
            second.check_registered(&registry, &id, "file", true, Access::Read),
            Ok(Decision::Allowed)
        );
        assert_eq!(
            second.check_registered(&registry, "fabricated", "file", true, Access::Read),
            Err(RegisteredAccessError::Resource(
                ResourceError::UnknownDirectory
            ))
        );
        first.revoke(1, &id).unwrap();
        assert_eq!(
            second.check_registered(&registry, &id, "file", true, Access::Read),
            Ok(Decision::Denied(Denial::NoGrant))
        );
        fs::write(root.join("private/grants.lock"), b"").unwrap();
        assert_eq!(
            second.check_registered(&registry, &id, "file", true, Access::Read),
            Err(RegisteredAccessError::Store(StoreError::Busy))
        );
    });
    fs::remove_dir_all(root).unwrap();
    result.unwrap();
}
