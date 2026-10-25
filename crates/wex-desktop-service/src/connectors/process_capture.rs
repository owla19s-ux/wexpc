//! Owned output collectors. Stopping a collector also joins its dedicated reader.
use std::{
    io::Read,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};
#[derive(Default)]
pub(super) struct Capture {
    pub(super) bytes: Vec<u8>,
    pub(super) truncated: bool,
    pub(super) finished: bool,
    pub(super) failed: bool,
}
pub(super) struct CaptureReader {
    pub(super) state: Arc<Mutex<Capture>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl CaptureReader {
    pub(super) fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let Some(worker) = self.worker.take() else {
            return;
        };
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            #[link(name = "kernel32")]
            extern "system" {
                #[link_name = "CancelSynchronousIo"]
                fn cancel_synchronous_io(thread: *mut std::ffi::c_void) -> i32;
            }
            // A dedicated reader thread never starts unrelated I/O. Repeat
            // until it exits: cancellation can race the check before ReadFile
            // (or a synchronous PeekNamedPipe). Keep its handle alive for join.
            while !worker.is_finished() {
                unsafe {
                    cancel_synchronous_io(worker.as_raw_handle());
                }
                thread::sleep(Duration::from_millis(5));
            }
        }
        if worker.join().is_err() {
            self.state.lock().unwrap_or_else(|p| p.into_inner()).failed = true;
        }
    }
}
impl Drop for CaptureReader {
    fn drop(&mut self) {
        self.stop();
    }
}
// Production sources must be owned PipeReader endpoints. Unix reads must be
// nonblocking; Windows shutdown cancels synchronous I/O on this dedicated thread.
// Test readers also honour WouldBlock/EOF without blocking.
pub(super) fn collect(mut source: impl Read + Send + 'static) -> CaptureReader {
    let capture = Arc::new(Mutex::new(Capture::default()));
    let target = capture.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_read = stop.clone();
    let worker = thread::spawn(move || {
        let mut chunk = [0u8; 8192];
        while !stop_read.load(Ordering::Acquire) {
            let read = source.read(&mut chunk);
            if matches!(&read, Err(error) if error.kind() == std::io::ErrorKind::WouldBlock) {
                thread::sleep(Duration::from_millis(5));
                continue;
            }
            if matches!(&read, Err(error) if error.kind() == std::io::ErrorKind::Interrupted) {
                continue;
            }
            let Ok(mut state) = target.lock() else {
                return;
            };
            match read {
                Ok(0) => {
                    state.finished = true;
                    return;
                }
                Ok(size) => {
                    let kept = size.min((64usize * 1024).saturating_sub(state.bytes.len()));
                    state.bytes.extend_from_slice(&chunk[..kept]);
                    state.truncated |= kept < size;
                }
                Err(_) => {
                    state.failed = true;
                    state.finished = true;
                    return;
                }
            }
        }
    });
    CaptureReader {
        state: capture,
        stop,
        worker: Some(worker),
    }
}
#[cfg(test)]
mod capture_tests {
    use super::*;
    use std::time::Instant;
    fn finished(capture: &CaptureReader) {
        let start = Instant::now();
        while !capture.state.lock().unwrap().finished {
            assert!(start.elapsed() < Duration::from_secs(2));
            thread::sleep(Duration::from_millis(1));
        }
    }
    #[test]
    fn temporarily_empty_output_keeps_waiting_instead_of_failing() {
        use std::sync::atomic::{AtomicBool, Ordering};
        struct Idle(Arc<AtomicBool>);
        impl Read for Idle {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                if self.0.load(Ordering::Acquire) {
                    Ok(0)
                } else {
                    Err(std::io::ErrorKind::WouldBlock.into())
                }
            }
        }
        let close = Arc::new(AtomicBool::new(false));
        let capture = collect(Idle(close.clone()));
        thread::sleep(Duration::from_millis(30));
        let failed = capture.state.lock().unwrap().failed;
        close.store(true, Ordering::Release);
        finished(&capture);
        assert!(!failed, "an open idle pipe is not a read failure");
    }
    #[test]
    fn capture_helper() {
        if std::env::args().any(|arg| arg == "--exact") {
            thread::sleep(Duration::from_secs(10));
        }
    }
    #[test]
    fn dropping_capture_releases_an_idle_pipe_without_waiting_for_the_child() {
        struct Child(std::process::Child);
        impl Drop for Child {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let mut child = Child(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "connectors::process_capture::capture_tests::capture_helper",
                ])
                .stdout(std::process::Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let capture = collect(
            super::super::process_pipe::PipeReader::new(child.0.stdout.take().unwrap()).unwrap(),
        );
        thread::sleep(Duration::from_millis(30));
        let start = Instant::now();
        drop(capture);
        assert!(start.elapsed() < Duration::from_secs(2));
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "the reader closes independently of child termination"
        );
    }
    #[test]
    fn oversized_output_is_drained_without_unbounded_retention() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        struct Counted {
            cursor: std::io::Cursor<Vec<u8>>,
            read: Arc<AtomicUsize>,
        }
        impl Read for Counted {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                let n = self.cursor.read(buffer)?;
                self.read.fetch_add(n, Ordering::SeqCst);
                Ok(n)
            }
        }
        let count = Arc::new(AtomicUsize::new(0));
        let capture = collect(Counted {
            cursor: std::io::Cursor::new(vec![b'x'; 128 * 1024]),
            read: count.clone(),
        });
        finished(&capture);
        let state = capture.state.lock().unwrap();
        assert!(state.truncated);
        assert_eq!(state.bytes.len(), 64 * 1024);
        assert_eq!(count.load(Ordering::SeqCst), 128 * 1024);
    }
    #[test]
    fn exact_limit_and_short_output_are_not_truncated() {
        for size in [0, 4096, 64 * 1024] {
            let capture = collect(std::io::Cursor::new(vec![b'y'; size]));
            finished(&capture);
            let state = capture.state.lock().unwrap();
            assert!(!state.truncated);
            assert_eq!(state.bytes.len(), size);
        }
    }
}
