//! Trusted Host process adapter. A request is inert until Core dispatch and
//! exact argument binding are verified; never infer authority from a project.
use crate::{Result, ServiceError};
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    path::Path,
    process::{Command, Stdio},
    sync::mpsc,
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

#[derive(Debug, Serialize)]
pub struct ProcessReport {
    pub program: String,
    pub working_directory: String,
    pub timeout_ms: u64,
    pub success: bool,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub elapsed_ms: u64,
}

pub fn execute_approved_process_report(
    dispatch: &Dispatch,
    resource: &str,
    root: &Path,
    request: &ProcessRequest,
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
    run_process(root, request)
}

pub(super) fn run_process(root: &Path, request: &ProcessRequest) -> Result<ProcessReport> {
    request.validate()?;
    if !root.is_absolute() || !root.is_dir() {
        return Err(ServiceError::new("RESOURCE_UNAVAILABLE", "工作目录不可用"));
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
    // An interrupted runner must not leave descendants holding its pipes.
    // Unix process groups cover descendants that remain in the inherited group;
    // this is lifecycle cleanup, not containment of a hostile setsid caller.
    #[cfg(unix)]
    struct Group {
        id: u32,
        armed: bool,
    }
    #[cfg(unix)]
    impl Drop for Group {
        fn drop(&mut self) {
            // SAFETY: this group was created for our newly spawned child.
            if self.armed {
                unsafe {
                    libc::kill(-(self.id as i32), libc::SIGKILL);
                }
            }
        }
    }
    #[cfg(unix)]
    let mut group = Group {
        id: child.id(),
        armed: true,
    };
    fn collect(
        source: impl Read + Send + 'static,
    ) -> mpsc::Receiver<std::io::Result<(Vec<u8>, bool)>> {
        let (send, receive) = mpsc::sync_channel(1);
        thread::spawn(move || {
            let _ = send.send(super::terminal_dispatch::collect_bounded_output(
                source,
                64 * 1024,
            ));
        });
        receive
    }
    let stdout = collect(child.stdout.take().expect("piped stdout"));
    let stderr = collect(child.stderr.take().expect("piped stderr"));
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() < timeout => thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(unknown_error());
            }
        }
    };
    let out = stdout
        .recv_timeout(timeout.saturating_sub(start.elapsed()))
        .map_err(|_| unknown_error())?
        .map_err(|_| unknown_error())?;
    let err = stderr
        .recv_timeout(timeout.saturating_sub(start.elapsed()))
        .map_err(|_| unknown_error())?
        .map_err(|_| unknown_error())?;
    #[cfg(unix)]
    {
        group.armed = false;
    }
    Ok(ProcessReport {
        program: request.program.clone(),
        working_directory: root.to_string_lossy().into_owned(),
        timeout_ms: request.timeout_ms,
        success: status.success(),
        exit_code: status.code(),
        stdout: String::from_utf8_lossy(&out.0).into_owned(),
        stderr: String::from_utf8_lossy(&err.0).into_owned(),
        stdout_truncated: out.1,
        stderr_truncated: err.1,
        elapsed_ms: start.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
    })
}
fn unknown_error() -> ServiceError {
    ServiceError::new(
        "OUTCOME_UNKNOWN",
        "进程或输出管道超时/中断，副作用需核查；未自动重试",
    )
}
