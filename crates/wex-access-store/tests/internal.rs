#![cfg(any(target_os = "linux", target_os = "android"))]
use std::fs;
use wex_access_store::{Grant, Store};
use wex_core::access::Access;
use wex_local_resources::{DirectoryRegistry, Protection};

#[test]
fn store_private_authority_is_never_a_readable_user_resource() {
    let path = std::env::temp_dir().join(format!("wex-internal-root-{}", std::process::id()));
    let store = Store::open(&path).unwrap();
    let mut registry = DirectoryRegistry::new();
    let id = registry.register(&path, Protection::General).unwrap();
    store
        .set(
            0,
            Grant {
                directory_id: id.clone(),
                access: Access::ReadWrite,
                explicit_personal: true,
            },
        )
        .unwrap();
    // Do not print the snapshot or rely on the error text: no bytes may escape.
    let read_denied = store
        .read_registered(&registry, &id, "grants.json", true)
        .is_err();
    let check_denied = store
        .check_registered(&registry, &id, "grants.json", true, Access::Read)
        .is_err();
    fs::remove_dir_all(path).unwrap();
    assert!(
        read_denied,
        "private authorization snapshot was returned as user data"
    );
    assert!(
        check_denied,
        "private authorization directory passed preflight"
    );
}
