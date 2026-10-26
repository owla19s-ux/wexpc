//! Shared contract: every intermediate directory is opened relative to a
//! pinned parent. Windows executes this contract on its native backend too.
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
use wex_core::access::{Access, Denial, DirectoryGrant};
use wex_local_resources::{DirectoryRegistry, FileReadError, Protection};
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "wex-relative-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn reads_nested_bytes_from_the_authorized_root() {
    let dir = Temp::new();
    fs::create_dir_all(dir.0.join("src/nested")).unwrap();
    fs::write(dir.0.join("src/nested/a.rs"), b"exact\0bytes").unwrap();
    let mut registry = DirectoryRegistry::new();
    let id = registry.register(&dir.0, Protection::General).unwrap();
    let grant = [DirectoryGrant {
        directory_id: &id,
        access: Access::Read,
        explicit_personal: false,
    }];
    let snap = registry
        .read_relative_file_snapshot(&id, "src/nested/a.rs", true, &grant, &[])
        .unwrap();
    assert_eq!(snap.bytes, b"exact\0bytes");
}
#[test]
fn denial_precedes_nested_missing_path_probes() {
    let dir = Temp::new();
    let mut registry = DirectoryRegistry::new();
    let id = registry.register(&dir.0, Protection::General).unwrap();
    assert_eq!(
        registry
            .read_relative_file_snapshot(&id, "missing/child", true, &[], &[])
            .unwrap_err(),
        FileReadError::Denied(Denial::NoGrant)
    );
}
#[test]
fn rejects_traversal_links_and_protected_child() {
    let dir = Temp::new();
    fs::create_dir(dir.0.join("private")).unwrap();
    fs::write(dir.0.join("private/secret"), "secret").unwrap();
    let mut registry = DirectoryRegistry::new();
    let id = registry.register(&dir.0, Protection::General).unwrap();
    registry
        .register(dir.0.join("private"), Protection::Protected)
        .unwrap();
    let grant = [DirectoryGrant {
        directory_id: &id,
        access: Access::Read,
        explicit_personal: false,
    }];
    for path in [
        "../outside",
        "src/../private/secret",
        "private/secret",
        "/absolute",
        "a//b",
        "a\\b",
        "a/./b",
    ] {
        assert!(
            registry
                .read_relative_file_snapshot(&id, path, true, &grant, &[])
                .is_err(),
            "{path}"
        );
    }
}
#[cfg(unix)]
#[test]
fn never_traverses_symlink_ancestors_or_reads_hardlink_aliases() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let dir = Temp::new();
    let outside = Temp::new();
    fs::write(outside.0.join("secret"), "secret").unwrap();
    symlink(&outside.0, dir.0.join("linked")).unwrap();
    fs::create_dir(dir.0.join("src")).unwrap();
    fs::hard_link(outside.0.join("secret"), dir.0.join("src/alias")).unwrap();
    fs::write(dir.0.join("src/executable"), "script").unwrap();
    fs::set_permissions(
        dir.0.join("src/executable"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    let mut registry = DirectoryRegistry::new();
    let id = registry.register(&dir.0, Protection::General).unwrap();
    let grant = [DirectoryGrant {
        directory_id: &id,
        access: Access::Read,
        explicit_personal: false,
    }];
    for path in ["linked/secret", "src/alias"] {
        assert!(registry
            .read_relative_file_snapshot(&id, path, true, &grant, &[])
            .is_err());
    }
    assert!(
        registry
            .read_relative_file_snapshot(&id, "src/executable", true, &grant, &[])
            .unwrap()
            .executable
    );
}

#[test]
fn enforces_limits_and_internal_exclusions_for_nested_files() {
    let dir = Temp::new();
    fs::create_dir_all(dir.0.join("src")).unwrap();
    fs::write(dir.0.join("src/large"), vec![1u8; 65537]).unwrap();
    let mut registry = DirectoryRegistry::new();
    let id = registry.register(&dir.0, Protection::General).unwrap();
    let grant = [DirectoryGrant {
        directory_id: &id,
        access: Access::Read,
        explicit_personal: false,
    }];
    assert_eq!(
        registry
            .read_relative_file_snapshot(&id, "src/large", true, &grant, &[])
            .unwrap_err(),
        FileReadError::TooLarge
    );
    assert!(registry
        .read_relative_file_snapshot(
            &id,
            "src/large",
            true,
            &grant,
            &[dir.0.join("src").as_path()]
        )
        .is_err());
    assert_eq!(
        registry
            .read_relative_file_snapshot(&id, "src/large", false, &grant, &[])
            .unwrap_err(),
        FileReadError::Denied(Denial::SystemUnavailable)
    );
}

#[cfg(unix)]
#[test]
fn concurrent_symlink_parent_swaps_never_expose_outside_bytes() {
    use std::os::unix::fs::symlink;
    let dir = Temp::new();
    let outside = Temp::new();
    fs::create_dir(dir.0.join("src")).unwrap();
    fs::write(dir.0.join("src/value"), "allowed").unwrap();
    fs::write(outside.0.join("value"), "forbidden").unwrap();
    let mut registry = DirectoryRegistry::new();
    let id = registry.register(&dir.0, Protection::General).unwrap();
    let grant = [DirectoryGrant {
        directory_id: &id,
        access: Access::Read,
        explicit_personal: false,
    }];
    let observed = std::thread::scope(|scope| {
        scope.spawn(|| {
            for _ in 0..300 {
                fs::rename(dir.0.join("src"), dir.0.join("held")).unwrap();
                symlink(&outside.0, dir.0.join("src")).unwrap();
                std::thread::yield_now();
                fs::remove_file(dir.0.join("src")).unwrap();
                fs::rename(dir.0.join("held"), dir.0.join("src")).unwrap();
            }
        });
        (0..300)
            .filter_map(|_| {
                registry
                    .read_relative_file_snapshot(&id, "src/value", true, &grant, &[])
                    .ok()
            })
            .collect::<Vec<_>>()
    });
    for result in observed {
        assert_eq!(result.bytes, b"allowed");
    }
    assert_eq!(
        registry
            .read_relative_file_snapshot(&id, "src/value", true, &grant, &[])
            .unwrap()
            .bytes,
        b"allowed"
    );
}
