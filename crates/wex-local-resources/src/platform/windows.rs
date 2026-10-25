//! Windows backend.
//!
//! Windows has no `openat`, so a leaf is opened **relative to the pinned
//! directory handle** with `NtCreateFile` (`OBJECT_ATTRIBUTES::RootDirectory`).
//! That is the same kernel primitive `openat` gives on Unix: one component is
//! resolved against an open handle and the user supplied path is never
//! resolved again.
//!
//! Identity is (volume serial, file index) from `GetFileInformationByHandle`,
//! the direct analogue of (device, inode). Reparse points are opened but never
//! traversed (`FILE_OPEN_REPARSE_POINT`), so a junction cannot redirect a read.

use super::*;
use crate::FileReadError;
use std::ffi::{c_void, OsStr};
use std::io::Read;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::MetadataExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, RawHandle};

const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x0010;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;

const FILE_READ_ATTRIBUTES: u32 = 0x0080;
const FILE_LIST_DIRECTORY: u32 = 0x0001;
const FILE_READ_DATA: u32 = 0x0001;
const SYNCHRONIZE: u32 = 0x0010_0000;

const FILE_SHARE_ALL: u32 = 0x0000_0007;
const OPEN_EXISTING: u32 = 3;
const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;

const OBJ_CASE_INSENSITIVE: u32 = 0x0000_0040;
const FILE_OPEN: u32 = 0x0000_0001;
const FILE_NON_DIRECTORY_FILE: u32 = 0x0000_0040;
const FILE_SYNCHRONOUS_IO_NONALERT: u32 = 0x0000_0020;
const FILE_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const FILE_OPEN_FOR_BACKUP_INTENT: u32 = 0x0000_4000;

const BCRYPT_USE_SYSTEM_PREFERRED_RNG: u32 = 0x0000_0002;
const INVALID_HANDLE_VALUE: isize = -1;

#[repr(C)]
struct ByHandleFileInformation {
    attributes: u32,
    creation_low: u32,
    creation_high: u32,
    access_low: u32,
    access_high: u32,
    write_low: u32,
    write_high: u32,
    volume_serial: u32,
    size_high: u32,
    size_low: u32,
    links: u32,
    index_high: u32,
    index_low: u32,
}

#[repr(C)]
struct UnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *const u16,
}

#[repr(C)]
struct ObjectAttributes {
    length: u32,
    root_directory: isize,
    object_name: *const UnicodeString,
    attributes: u32,
    security_descriptor: *const c_void,
    security_quality_of_service: *const c_void,
}

#[repr(C)]
struct IoStatusBlock {
    status: usize,
    information: usize,
}

#[link(name = "kernel32")]
extern "system" {
    fn CreateFileW(
        name: *const u16,
        access: u32,
        share: u32,
        security: *const c_void,
        disposition: u32,
        flags: u32,
        template: isize,
    ) -> isize;
    fn GetFileInformationByHandle(handle: isize, info: *mut ByHandleFileInformation) -> i32;
}

#[link(name = "ntdll")]
extern "system" {
    fn NtCreateFile(
        handle: *mut isize,
        access: u32,
        attributes: *mut ObjectAttributes,
        iosb: *mut IoStatusBlock,
        allocation_size: *const i64,
        file_attributes: u32,
        share: u32,
        disposition: u32,
        options: u32,
        ea: *const c_void,
        ea_length: u32,
    ) -> i32;
}

#[link(name = "bcrypt")]
extern "system" {
    fn BCryptGenRandom(algorithm: isize, buffer: *mut u8, length: u32, flags: u32) -> i32;
}

fn wide(path: &Path) -> Result<Vec<u16>, ResourceError> {
    let mut units: Vec<u16> = path.as_os_str().encode_wide().collect();
    if units.contains(&0) {
        return Err(ResourceError::InvalidPath);
    }
    units.push(0);
    Ok(units)
}

