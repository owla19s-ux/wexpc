//! Linux / Android backend.
//!
//! Directories are pinned with `openat(O_PATH|O_NOFOLLOW)`-style handles and a
//! leaf is reopened through `/proc/self/fd`, so no user supplied path is ever
//! resolved twice. Mount identity and inode identity are compared around every
//! read.

use super::*;
use crate::FileReadError;
use std::ffi::CString;
use std::fs;
use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd};

pub(super) fn open_directory(path: &Path) -> Result<File, ResourceError> {
    use std::os::unix::fs::OpenOptionsExt;
    // Reject non-directories (FIFOs, sockets, devices, symlinks) before any
    // open: opening a FIFO would block, and the error code must stay
    // UnsupportedTarget rather than collapsing into Unavailable.
    let metadata = fs::symlink_metadata(path).map_err(|_| ResourceError::Unavailable)?;
    if !metadata.is_dir() || is_link(&metadata) {
        return Err(ResourceError::UnsupportedTarget);
    }
    // Kernel-enforced type/no-follow also covers a replacement after stat.
    fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| ResourceError::Unavailable)
}

pub(super) fn identity_of_file(file: &File) -> Result<Identity, ResourceError> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata().map_err(|_| ResourceError::Unavailable)?;
    Ok(Identity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

pub(super) fn identity_of_path(path: &Path) -> Result<Identity, ResourceError> {
    use std::os::unix::fs::MetadataExt;
    let metadata = fs::symlink_metadata(path).map_err(|_| ResourceError::Unavailable)?;
    Ok(Identity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

pub(super) fn is_link(metadata: &Metadata) -> bool {
    metadata.file_type().is_symlink()
}

/// Hard-linked files can alias into a protected directory; the caller rejects
/// anything that is not a single-link regular file.
pub(super) fn has_single_link(metadata: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    metadata.nlink() == 1
}

pub(super) fn valid_leaf(leaf: &str) -> bool {
    !leaf.is_empty() && leaf != "." && leaf != ".." && !leaf.contains('/')
}

pub(super) fn random_id() -> Result<String, ResourceError> {
    let mut random = [0u8; 32];
    File::open("/dev/urandom")
        .and_then(|mut source| source.read_exact(&mut random))
        .map_err(|_| ResourceError::Unavailable)?;
    Ok(super::hex(random))
}

pub(super) fn read_leaf(root: &File, leaf: &str) -> Result<Vec<u8>, FileReadError> {
    let pinned = pin_leaf(root, leaf)?;
    let before = pinned.metadata().map_err(|_| unavailable())?;
    regular(&before)?;
    if before.len() > 65536 {
        return Err(FileReadError::TooLarge);
    }
    // Reopen only the pinned regular object through trusted procfs, never the
    // user-provided filesystem path.
    let file =
        File::open(format!("/proc/self/fd/{}", pinned.as_raw_fd())).map_err(|_| unavailable())?;
    let opened = file.metadata().map_err(|_| unavailable())?;
    if stamp(&before) != stamp(&opened) {
        return Err(FileReadError::Changed);
    }
    let mut bytes = Vec::new();
    (&file)
        .take(65537)
        .read_to_end(&mut bytes)
        .map_err(|_| unavailable())?;
    if bytes.len() > 65536 {
        return Err(FileReadError::TooLarge);
    }
    let after = file.metadata().map_err(|_| unavailable())?;
    regular(&after)?;
    if stamp(&before) != stamp(&after) || bytes.len() as u64 != after.len() {
        return Err(FileReadError::Changed);
    }
    Ok(bytes)
}

/// Kernel-enforced no-follow, exclusive creation relative to the pinned root.
/// A failed write may leave a partial file; callers must treat it as unknown.
pub(super) fn create_leaf(root: &File, leaf: &str, bytes: &[u8]) -> Result<(), FileReadError> {
    use std::io::Write;
    if !valid_leaf(leaf) || bytes.len() > 65536 {
        return Err(FileReadError::Resource(ResourceError::InvalidPath));
    }
    let name = CString::new(leaf)
        .map_err(|_| FileReadError::Resource(ResourceError::InvalidPath))?;
    // SAFETY: live directory fd; only a single, NUL-terminated component.
    let fd = unsafe { libc::openat(root.as_raw_fd(), name.as_ptr(),
        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        0o600) };
    if fd < 0 { return Err(unavailable()); }
    // SAFETY: openat returned a newly owned fd.
    let mut file = unsafe { File::from_raw_fd(fd) };
    file.write_all(bytes).map_err(|_| FileReadError::Changed)?;
    file.sync_all().map_err(|_| FileReadError::Changed)?;
    // A concurrent rename can detach the new inode. Re-check the exact
    // pinned directory entry without following links before reporting success.
    let pinned = pin_leaf(root, leaf)?;
    if stamp(&file.metadata().map_err(|_| FileReadError::Changed)?)
        != stamp(&pinned.metadata().map_err(|_| FileReadError::Changed)?)
    {
        return Err(FileReadError::Changed);
    }
    Ok(())
}

/// Compare-and-swap existing file contents through its pinned object.
/// The existing file must be a single-linked ordinary leaf on the root mount.
/// Never truncate before verifying the previously read content bytes.
/// This is an in-place write and is deliberately NOT yet exposed to AI tools:
/// callers needing atomic replacement must use a staging/rename transaction.
pub(super) fn prepare_replace_leaf(root: &File, leaf: &str, expected: &[u8]) -> Result<(), FileReadError> {
    if !valid_leaf(leaf) || expected.len() > 65536 {
        return Err(FileReadError::Resource(ResourceError::InvalidPath));
    }
    // Use the exact hardened read path to check the object and version.
    let actual = read_leaf(root, leaf)?;
    if actual != expected { return Err(FileReadError::Changed); }
    Ok(())
}

fn unavailable() -> FileReadError {
    FileReadError::Resource(ResourceError::Unavailable)
}

fn pin_leaf(root: &File, leaf: &str) -> Result<File, FileReadError> {
    let name =
        CString::new(leaf).map_err(|_| FileReadError::Resource(ResourceError::InvalidPath))?;
    // SAFETY: live root FD and NUL-terminated single-component name.
    // O_PATH does not perform content/device open; O_NOFOLLOW pins links
    // themselves so regular() rejects them. No O_CREAT or write flags.
    let fd = unsafe {
        libc::openat(
            root.as_raw_fd(),
            name.as_ptr(),
            libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(unavailable());
    }
    // SAFETY: successful openat returned a new descriptor, owned once.
    let pinned = unsafe { File::from_raw_fd(fd) };
    regular(&pinned.metadata().map_err(|_| unavailable())?)?;
    // Direct-name grammar prevents traversal. Mount identity separately
    // rejects file bind mounts, including aliases on the same device.
    if mount_id(root)? != mount_id(&pinned)? {
        return Err(FileReadError::Resource(ResourceError::OutsideDirectory));
    }
    Ok(pinned)
}

fn mount_id(file: &File) -> Result<u64, FileReadError> {
    let info =
        File::open(format!("/proc/self/fdinfo/{}", file.as_raw_fd())).map_err(|_| unavailable())?;
    let mut bytes = Vec::new();
    info.take(4097)
        .read_to_end(&mut bytes)
        .map_err(|_| unavailable())?;
    if bytes.len() > 4096 {
        return Err(unavailable());
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| unavailable())?;
    let mut values = text.lines().filter_map(|line| line.strip_prefix("mnt_id:"));
    let id = values
        .next()
        .ok_or_else(unavailable)?
        .trim()
        .parse::<u64>()
        .map_err(|_| unavailable())?;
    if values.next().is_some() {
        return Err(unavailable());
    }
    Ok(id)
}

fn regular(metadata: &Metadata) -> Result<(), FileReadError> {
    use std::os::unix::fs::MetadataExt;
    if !metadata.is_file() || metadata.nlink() != 1 {
        return Err(FileReadError::Resource(ResourceError::UnsupportedTarget));
    }
    Ok(())
}

fn stamp(m: &Metadata) -> (u64, u64, u64, u64, i64, i64, i64, i64) {
    use std::os::unix::fs::MetadataExt;
    (
        m.dev(),
        m.ino(),
        m.nlink(),
        m.len(),
        m.mtime(),
        m.mtime_nsec(),
        m.ctime(),
        m.ctime_nsec(),
    )
}
