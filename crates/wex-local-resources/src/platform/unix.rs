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