pub(super) fn open_directory(path: &Path) -> Result<File, ResourceError> {
    let name = wide(path)?;
    // BACKUP_SEMANTICS is required to obtain a directory handle; the reparse
    // flag keeps a junction itself open instead of traversing it.
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            FILE_SHARE_ALL,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            0,
        )
    };
    if handle == INVALID_HANDLE_VALUE || handle == 0 {
        return Err(ResourceError::Unavailable);
    }
    // SAFETY: CreateFileW returned a valid, owned handle.
    let file = unsafe { File::from_raw_handle(handle as RawHandle) };
    let info = information(&file)?;
    if info.attributes & FILE_ATTRIBUTE_DIRECTORY == 0
        || info.attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err(ResourceError::UnsupportedTarget);
    }
    Ok(file)
}

pub(super) fn identity_of_file(file: &File) -> Result<Identity, ResourceError> {
    let info = information(file)?;
    Ok(Identity {
        device: u64::from(info.volume_serial),
        inode: (u64::from(info.index_high) << 32) | u64::from(info.index_low),
    })
}

pub(super) fn identity_of_path(path: &Path) -> Result<Identity, ResourceError> {
    let name = wide(path)?;
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            FILE_SHARE_ALL,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            0,
        )
    };
    if handle == INVALID_HANDLE_VALUE || handle == 0 {
        return Err(ResourceError::Unavailable);
    }
    // SAFETY: valid owned handle, closed by `file` on drop.
    let file = unsafe { File::from_raw_handle(handle as RawHandle) };
    identity_of_file(&file)
}

pub(super) fn is_link(metadata: &Metadata) -> bool {
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

/// `std` exposes no link count for Windows metadata. The authoritative check
/// runs on the opened handle inside `read_leaf`; this early filter therefore
/// accepts and lets the handle decide.
pub(super) fn has_single_link(_metadata: &Metadata) -> bool {
    true
}

pub(super) fn valid_leaf(leaf: &str) -> bool {
    if leaf.is_empty() || leaf == "." || leaf == ".." {
        return false;
    }
    if leaf.ends_with('.') || leaf.ends_with(' ') {
        return false;
    }
    if leaf.contains(['/', '\\', ':', '<', '>', '"', '|', '?', '*'])
        || leaf.chars().any(char::is_control)
    {
        return false;
    }
    // Avoid Win32 device names even when the NT relative-name API would
    // interpret them differently. An extension does not make one safe.
    let stem = leaf.split('.').next().unwrap_or(leaf).to_ascii_uppercase();
    if matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) {
        return false;
    }
    if stem.len() == 4
        && (stem.starts_with("COM") || stem.starts_with("LPT"))
        && matches!(stem.as_bytes()[3], b'1'..=b'9')
    {
        return false;
    }
    true
}

