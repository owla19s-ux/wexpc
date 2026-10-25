//! Trusted Host process adapter. A request is inert until Core dispatch and
//! exact argument binding are verified; never infer authority from a project.
use super::process_capture::collect;
use crate::{Result, ServiceError};
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use wex_core::operation::{Action, Dispatch};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessRequest {
    pub program: String,
    pub arguments: Vec<String>,
    pub timeout_ms: u64,
}
impl ProcessRequest {
    pub fn validate(&self) -> Result<()> {
        if self.program.is_empty()
            || self.program.contains('\0')
            || self.program.len() > 4096
            || self.arguments.iter().any(|s| s.contains('\0'))
            || self.arguments.iter().map(|s| s.len()).sum::<usize>() > 1024 * 1024
            || self.timeout_ms == 0
        {
            return Err(ServiceError::new(
                "INVALID_INPUT",
                "程序、参数或超时设置无效",
            ));
        }
        Ok(())
    }
    pub fn intent_digest(&self, resource: &str, root: &Path) -> Result<String> {
        self.validate()?;
        let root = root
            .to_str()
            .ok_or_else(|| ServiceError::new("INVALID_INPUT", "工作目录编码无效"))?;
        let encoded = serde_json::to_vec(self)
            .map_err(|_| ServiceError::new("INVALID_INPUT", "命令参数无法编码"))?;
        Ok(super::intent_binding::digest_arguments(
            "terminal_execute",
            resource,
            &[root.as_bytes(), &encoded],
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessTermination {
    Exited,
    Cancelled,
    TimedOut,
    OutputInterrupted,
}
#[derive(Debug, Serialize)]
pub struct ProcessReport {
    pub termination: ProcessTermination,
    pub program: String,
    pub working_directory: String,
    pub timeout_ms: u64,
    pub success: bool,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    /// True only after EOF without a read error; independent of retention limits.
    pub stdout_complete: bool,
    pub stderr_complete: bool,
    pub elapsed_ms: u64,
}

pub fn execute_approved_process_report(
    dispatch: &Dispatch,
    resource: &str,
    root: &Path,
    request: &ProcessRequest,
) -> Result<ProcessReport> {
    let report = execute_approved_process_report_with_control(
        dispatch,
        resource,
        root,
        request,
        &crate::ProcessCancellation::default(),
    )?;
    if report.interrupted() {
        Err(unknown_error())
    } else {
        Ok(report)
    }
}
impl ProcessReport {
    pub fn interrupted(&self) -> bool {
        self.termination != ProcessTermination::Exited
    }
}
pub fn execute_approved_process_report_with_control(
    dispatch: &Dispatch,
    resource: &str,
    root: &Path,
    request: &ProcessRequest,
    control: &crate::ProcessCancellation,
) -> Result<ProcessReport> {
    if dispatch.intent.action != Action::Write
        || dispatch.intent.capability_id != "terminal_execute"
        || dispatch.intent.resource_id != resource
    {
        return Err(ServiceError::new(
            "DISPATCH_REJECTED",
            "进程请求未获匹配授权",
        ));
    }
    if request.intent_digest(resource, root)? != dispatch.intent.intent_digest {
        return Err(ServiceError::new(
            "INTENT_MISMATCH",
            "程序参数或工作目录与批准时不一致",
        ));
    }
    run_process_with_control(root, request, control)
}

pub(super) fn run_process_with_control(
    root: &Path,
    request: &ProcessRequest,
    control: &crate::ProcessCancellation,
) -> Result<ProcessReport> {
    struct Finish<'a>(&'a crate::ProcessCancellation);
    impl Drop for Finish<'_> {
        fn drop(&mut self) {
            self.0.finish();
        }
    }
    let _finish = Finish(control);
    request.validate()?;
    if !root.is_absolute() || !root.is_dir() {
        return Err(ServiceError::new("RESOURCE_UNAVAILABLE", "工作目录不可用"));
    }
    if control.is_finished() {
        return Err(ServiceError::new(
            "PROCESS_FINISHED",
            "执行控制已结算，不能用于新进程",
        ));
    }
    if control.is_cancelled() {
        return Err(ServiceError::new(
            "PROCESS_CANCELLED",
            "程序启动前已取消，未执行程序",
        ));
    }
    let start = Instant::now();
    let timeout = Duration::from_millis(request.timeout_ms);
    let mut command = Command::new(&request.program);
    command
        .args(&request.arguments)
        .current_dir(root)
        .env("CARGO_TERM_COLOR", "never")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|_| ServiceError::new("TOOL_UNAVAILABLE", "无法启动所选程序"))?;
    #[cfg(unix)]
    struct Group {
        id: u32,
        armed: bool,
    }
    #[cfg(unix)]
    impl Group {
        fn stop(&mut self) {
            if self.armed {
                unsafe {
                    libc::kill(-(self.id as i32), libc::SIGKILL);
                }
                self.armed = false;
            }
        }
    }
    #[cfg(unix)]
    impl Drop for Group {
        fn drop(&mut self) {
            self.stop();
        }
    }
    #[cfg(unix)]
    let mut group = Group {
        id: child.id(),
        armed: true,
    };
    use super::process_pipe::PipeReader;
    let pipes = (|| -> std::io::Result<_> {
        Ok((
            PipeReader::new(child.stdout.take().expect("piped stdout"))?,
            PipeReader::new(child.stderr.take().expect("piped stderr"))?,
        ))
    })();
    let (out_pipe, err_pipe) = match pipes {
        Ok(pipes) => pipes,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(unknown_error());
        }
    };
    let mut stdout = collect(out_pipe);
    let mut stderr = collect(err_pipe);
    let mut status = None;
    let termination = loop {
        if control.is_cancelled() {
            break ProcessTermination::Cancelled;
        }
        if start.elapsed() >= timeout {
            break ProcessTermination::TimedOut;
        }
        if status.is_none() {
            match child.try_wait() {
                Ok(value) => status = value,
                Err(_) => break ProcessTermination::OutputInterrupted,
            }
        }
        let out = stdout.state.lock().map_err(|_| unknown_error())?;
        let err = stderr.state.lock().map_err(|_| unknown_error())?;
        if out.failed || err.failed {
            break ProcessTermination::OutputInterrupted;
        }
        if status.is_some() && out.finished && err.finished {
            break if control.finish() {
                ProcessTermination::Exited
            } else {
                ProcessTermination::Cancelled
            };
        }
        drop(out);
        drop(err);
        thread::sleep(Duration::from_millis(10));
    };
    if termination != ProcessTermination::Exited {
        #[cfg(unix)]
        group.stop();
        if status.is_none() {
            let _ = child.kill();
            status = child.wait().ok();
        }
        // Bounded grace period for final pipe bytes after termination.
        // A surviving descendant may retain writers; reader ownership still ends.
        let cleanup = Instant::now();
        while cleanup.elapsed() < Duration::from_millis(200) {
            if stdout.state.lock().map_err(|_| unknown_error())?.finished
                && stderr.state.lock().map_err(|_| unknown_error())?.finished
            {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
    } else {
        #[cfg(unix)]
        {
            group.armed = false;
        }
    }
    stdout.stop();
    stderr.stop();
    let out = stdout.state.lock().map_err(|_| unknown_error())?;
    let err = stderr.state.lock().map_err(|_| unknown_error())?;
    Ok(ProcessReport {
        termination,
        program: request.program.clone(),
        working_directory: root.to_string_lossy().into_owned(),
        timeout_ms: request.timeout_ms,
        success: termination == ProcessTermination::Exited && status.is_some_and(|s| s.success()),
        exit_code: status.and_then(|s| s.code()),
        stdout: String::from_utf8_lossy(&out.bytes).into_owned(),
        stderr: String::from_utf8_lossy(&err.bytes).into_owned(),
        stdout_truncated: out.truncated,
        stderr_truncated: err.truncated,
        stdout_complete: out.finished && !out.failed,
        stderr_complete: err.finished && !err.failed,
        elapsed_ms: start.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
    })
}
fn unknown_error() -> ServiceError {
    ServiceError::new(
        "OUTCOME_UNKNOWN",
        "进程或输出管道中断，副作用需核查；未自动重试",
    )
}
