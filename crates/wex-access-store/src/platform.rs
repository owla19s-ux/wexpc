//! Platform storage primitives for the grant authority store.
//!
//! The store keeps authorization records, so its directory and files must be
//! private to the current user and every replacement must be atomic. Unix uses
//! mode bits plus rename; Windows needs an explicit DACL and MoveFileEx.

use std::fs::File;
use std::io;
use std::path::Path;

#[cfg(unix)]
mod imp {
    use super::*;
    use std::fs::{self, OpenOptions};
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};

    pub(super) const fn supported() -> bool {
        true
    }

    pub(super) fn create_private_dir(path: &Path) -> io::Result<()> {
        match fs::DirBuilder::new().mode(0o700).create(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
            Err(error) => Err(error),
        }
    }

    pub(super) fn create_private_file(path: &Path) -> io::Result<File> {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
    }

    pub(super) fn is_private(path: &Path, _dir: bool) -> bool {
        match fs::symlink_metadata(path) {
            Ok(metadata) => metadata.permissions().mode() & 0o077 == 0,
            Err(_) => false,
        }
    }

    /// Rename is atomic on the same filesystem; the old snapshot is never
    /// deleted first.
    pub(super) fn replace_file(from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::ffi::{c_void, OsStr};
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{FromRawHandle, RawHandle};

    const TOKEN_QUERY: u32 = 0x0008;
    const TOKEN_USER_CLASS: u32 = 1;
    const SDDL_REVISION_1: u32 = 1;
    const SE_FILE_OBJECT: u32 = 1;
    const DACL_SECURITY_INFORMATION: u32 = 0x0000_0004;
    const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x0000_0001;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x0000_0008;

    const GENERIC_WRITE: u32 = 0x4000_0000;
    const CREATE_NEW: u32 = 1;
    const FILE_ATTRIBUTE_NORMAL: u32 = 0x80;
    const INVALID_HANDLE_VALUE: isize = -1;

    #[repr(C)]
    struct SecurityAttributes {
        length: u32,
        descriptor: *mut c_void,
        inherit: i32,
    }

    #[repr(C)]
    struct AclHeader {
        revision: u8,
        size: u16,
        ace_count: u16,
        reserved: u16,
    }

    #[repr(C)]
    struct AceHeader {
        ace_type: u8,
        ace_flags: u8,
        ace_size: u16,
    }

    #[repr(C)]
    struct AccessAllowedAce {
        header: AceHeader,
        mask: u32,
    }

    #[link(name = "advapi32")]
    extern "system" {
        fn OpenProcessToken(process: isize, access: u32, token: *mut isize) -> i32;
        fn GetTokenInformation(
            token: isize,
            class: u32,
            buffer: *mut c_void,
            length: u32,
            needed: *mut u32,
        ) -> i32;
        fn ConvertSidToStringSidW(sid: *const c_void, string: *mut *mut u16) -> i32;
        fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
            string: *const u16,
            revision: u32,
            descriptor: *mut *mut c_void,
            size: *mut u32,
        ) -> i32;
        fn GetNamedSecurityInfoW(
            name: *const u16,
            object_type: u32,
            info: u32,
            owner: *mut *mut c_void,
            group: *mut *mut c_void,
            dacl: *mut *mut c_void,
            sacl: *mut *mut c_void,
            descriptor: *mut *mut c_void,
        ) -> u32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn CloseHandle(handle: isize) -> i32;
        fn LocalFree(memory: *mut c_void) -> *mut c_void;
        fn CreateDirectoryW(path: *const u16, attributes: *const SecurityAttributes) -> i32;
        fn CreateFileW(
            name: *const u16,
            access: u32,
            share: u32,
            attributes: *const SecurityAttributes,
            disposition: u32,
            flags: u32,
            template: isize,
        ) -> isize;
        fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
        fn GetLastError() -> u32;
    }

    pub(super) const fn supported() -> bool {
        true
    }

    fn wide(path: &Path) -> io::Result<Vec<u16>> {
        let mut units: Vec<u16> = path.as_os_str().encode_wide().collect();
        if units.contains(&0) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "NUL in path"));
        }
        units.push(0);
        Ok(units)
    }

    fn last_error() -> io::Error {
        io::Error::from_raw_os_error(unsafe { GetLastError() } as i32)
    }

    /// Current user SID rendered as `S-1-5-...`.
    fn current_user_sid() -> io::Result<String> {
        let mut token: isize = 0;
        if unsafe { OpenProcessToken(-1, TOKEN_QUERY, &mut token) } == 0 {
            return Err(last_error());
        }
        let mut needed = 0u32;
        unsafe {
            GetTokenInformation(
                token,
                TOKEN_USER_CLASS,
                std::ptr::null_mut(),
                0,
                &mut needed,
            );
        }
        if needed == 0 {
            unsafe { CloseHandle(token) };
            return Err(io::Error::other("token query"));
        }
        let mut buffer = vec![0u8; needed as usize];
        let ok = unsafe {
            GetTokenInformation(
                token,
                TOKEN_USER_CLASS,
                buffer.as_mut_ptr() as *mut c_void,
                needed,
                &mut needed,
            )
        };
        unsafe { CloseHandle(token) };
        if ok == 0 {
            return Err(last_error());
        }
        // TOKEN_USER { SID_AND_ATTRIBUTES user } -> first field is the SID.
        let sid = unsafe { *(buffer.as_ptr() as *const *const c_void) };
        let mut string: *mut u16 = std::ptr::null_mut();
        if unsafe { ConvertSidToStringSidW(sid, &mut string) } == 0 {
            return Err(last_error());
        }
        let mut length = 0usize;
        while unsafe { *string.add(length) } != 0 {
            length += 1;
        }
        let rendered =
            String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(string, length) });
        unsafe { LocalFree(string as *mut c_void) };
        Ok(rendered)
    }

    /// Owner-only, inheritance-protected DACL descriptor.
    fn private_descriptor() -> io::Result<*mut c_void> {
        let sid = current_user_sid()?;
        // The SID string is embedded into an SDDL document, so only the
        // characters a SID may contain are accepted.
        if sid.is_empty() || !sid.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err(io::Error::other("unexpected SID rendering"));
        }
        let sddl: Vec<u16> = OsStr::new(&format!("D:P(A;;FA;;;{sid})"))
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let mut descriptor: *mut c_void = std::ptr::null_mut();
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(last_error());
        }
        Ok(descriptor)
    }

    pub(super) fn create_private_dir(path: &Path) -> io::Result<()> {
        if path.is_dir() {
            return Ok(());
        }
        let name = wide(path)?;
        let descriptor = private_descriptor()?;
        let attributes = SecurityAttributes {
            length: std::mem::size_of::<SecurityAttributes>() as u32,
            descriptor,
            inherit: 0,
        };
        let ok = unsafe { CreateDirectoryW(name.as_ptr(), &attributes) };
        let error = if ok == 0 { Some(last_error()) } else { None };
        unsafe { LocalFree(descriptor) };
        match error {
            None => Ok(()),
            // Racing creation is fine: the directory exists and verify_private
            // still has to pass its DACL check.
            Some(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(()),
            Some(e) => Err(e),
        }
    }

    pub(super) fn create_private_file(path: &Path) -> io::Result<File> {
        let name = wide(path)?;
        let descriptor = private_descriptor()?;
        let attributes = SecurityAttributes {
            length: std::mem::size_of::<SecurityAttributes>() as u32,
            descriptor,
            inherit: 0,
        };
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_WRITE,
                0,
                &attributes,
                CREATE_NEW,
                FILE_ATTRIBUTE_NORMAL,
                0,
            )
        };
        let error = if handle == INVALID_HANDLE_VALUE {
            Some(last_error())
        } else {
            None
        };
        unsafe { LocalFree(descriptor) };
        match error {
            Some(e) => Err(e),
            // SAFETY: CreateFileW returned an owned handle.
            None => Ok(unsafe { File::from_raw_handle(handle as RawHandle) }),
        }
    }

    /// Reject a DACL that grants anyone beyond the owner.
    ///
    /// Windows has no direct "group/other" bit, so the broad well-known
    /// principals are rejected explicitly and any other ACE must belong to the
    /// current user.
    pub(super) fn is_private(path: &Path, _dir: bool) -> bool {
        const BROAD: [&str; 5] = [
            "S-1-1-0",      // Everyone
            "S-1-5-32-545", // BUILTIN\Users
            "S-1-5-11",     // Authenticated Users
            "S-1-5-4",      // INTERACTIVE
            "S-1-5-7",      // ANONYMOUS LOGON
        ];
        let Ok(name) = wide(path) else { return false };
        let mut dacl: *mut c_void = std::ptr::null_mut();
        let mut descriptor: *mut c_void = std::ptr::null_mut();
        let status = unsafe {
            GetNamedSecurityInfoW(
                name.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut dacl,
                std::ptr::null_mut(),
                &mut descriptor,
            )
        };
        if status != 0 {
            return false;
        }
        if dacl.is_null() {
            // No DACL means everyone is granted access.
            unsafe { LocalFree(descriptor) };
            return false;
        }
        let acl = dacl as *const AclHeader;
        let count = unsafe { (*acl).ace_count } as usize;
        let acl_size = unsafe { (*acl).size } as usize;
        let mut offset = std::mem::size_of::<AclHeader>();
        let mut private = true;
        for _ in 0..count {
            // Stay inside the buffer the API allocated: a corrupted or hostile
            // ACL must not be able to walk the pointer past its own end.
            if offset + std::mem::size_of::<AceHeader>() > acl_size {
                private = false;
                break;
            }
            let ace = unsafe { (dacl as *const u8).add(offset) };
            let header = unsafe { &*(ace as *const AceHeader) };
            let ace_size = header.ace_size as usize;
            // An access-allowed ACE is a header, a mask and a SID; the smallest
            // SID is 8 bytes, so anything shorter cannot be inspected safely.
            if ace_size < std::mem::size_of::<AceHeader>() + 4 + 8 || offset + ace_size > acl_size {
                private = false;
                break;
            }
            if header.ace_type == ACCESS_ALLOWED_ACE_TYPE {
                let allowed = unsafe { &*(ace as *const AccessAllowedAce) };
                // ACE layout: 4 byte header, 4 byte mask, then the variable
                // length SID. It is not a fixed-size struct.
                let sid =
                    unsafe { ace.add(std::mem::size_of::<AccessAllowedAce>()) as *const c_void };
                let mut rendered: *mut u16 = std::ptr::null_mut();
                let ok = unsafe { ConvertSidToStringSidW(sid, &mut rendered) };
                if ok == 0 {
                    private = false;
                    break;
                }
                let mut length = 0usize;
                while unsafe { *rendered.add(length) } != 0 {
                    length += 1;
                }
                let text = String::from_utf16_lossy(unsafe {
                    std::slice::from_raw_parts(rendered, length)
                });
                unsafe { LocalFree(rendered as *mut c_void) };
                if allowed.mask != 0 && BROAD.contains(&text.as_str()) {
                    private = false;
                    break;
                }
            }
            offset += ace_size;
        }
        unsafe { LocalFree(descriptor) };
        private
    }

    /// Never delete the old snapshot first; the kernel replaces it in place.
    pub(super) fn replace_file(from: &Path, to: &Path) -> io::Result<()> {
        let source = wide(from)?;
        let destination = wide(to)?;
        let flags = MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH;
        if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), flags) } == 0 {
            return Err(last_error());
        }
        Ok(())
    }
}

pub(crate) const fn supported() -> bool {
    imp::supported()
}

pub(crate) fn create_private_dir(path: &Path) -> io::Result<()> {
    imp::create_private_dir(path)
}

pub(crate) fn create_private_file(path: &Path) -> io::Result<File> {
    imp::create_private_file(path)
}

pub(crate) fn is_private(path: &Path) -> bool {
    imp::is_private(path, true)
}

pub(crate) fn replace_file(from: &Path, to: &Path) -> io::Result<()> {
    imp::replace_file(from, to)
}