pub(super) fn random_id() -> Result<String, ResourceError> {
    let mut random = [0u8; 32];
    let status = unsafe {
        BCryptGenRandom(
            0,
            random.as_mut_ptr(),
            random.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status != 0 {
        return Err(ResourceError::Unavailable);
    }
    Ok(super::hex(random))
}

pub(super) fn read_leaf(root: &File, leaf: &str) -> Result<Vec<u8>, FileReadError> {
    if leaf.is_empty()
        || leaf == "."
        || leaf == ".."
        || leaf.contains('/')
        || leaf.contains('\\')
        || leaf.contains('\0')
    {
        return Err(FileReadError::Resource(ResourceError::InvalidPath));
    }
    let name: Vec<u16> = OsStr::new(leaf).encode_wide().collect();
    if name.is_empty() || name.len() > 255 || name.contains(&0) {
        return Err(FileReadError::Resource(ResourceError::InvalidPath));
    }
    let object_name = UnicodeString {
        length: (name.len() * 2) as u16,
        maximum_length: (name.len() * 2) as u16,
        buffer: name.as_ptr(),
    };
    let mut attributes = ObjectAttributes {
        length: std::mem::size_of::<ObjectAttributes>() as u32,
        root_directory: root.as_raw_handle() as isize,
        object_name: &object_name,
        attributes: OBJ_CASE_INSENSITIVE,
        security_descriptor: std::ptr::null(),
        security_quality_of_service: std::ptr::null(),
    };
    let mut iosb = IoStatusBlock {
        status: 0,
        information: 0,
    };
    let mut handle: isize = INVALID_HANDLE_VALUE;
    let status = unsafe {
        NtCreateFile(
            &mut handle,
            FILE_READ_DATA | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            &mut attributes,
            &mut iosb,
            std::ptr::null(),
            0,
            FILE_SHARE_ALL,
            FILE_OPEN,
            FILE_NON_DIRECTORY_FILE
                | FILE_SYNCHRONOUS_IO_NONALERT
                | FILE_OPEN_REPARSE_POINT
                | FILE_OPEN_FOR_BACKUP_INTENT,
            std::ptr::null(),
            0,
        )
    };
    if status < 0 || handle == INVALID_HANDLE_VALUE {
        return Err(FileReadError::Resource(ResourceError::Unavailable));
    }
    // SAFETY: NtCreateFile returned a valid, owned handle.
    let file = unsafe { File::from_raw_handle(handle as RawHandle) };

    let before = information(&file).map_err(FileReadError::Resource)?;
    regular(&before)?;
    let size = leaf_size(&before);
    if size > 65536 {
        return Err(FileReadError::TooLarge);
    }
    let root_info = information(root).map_err(FileReadError::Resource)?;
    // A file bind/mount alias on a different volume is not this directory.
    if root_info.volume_serial != before.volume_serial {
        return Err(FileReadError::Resource(ResourceError::OutsideDirectory));
    }

    let mut bytes = Vec::new();
    (&file)
        .take(65537)
        .read_to_end(&mut bytes)
        .map_err(|_| FileReadError::Resource(ResourceError::Unavailable))?;
    if bytes.len() > 65536 {
        return Err(FileReadError::TooLarge);
    }

    let after = information(&file).map_err(FileReadError::Resource)?;
    regular(&after)?;
    if stamp(&before) != stamp(&after) || bytes.len() as u64 != leaf_size(&after) {
        return Err(FileReadError::Changed);
    }
    Ok(bytes)
}

/// Exclusive create beneath the trusted directory handle. Never overwrite.
/// Failure after creation may leave a partial file and must not be retried blindly.
pub(super) fn create_leaf(root: &File, leaf: &str, bytes: &[u8]) -> Result<(), FileReadError> {
    use std::io::Write;
    if !valid_leaf(leaf) || bytes.len() > 65536 {
        return Err(FileReadError::Resource(ResourceError::InvalidPath));
    }
    let name: Vec<u16> = OsStr::new(leaf).encode_wide().collect();
    if name.is_empty() || name.len() > 255 || name.contains(&0) {
        return Err(FileReadError::Resource(ResourceError::InvalidPath));
    }
    let object_name = UnicodeString {
        length: (name.len() * 2) as u16,
        maximum_length: (name.len() * 2) as u16,
        buffer: name.as_ptr(),
    };
    let mut attributes = ObjectAttributes {
        length: std::mem::size_of::<ObjectAttributes>() as u32,
        root_directory: root.as_raw_handle() as isize,
        object_name: &object_name,
        attributes: OBJ_CASE_INSENSITIVE,
        security_descriptor: std::ptr::null(),
        security_quality_of_service: std::ptr::null(),
    };
    let mut iosb = IoStatusBlock {
        status: 0,
        information: 0,
    };
    let mut handle = INVALID_HANDLE_VALUE;
    // FILE_CREATE fails if this name already exists; OPEN_REPARSE_POINT never follows links.
    const FILE_WRITE_DATA: u32 = 0x0002;
    const FILE_CREATE: u32 = 0x0000_0002;
    let status = unsafe {
        NtCreateFile(
            &mut handle,
            FILE_WRITE_DATA | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            &mut attributes,
            &mut iosb,
            std::ptr::null(),
            0,
            FILE_SHARE_ALL,
            FILE_CREATE,
            FILE_NON_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT | FILE_OPEN_REPARSE_POINT,
            std::ptr::null(),
            0,
        )
    };
    if status < 0 || handle == INVALID_HANDLE_VALUE {
        return Err(FileReadError::Resource(ResourceError::Unavailable));
    }
    // SAFETY: NtCreateFile returned a new owned handle.
    let mut file = unsafe { File::from_raw_handle(handle as RawHandle) };
    let root_info = information(root).map_err(FileReadError::Resource)?;
    let new_info = information(&file).map_err(FileReadError::Resource)?;
    regular(&new_info)?;
    if new_info.volume_serial != root_info.volume_serial {
        return Err(FileReadError::Resource(ResourceError::OutsideDirectory));
    }
    file.write_all(bytes).map_err(|_| FileReadError::Changed)?;
    file.sync_all().map_err(|_| FileReadError::Changed)?;
    let after = information(&file).map_err(|_| FileReadError::Changed)?;
    regular(&after)?;
    if after.volume_serial != new_info.volume_serial
        || after.index_high != new_info.index_high
        || after.index_low != new_info.index_low
        || leaf_size(&after) != bytes.len() as u64
    {
        return Err(FileReadError::Changed);
    }
    Ok(())
}

/// Non-mutating precondition for a future versioned replacement transaction.
/// This is NOT a write authorization token; the commit must revalidate atomically.
pub(super) fn prepare_replace_leaf(
    root: &File,
    leaf: &str,
    expected: &[u8],
) -> Result<(), FileReadError> {
    if !valid_leaf(leaf) || expected.len() > 65536 {
        return Err(FileReadError::Resource(ResourceError::InvalidPath));
    }
    if read_leaf(root, leaf)? != expected {
        return Err(FileReadError::Changed);
    }
    Ok(())
}

fn information(file: &File) -> Result<ByHandleFileInformation, ResourceError> {
    let mut info = ByHandleFileInformation {
        attributes: 0,
        creation_low: 0,
        creation_high: 0,
        access_low: 0,
        access_high: 0,
        write_low: 0,
        write_high: 0,
        volume_serial: 0,
        size_high: 0,
        size_low: 0,
        links: 0,
        index_high: 0,
        index_low: 0,
    };
    let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle() as isize, &mut info) };
    if ok == 0 {
        return Err(ResourceError::Unavailable);
    }
    Ok(info)
}

