//! Platform backends.
//!
//! Every backend implements the same security contract:
//!
//! 1. A directory is pinned by an **open handle**, never by a path.
//! 2. A leaf below it is opened **relative to that handle**, so the
//!    user-supplied path is never resolved a second time.
//! 3. Links / reparse points are rejected at every component; nothing is
//!    followed to discover whether a destination exists.
//! 4. Object identity (device+inode, volume serial+file id) is compared
//!    before and after a read.
//! 5. A leaf must be a single-linked regular file of at most 64 KiB.
//!
//! The public API of this crate stays platform neutral; only this module
//! knows which operating system is underneath.

use std::fs::{File, Metadata};
use std::path::Path;

use crate::{FileReadError, Identity, ResourceError};

#[cfg(any(target_os = "linux", target_os = "android"))]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(any(target_os = "linux", target_os = "android"))]
use unix as backend;
#[cfg(windows)]
use windows as backend;

/// A backend exists for this target.
pub(crate) const fn supported() -> bool {
    cfg!(any(target_os = "linux", target_os = "android", windows))
}

pub(crate) fn open_directory(path: &Path) -> Result<File, ResourceError> {
    backend::open_directory(path)
}

pub(crate) fn identity_of_file(file: &File) -> Result<Identity, ResourceError> {
    backend::identity_of_file(file)
}

pub(crate) fn identity_of_path(path: &Path) -> Result<Identity, ResourceError> {
    backend::identity_of_path(path)
}

pub(crate) fn is_link(metadata: &Metadata) -> bool {
    backend::is_link(metadata)
}

/// Reject hard links early. Content exposure is additionally guarded by the
/// authoritative handle-level check inside `read_leaf`.
pub(crate) fn has_single_link(metadata: &Metadata) -> bool {
    backend::has_single_link(metadata)
}

/// A leaf is a single file name below the pinned directory. The grammar is
/// platform specific: Windows additionally treats `\\` as a separator, `:`
/// as an alternate data stream, and silently strips trailing dots/spaces.
pub(crate) fn valid_leaf(leaf: &str) -> bool {
    backend::valid_leaf(leaf)
}

pub(crate) fn random_id() -> Result<String, ResourceError> {
    backend::random_id()
}

/// Create a new direct child relative to a trusted pinned directory handle.
/// Never replace an existing file; errors after creation may have side effects.
pub(crate) fn create_leaf(root: &File, leaf: &str, bytes: &[u8]) -> Result<(), FileReadError> {
    backend::create_leaf(root, leaf, bytes)
}

/// Read-only edit preflight. This cannot authorize a later write by itself.
pub(crate) fn prepare_replace_leaf(
    root: &File,
    leaf: &str,
    expected: &[u8],
) -> Result<(), FileReadError> {
    backend::prepare_replace_leaf(root, leaf, expected)
}

pub(crate) fn read_leaf(root: &File, leaf: &str) -> Result<Vec<u8>, FileReadError> {
    backend::read_leaf(root, leaf)
}

pub(crate) fn hex(bytes: [u8; 32]) -> String {
    const HEX: &[u8] = b"0123456789abcdef";
    let mut id = String::with_capacity(64);
    for byte in bytes {
        id.push(HEX[usize::from(byte >> 4)] as char);
        id.push(HEX[usize::from(byte & 15)] as char);
    }
    id
}
