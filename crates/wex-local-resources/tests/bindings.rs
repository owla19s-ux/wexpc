#![cfg(any(target_os = "linux", target_os = "android"))]

use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use wex_core::access::{Access, Decision, Denial, DirectoryGrant};
use wex_local_resources::{DirectoryRegistry, Protection, ResourceError};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Sandbox(PathBuf);
impl Sandbox {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "wex-resources-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::create_dir(path.join("docs")).unwrap();
        fs::write(path.join("docs/file"), b"private content").unwrap();
        Self(path)
    }
    fn docs(&self) -> PathBuf {
        self.0.join("docs")
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn allowed(
    registry: &DirectoryRegistry,
    id: &str,
    path: &str,
    personal: bool,
) -> Result<Decision, ResourceError> {
    registry.check(
        id,
        path,
        true,
        Access::Read,
        &[DirectoryGrant {
            directory_id: id,
            access: Access::Read,
            explicit_personal: personal,
        }],
    )
}

#[test]
fn registration_generates_opaque_identity_and_protects_by_default() {
    let root = Sandbox::new();
    let mut registry = DirectoryRegistry::new();
    let id = registry
        .register(root.docs(), Protection::default())
        .unwrap();
    assert_eq!(id.len(), 64);
    assert!(id.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(
        allowed(&registry, &id, "file", false),
        Ok(Decision::Denied(Denial::PersonalNotAuthorized))
    );
    assert_eq!(allowed(&registry, &id, "file", true), Ok(Decision::Allowed));
    assert_eq!(
        registry.check(&id, "file", true, Access::Read, &[]),
        Ok(Decision::Denied(Denial::NoGrant))
    );
}

#[test]
fn general_binding_still_requires_exact_grant_and_operation() {
    let root = Sandbox::new();
    let mut registry = DirectoryRegistry::new();
    let id = registry.register(root.docs(), Protection::General).unwrap();
    assert_eq!(
        allowed(&registry, &id, "file", false),
        Ok(Decision::Allowed)
    );
    assert_eq!(
        registry.check(
            &id,
            "file",
            true,
            Access::Write,
            &[DirectoryGrant {
                directory_id: &id,
                access: Access::Read,
                explicit_personal: false
            }]
        ),
        Ok(Decision::Denied(Denial::OperationNotAuthorized))
    );
    assert_eq!(
        registry.check("invented", "file", true, Access::Read, &[]),
        Err(ResourceError::UnknownDirectory)
    );
}

#[test]
fn system_denial_precedes_resource_metadata_io() {
    let root = Sandbox::new();
    let mut registry = DirectoryRegistry::new();
    let id = registry.register(root.docs(), Protection::General).unwrap();
    fs::remove_dir_all(root.docs()).unwrap();
    assert_eq!(
        registry.check(&id, "missing", false, Access::Read, &[]),
        Ok(Decision::Denied(Denial::SystemUnavailable))
    );
    assert_eq!(
        allowed(&registry, &id, "file", true),
        Err(ResourceError::Unavailable)
    );
}

#[test]
fn invalid_relative_paths_never_escape_the_binding() {
    let root = Sandbox::new();
    let mut registry = DirectoryRegistry::new();
    let id = registry.register(root.docs(), Protection::General).unwrap();
    for path in [
        "/etc",
        "..",
        "../docs/file",
        "./file",
        "a/../file",
        "a//file",
        "file/",
        "file\0",
        &"a".repeat(4097),
    ] {
        assert_eq!(
            allowed(&registry, &id, path, true),
            Err(ResourceError::InvalidPath)
        );
    }
    assert_eq!(allowed(&registry, &id, "", false), Ok(Decision::Allowed));
}

#[test]
fn symlink_escape_and_prefix_sibling_are_denied() {
    let root = Sandbox::new();
    fs::create_dir(root.0.join("docs-private")).unwrap();
    fs::write(root.0.join("docs-private/secret"), b"secret").unwrap();
    symlink(root.0.join("docs-private"), root.docs().join("outside")).unwrap();
    let mut registry = DirectoryRegistry::new();
    let id = registry.register(root.docs(), Protection::General).unwrap();
    assert_eq!(
        allowed(&registry, &id, "outside/secret", false),
        Err(ResourceError::OutsideDirectory)
    );
}

#[test]
fn physical_alias_cannot_register_again_with_weaker_protection() {
    let root = Sandbox::new();
    symlink(root.docs(), root.0.join("alias")).unwrap();
    let mut registry = DirectoryRegistry::new();
    registry
        .register(root.docs(), Protection::Protected)
        .unwrap();
    assert_eq!(
        registry.register(root.0.join("alias"), Protection::General),
        Err(ResourceError::AlreadyRegistered)
    );
}

#[test]
fn root_replacement_does_not_retarget_an_existing_grant() {
    let root = Sandbox::new();
    let mut registry = DirectoryRegistry::new();
    let id = registry.register(root.docs(), Protection::General).unwrap();
    fs::rename(root.docs(), root.0.join("kept-original")).unwrap();
    fs::create_dir(root.docs()).unwrap();
    fs::write(root.docs().join("file"), b"replacement").unwrap();
    assert_eq!(
        allowed(&registry, &id, "file", true),
        Err(ResourceError::Unavailable)
    );
}

#[test]
fn parent_personal_consent_cannot_replace_child_protection_consent() {
    let root = Sandbox::new();
    fs::create_dir(root.docs().join("personal")).unwrap();
    fs::write(root.docs().join("personal/secret"), b"secret").unwrap();
    let mut registry = DirectoryRegistry::new();
    let parent = registry.register(root.docs(), Protection::General).unwrap();
    let child = registry
        .register(root.docs().join("personal"), Protection::Protected)
        .unwrap();
    assert_eq!(
        allowed(&registry, &parent, "personal/secret", true),
        Err(ResourceError::ProtectedBoundary)
    );
    assert_eq!(
        allowed(&registry, &parent, "", true),
        Err(ResourceError::ProtectedBoundary)
    );
    assert_eq!(
        allowed(&registry, &parent, "file", false),
        Ok(Decision::Allowed)
    );
    assert_eq!(
        allowed(&registry, &child, "secret", false),
        Ok(Decision::Denied(Denial::PersonalNotAuthorized))
    );
    assert_eq!(
        allowed(&registry, &child, "secret", true),
        Ok(Decision::Allowed)
    );
}

#[test]
fn general_child_cannot_override_a_protected_ancestor() {
    let root = Sandbox::new();
    fs::create_dir(root.docs().join("child")).unwrap();
    fs::write(root.docs().join("child/file"), b"secret").unwrap();
    let mut registry = DirectoryRegistry::new();
    registry
        .register(root.docs(), Protection::Protected)
        .unwrap();
    let id = registry
        .register(root.docs().join("child"), Protection::General)
        .unwrap();
    assert_eq!(
        allowed(&registry, &id, "file", true),
        Err(ResourceError::ProtectedBoundary)
    );
}

#[test]
fn missing_protected_root_does_not_silently_remove_its_protection() {
    let root = Sandbox::new();
    fs::create_dir(root.docs().join("personal")).unwrap();
    let mut registry = DirectoryRegistry::new();
    let id = registry.register(root.docs(), Protection::General).unwrap();
    registry
        .register(root.docs().join("personal"), Protection::Protected)
        .unwrap();
    fs::remove_dir(root.docs().join("personal")).unwrap();
    assert_eq!(
        allowed(&registry, &id, "file", false),
        Err(ResourceError::Unavailable)
    );
}

#[test]
fn hard_link_alias_of_personal_file_is_not_accepted_as_general_data() {
    let root = Sandbox::new();
    fs::create_dir(root.docs().join("personal")).unwrap();
    fs::write(root.docs().join("personal/secret"), b"secret").unwrap();
    fs::hard_link(
        root.docs().join("personal/secret"),
        root.docs().join("alias"),
    )
    .unwrap();
    let mut registry = DirectoryRegistry::new();
    let id = registry.register(root.docs(), Protection::General).unwrap();
    registry
        .register(root.docs().join("personal"), Protection::Protected)
        .unwrap();
    assert_eq!(
        allowed(&registry, &id, "alias", true),
        Err(ResourceError::UnsupportedTarget)
    );
}

#[test]
fn a_new_registry_cannot_reactivate_old_ids_for_the_same_path() {
    let root = Sandbox::new();
    let mut first = DirectoryRegistry::new();
    let old = first.register(root.docs(), Protection::General).unwrap();
    let mut second = DirectoryRegistry::new();
    let new = second.register(root.docs(), Protection::General).unwrap();
    assert_ne!(old, new);
    assert_eq!(
        allowed(&second, &old, "file", true),
        Err(ResourceError::UnknownDirectory)
    );
    assert_eq!(
        second.check(
            &new,
            "file",
            true,
            Access::Read,
            &[DirectoryGrant {
                directory_id: &old,
                access: Access::ReadWrite,
                explicit_personal: true
            }]
        ),
        Ok(Decision::Denied(Denial::NoGrant))
    );
}

#[test]
fn missing_file_relative_root_and_regular_file_cannot_be_registered() {
    let root = Sandbox::new();
    let mut registry = DirectoryRegistry::new();
    for path in [
        root.docs().join("missing"),
        root.docs().join("file"),
        PathBuf::from("relative"),
    ] {
        assert!(registry.register(path, Protection::Protected).is_err());
    }
}

#[test]
fn registry_is_bounded_without_discarding_existing_bindings() {
    let root = Sandbox::new();
    let mut registry = DirectoryRegistry::new();
    for i in 0..128 {
        let path = root.0.join(format!("d{i}"));
        fs::create_dir(&path).unwrap();
        registry.register(path, Protection::General).unwrap();
    }
    assert_eq!(
        registry.register(root.docs(), Protection::General),
        Err(ResourceError::LimitExceeded)
    );
}

#[test]
fn denied_requests_do_not_probe_missing_targets_or_personal_metadata() {
    let root = Sandbox::new();
    let mut registry = DirectoryRegistry::new();
    let id = registry
        .register(root.docs(), Protection::Protected)
        .unwrap();
    assert_eq!(
        registry.check(&id, "missing", true, Access::Read, &[]),
        Ok(Decision::Denied(Denial::NoGrant))
    );
    assert_eq!(
        allowed(&registry, &id, "missing", false),
        Ok(Decision::Denied(Denial::PersonalNotAuthorized))
    );
    fs::remove_dir_all(root.docs()).unwrap();
    assert_eq!(
        registry.check(&id, "file", true, Access::Read, &[]),
        Ok(Decision::Denied(Denial::NoGrant))
    );
}

#[test]
fn fifo_registration_is_rejected_without_opening() {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    const CHILD_ROOT: &str = "WEX_FIFO_REGRESSION_ROOT";
    if let Some(path) = std::env::var_os(CHILD_ROOT) {
        assert_eq!(
            DirectoryRegistry::new().register(PathBuf::from(path), Protection::Protected),
            Err(ResourceError::UnsupportedTarget)
        );
        return;
    }
    let root = Sandbox::new();
    let fifo = root.0.join("pipe");
    let path = CString::new(fifo.as_os_str().as_bytes()).unwrap();
    // SAFETY: valid NUL-terminated test path, no retained pointer.
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "fifo_registration_is_rejected_without_opening"])
        .env(CHILD_ROOT, &fifo)
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("register blocked opening a FIFO instead of rejecting its type");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn protected_child_denial_precedes_target_existence_probe() {
    let root = Sandbox::new();
    fs::create_dir(root.docs().join("personal")).unwrap();
    let mut registry = DirectoryRegistry::new();
    let parent = registry.register(root.docs(), Protection::General).unwrap();
    registry
        .register(root.docs().join("personal"), Protection::Protected)
        .unwrap();
    assert_eq!(
        allowed(&registry, &parent, "personal/missing", true),
        Err(ResourceError::ProtectedBoundary)
    );
}

#[test]
fn dangling_target_symlink_is_denied_without_following_its_destination() {
    let root = Sandbox::new();
    symlink(root.0.join("outside-missing"), root.docs().join("alias")).unwrap();
    let mut registry = DirectoryRegistry::new();
    let id = registry.register(root.docs(), Protection::General).unwrap();
    assert_eq!(
        allowed(&registry, &id, "alias", false),
        Err(ResourceError::OutsideDirectory)
    );
}
