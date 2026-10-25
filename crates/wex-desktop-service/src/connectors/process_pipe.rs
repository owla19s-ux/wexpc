//! Single-owner process-pipe reads: Unix nonblocking, Windows peek before read.
//! Windows collector shutdown also cancels pending synchronous I/O.
//! Only the reader owned here consumes bytes; polling cannot race another reader.
use std::io::{self, Read};
#[cfg(unix)]
use std::os::fd::AsRawFd as PipeHandle;
#[cfg(windows)]
use std::os::windows::io::AsRawHandle as PipeHandle;

pub(super) struct PipeReader<T> {
    source: T,
}
impl<T: Read + PipeHandle> PipeReader<T> {
    pub(super) fn new(source: T) -> io::Result<Self> {
        #[cfg(unix)]
        {
            // The read endpoint is owned by this adapter. Do not modify the
            // writer's flags or duplicate/alias this read endpoint elsewhere.
            let fd = source.as_raw_fd();
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            if flags == -1
                || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1
            {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(Self { source })
    }
}
impl<T: Read + PipeHandle> Read for PipeReader<T> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        #[cfg(windows)]
        {
            #[link(name = "kernel32")]
            extern "system" {
                #[link_name = "PeekNamedPipe"]
                fn peek_named_pipe(
                    handle: *mut std::ffi::c_void,
                    buffer: *mut std::ffi::c_void,
                    size: u32,
                    read: *mut u32,
                    available: *mut u32,
                    remaining: *mut u32,
                ) -> i32;
            }
            if buffer.is_empty() {
                return Ok(0);
            }
            let mut available = 0u32;
            // This is an owned anonymous stdout/stderr pipe, not an arbitrary
            // shared synchronous handle. No concurrent ReadFile can steal the
            // bytes between this peek and our bounded read.
            let ok = unsafe {
                peek_named_pipe(
                    self.source.as_raw_handle(),
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null_mut(),
                    &mut available,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 {
                let error = io::Error::last_os_error();
                return if error.raw_os_error() == Some(109) {
                    Ok(0)
                } else {
                    Err(error)
                };
            }
            if available == 0 {
                return Err(io::ErrorKind::WouldBlock.into());
            }
            let size = buffer.len().min(available as usize);
            self.source.read(&mut buffer[..size])
        }
        #[cfg(unix)]
        self.source.read(buffer)
    }
}
