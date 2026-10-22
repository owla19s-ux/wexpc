#![cfg(any(target_os = "linux", target_os = "android"))]
use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use wex_core::access::{Access, Denial, DirectoryGrant};
use wex_local_resources::{DirectoryRegistry, FileReadError, Protection, ResourceError};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Sandbox(PathBuf);
impl Sandbox {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "wex-read-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn read(r: &DirectoryRegistry, id: &str, name: &str) -> Result<Vec<u8>, FileReadError> {
    r.read_file(
        id,
        name,
        true,
        &[DirectoryGrant {
            directory_id: id,
            access: Access::Read,
            explicit_personal: true,
        }],
    )
}

#[test]
fn real_binary_empty_and_exact_limit_reads() {
    let root = Sandbox::new();
    let mut r = DirectoryRegistry::new();
    let id = r.register(&root.0, Protection::Protected).unwrap();
    for bytes in [vec![], vec![0, 255, 1, 128], vec![42; 65536]] {
        fs::write(root.0.join("file"), &bytes).unwrap();
        assert_eq!(read(&r, &id, "file"), Ok(bytes));
    }
}

#[test]
fn oversized_file_returns_no_partial_bytes() {
    let root = Sandbox::new();
    fs::write(root.0.join("large"), vec![42; 65537]).unwrap();
    let mut r = DirectoryRegistry::new();
    let id = r.register(&root.0, Protection::General).unwrap();
    assert_eq!(read(&r, &id, "large"), Err(FileReadError::TooLarge));
}

#[test]
fn no_grant_personal_and_write_only_deny_before_target_probe() {
    let root = Sandbox::new();
    let mut r = DirectoryRegistry::new();
    let id = r.register(&root.0, Protection::Protected).unwrap();
    assert_eq!(
        r.read_file(&id, "missing", true, &[]),
        Err(FileReadError::Denied(Denial::NoGrant))
    );
    for (access, personal, denial) in [
        (Access::Read, false, Denial::PersonalNotAuthorized),
        (Access::Write, true, Denial::OperationNotAuthorized),
    ] {
        assert_eq!(
            r.read_file(
                &id,
                "missing",
                true,
                &[DirectoryGrant {
                    directory_id: &id,
                    access,
                    explicit_personal: personal
                }]
            ),
            Err(FileReadError::Denied(denial))
        );
    }
    assert_eq!(
        r.read_file(&id, "missing", false, &[]),
        Err(FileReadError::Denied(Denial::SystemUnavailable))
    );
}

#[test]
fn only_direct_names_are_accepted() {
    let root = Sandbox::new();
    let mut r = DirectoryRegistry::new();
    let id = r.register(&root.0, Protection::General).unwrap();
    for name in ["", ".", "..", "/file", "a/file", "../file", "file\0"] {
        assert_eq!(
            read(&r, &id, name),
            Err(FileReadError::Resource(ResourceError::InvalidPath))
        );
    }
}

#[test]
fn symlink_directory_and_hard_link_cannot_be_read() {
    let root = Sandbox::new();
    fs::write(root.0.join("file"), b"secret").unwrap();
    symlink(root.0.join("file"), root.0.join("link")).unwrap();
    fs::create_dir(root.0.join("folder")).unwrap();
    let mut r = DirectoryRegistry::new();
    let id = r.register(&root.0, Protection::General).unwrap();
    assert!(read(&r, &id, "link").is_err());
    assert!(read(&r, &id, "folder").is_err());
    fs::hard_link(root.0.join("file"), root.0.join("hard")).unwrap();
    assert!(read(&r, &id, "hard").is_err());
}

#[test]
fn fifo_is_rejected_without_blocking() {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let root = Sandbox::new();
    let path = root.0.join("pipe");
    let text = CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: NUL-terminated test path, no retained pointer.
    assert_eq!(unsafe { libc::mkfifo(text.as_ptr(), 0o600) }, 0);
    let mut r = DirectoryRegistry::new();
    let id = r.register(&root.0, Protection::General).unwrap();
    assert!(read(&r, &id, "pipe").is_err());
}

#[test]
fn replaced_registered_root_is_not_read() {
    let root = Sandbox::new();
    let dir = root.0.join("docs");
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("file"), b"original").unwrap();
    let mut r = DirectoryRegistry::new();
    let id = r.register(&dir, Protection::General).unwrap();
    fs::rename(&dir, root.0.join("old")).unwrap();
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("file"), b"replacement").unwrap();
    assert_eq!(
        read(&r, &id, "file"),
        Err(FileReadError::Resource(ResourceError::Unavailable))
    );
}