fn regular(info: &ByHandleFileInformation) -> Result<(), FileReadError> {
    if info.attributes & FILE_ATTRIBUTE_DIRECTORY != 0
        || info.attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || info.links != 1
    {
        return Err(FileReadError::Resource(ResourceError::UnsupportedTarget));
    }
    Ok(())
}

fn leaf_size(info: &ByHandleFileInformation) -> u64 {
    (u64::from(info.size_high) << 32) | u64::from(info.size_low)
}

fn stamp(info: &ByHandleFileInformation) -> (u32, u32, u32, u64, u32, u32, u32) {
    (
        info.volume_serial,
        info.index_high,
        info.index_low,
        leaf_size(info),
        info.write_high,
        info.write_low,
        info.links,
    )
}

#[cfg(test)]
mod leaf_name_tests {
    use super::*;

    #[test]
    fn windows_device_names_never_become_file_targets() {
        for name in [
            "CON", "con.txt", "PRN", "aux.rs", "NUL", "COM1", "com9.log", "LPT1", "lpt9.txt",
            "CONIN$", "CONOUT$",
        ] {
            assert!(!valid_leaf(name), "reserved: {name}");
        }
        for name in [
            "main.rs",
            "README.md",
            ".gitignore",
            "file name.txt",
            "com10.rs",
        ] {
            assert!(valid_leaf(name), "ordinary: {name}");
        }
    }

    #[test]
    fn windows_paths_and_alternate_streams_are_not_leaf_names() {
        for name in [
            "",
            ".",
            "..",
            "../config",
            r"sub\file",
            "file:stream",
            "file.",
            "file ",
            "a?b",
            "a*b",
            "a\nline",
        ] {
            assert!(!valid_leaf(name), "invalid: {name:?}");
        }
    }
}
