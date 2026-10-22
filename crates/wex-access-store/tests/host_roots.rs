#![cfg(any(target_os = "linux", target_os = "android"))]
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use wex_access_store::{Grant, RegisteredAccessError, RegisteredReadError, Store, StoreError};
use wex_core::access::{Access, Decision, Denial};
use wex_local_resources::{DirectoryRegistry, FileReadError, Protection, ResourceError};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Sandbox(PathBuf);
impl Sandbox {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "wex-host-roots-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        for name in ["private", "private-other", "public"] {
            fs::create_dir(root.join(name)).unwrap();
            fs::write(root.join(name).join("file"), b"data").unwrap();
        }
        Self(root)
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn grant(store: &Store, registry: &mut DirectoryRegistry, path: &Path) -> String {
    let id = registry.register(path, Protection::Protected).unwrap();
    let revision = store.load().unwrap().revision;
    store
        .set(
            revision,
            Grant {
                directory_id: id.clone(),
                access: Access::ReadWrite,
                explicit_personal: true,
            },
        )
        .unwrap();
    id
}
fn denied(store: &Store, registry: &DirectoryRegistry, id: &str, leaf: &str) {
    assert_eq!(
        store.check_registered(registry, id, leaf, true, Access::Read),
        Err(RegisteredAccessError::Resource(
            ResourceError::InternalDirectory
        ))
    );
    assert_eq!(
        store.read_registered(registry, id, leaf, true),
        Err(RegisteredReadError::File(FileReadError::Resource(
            ResourceError::InternalDirectory
        )))
    );
}
#[test]
fn host_private_root_is_applied_to_existing_store_access_methods() {
    let s = Sandbox::new();
    let private = s.path("private");
    let store = Store::open_with_internal_roots(s.path("authority"), &[&private]).unwrap();
    let mut registry = DirectoryRegistry::new();
    let id = grant(&store, &mut registry, &private);
    denied(&store, &registry, &id, "file");
    drop(store);
    let reopened = Store::open_with_internal_roots(s.path("authority"), &[&private]).unwrap();
    denied(&reopened, &registry, &id, "file");
}
#[test]
fn own_authority_root_cannot_be_removed_by_host_configuration() {
    let s = Sandbox::new();
    let private = s.path("private");
    let authority = s.path("authority");
    let store = Store::open_with_internal_roots(&authority, &[&private]).unwrap();
    let mut registry = DirectoryRegistry::new();
    let id = grant(&store, &mut registry, &authority);
    denied(&store, &registry, &id, "grants.json");
}
#[test]
fn store_rejects_ancestor_and_child_of_its_authority_root() {
    let s = Sandbox::new();
    let authority = s.path("authority");
    let store = Store::open(&authority).unwrap();
    fs::create_dir(authority.join("child")).unwrap();
    for path in [&s.0, &authority.join("child")] {
        let mut registry = DirectoryRegistry::new();
        let id = grant(&store, &mut registry, path);
        denied(&store, &registry, &id, "missing");
    }
}
#[test]
fn unrelated_and_component_neighbor_resources_remain_readable() {
    let s = Sandbox::new();
    let private = s.path("private");
    let store = Store::open_with_internal_roots(s.path("authority"), &[&private]).unwrap();
    for name in ["public", "private-other"] {
        let mut registry = DirectoryRegistry::new();
        let id = grant(&store, &mut registry, &s.path(name));
        assert_eq!(
            store.check_registered(&registry, &id, "file", true, Access::Read),
            Ok(Decision::Allowed)
        );
        assert_eq!(
            store.read_registered(&registry, &id, "file", true),
            Ok(b"data".to_vec())
        );
    }
}
#[test]
fn disappearing_host_root_denies_access_until_it_is_available_again() {
    let s = Sandbox::new();
    let private = s.path("private");
    let store = Store::open_with_internal_roots(s.path("authority"), &[&private]).unwrap();
    let mut registry = DirectoryRegistry::new();
    let id = grant(&store, &mut registry, &s.path("public"));
    assert_eq!(
        store.read_registered(&registry, &id, "file", true),
        Ok(b"data".to_vec())
    );
    fs::rename(&private, s.path("moved")).unwrap();
    assert_eq!(
        store.check_registered(&registry, &id, "file", true, Access::Read),
        Err(RegisteredAccessError::Resource(ResourceError::Unavailable))
    );
    assert_eq!(
        store.read_registered(&registry, &id, "file", true),
        Err(RegisteredReadError::File(FileReadError::Resource(
            ResourceError::Unavailable
        )))
    );
    fs::rename(s.path("moved"), &private).unwrap();
    assert_eq!(
        store.read_registered(&registry, &id, "file", true),
        Ok(b"data".to_vec())
    );
}
#[test]
fn grant_and_system_denials_precede_host_root_metadata() {
    let s = Sandbox::new();
    let missing = s.path("missing");
    let store = Store::open_with_internal_roots(s.path("authority"), &[&missing]).unwrap();
    let mut registry = DirectoryRegistry::new();
    let id = registry
        .register(s.path("public"), Protection::Protected)
        .unwrap();
    assert_eq!(
        store.check_registered(&registry, &id, "file", true, Access::Read),
        Ok(Decision::Denied(Denial::NoGrant))
    );
    assert_eq!(
        store.read_registered(&registry, &id, "file", true),
        Err(RegisteredReadError::File(FileReadError::Denied(
            Denial::NoGrant
        )))
    );
    assert_eq!(
        store.read_registered(&registry, &id, "file", false),
        Err(RegisteredReadError::File(FileReadError::Denied(
            Denial::SystemUnavailable
        )))
    );
}
#[test]
fn fifteen_host_roots_fit_alongside_the_mandatory_authority_root() {
    let s = Sandbox::new();
    let private = s.path("private");
    let roots: Vec<&Path> = vec![private.as_path(); 15];
    let store = Store::open_with_internal_roots(s.path("authority"), &roots).unwrap();
    let mut registry = DirectoryRegistry::new();
    let id = grant(&store, &mut registry, &s.path("public"));
    assert_eq!(
        store.read_registered(&registry, &id, "file", true),
        Ok(b"data".to_vec())
    );
    let sixteen: Vec<&Path> = vec![private.as_path(); 16];
    let extra = s.path("excess-authority");
    assert!(matches!(
        Store::open_with_internal_roots(&extra, &sixteen),
        Err(StoreError::LimitExceeded)
    ));
    assert!(
        !extra.exists(),
        "invalid configuration must not create a store"
    );
}
#[test]
fn invalid_host_path_configuration_has_no_store_side_effects() {
    use std::os::unix::ffi::OsStringExt;
    let s = Sandbox::new();
    let long = PathBuf::from(format!("/{}", "x".repeat(4096)));
    let non_utf8 = PathBuf::from(std::ffi::OsString::from_vec(vec![b'/', 0xff]));
    let paths = [
        Path::new("relative"),
        Path::new("/bad\0root"),
        long.as_path(),
        non_utf8.as_path(),
    ];
    for (i, path) in paths.iter().enumerate() {
        let authority = s.path(&format!("invalid-{i}"));
        assert!(matches!(
            Store::open_with_internal_roots(&authority, &[*path]),
            Err(StoreError::Unavailable)
        ));
        assert!(!authority.exists());
    }
}
