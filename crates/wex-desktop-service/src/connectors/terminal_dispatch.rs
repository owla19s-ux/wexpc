//! Explicitly approved, allowlisted terminal execution (no shell).
//! Cargo builds may execute repository build scripts: always require a separate
//! human-approved command intent, never infer access from a project link.
use crate::{Result, ServiceError};
use std::{io::Read, path::Path, process::{Command, Stdio}, thread, time::{Duration, Instant}};
use wex_core::operation::{Action, Dispatch};

/// The only commands allowed by this first-stage terminal adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalTask { CargoTest, CargoFmtCheck }

impl TerminalTask {
    fn capability(self) -> &'static str {
        match self {
            Self::CargoTest => "terminal_cargo_test",
            Self::CargoFmtCheck => "terminal_cargo_fmt_check",
        }
    }
    fn arguments(self) -> &'static [&'static str] {
        match self {
            Self::CargoTest => &["test", "--workspace", "--locked"],
            Self::CargoFmtCheck => &["fmt", "--all", "--", "--check"],
        }
    }
}

fn require_success(success: bool) -> Result<()> {
    if success {
        Ok(())
    } else {
        Err(ServiceError::new("TOOL_EXIT_NONZERO", "Cargo returned a nonzero exit status"))
    }
}

/// The trusted Host must persist InFlight and record the final observed
/// exit status as evidence. A timeout is uncertain; never auto-retry.
pub fn execute_approved_terminal(
    dispatch: &Dispatch,
    resource_id: &str,
    trusted_root: &Path,
    task: TerminalTask,
) -> Result<String> {
    if dispatch.intent.action != Action::Write
        || dispatch.intent.capability_id != task.capability()
        || dispatch.intent.resource_id != resource_id
    {
        return Err(ServiceError::new("DISPATCH_REJECTED", "终端指令未获匹配授权"));
    }
    if !trusted_root.is_absolute() || !trusted_root.is_dir() {
        return Err(ServiceError::new("RESOURCE_UNAVAILABLE", "仓库工作目录无效"));
    }
    let root = trusted_root.to_str().ok_or_else(|| ServiceError::new("INVALID_INPUT", "目录编码无效"))?;
    super::intent_binding::verify_arguments(dispatch, &[root.as_bytes()])?;
    let mut child = Command::new("cargo")
        .args(task.arguments()).current_dir(trusted_root)
        .env("CARGO_TERM_COLOR", "never")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .spawn().map_err(|_| ServiceError::new("TOOL_UNAVAILABLE", "无法启动 Cargo"))?;
    let out = child.stdout.take().ok_or_else(|| ServiceError::new("TOOL_ERROR", "无标准输出"))?;
    let err = child.stderr.take().ok_or_else(|| ServiceError::new("TOOL_ERROR", "无错误输出"))?;
    fn collect(mut source: impl Read + Send + 'static) -> thread::JoinHandle<std::io::Result<Vec<u8>>> {
        thread::spawn(move || {
            let mut bytes = Vec::new();
            source.by_ref().take(64 * 1024 + 1).read_to_end(&mut bytes)?;
            Ok(bytes)
        })
    }
    let stdout = collect(out);
    let stderr = collect(err);
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() < Duration::from_secs(180) => thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ServiceError::new("OUTCOME_UNKNOWN", "Cargo 超时或异常中断；未自动重试"));
            }
        }
    };
    let mut output = stdout.join().map_err(|_| ServiceError::new("TOOL_ERROR", "读取线程中断"))?
        .map_err(|_| ServiceError::new("TOOL_ERROR", "标准输出读取失败"))?;
    let error = stderr.join().map_err(|_| ServiceError::new("TOOL_ERROR", "读取线程中断"))?
        .map_err(|_| ServiceError::new("TOOL_ERROR", "错误输出读取失败"))?;
    if output.len() > 64 * 1024 || error.len() > 64 * 1024 {
        return Err(ServiceError::new("TOOL_OUTPUT_LIMIT", "终端输出超过上限"));
    }
    output.extend(error);
    let report = String::from_utf8_lossy(&output);
    require_success(status.success())?;
    Ok(format!("exit={:?}\n{}", status.code(), report))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wex_core::operation::OperationIntent;
    #[test]
    fn cargo_failure_is_not_a_successful_receipt() {
        assert!(require_success(true).is_ok());
        assert_eq!(require_success(false).unwrap_err().code, "TOOL_EXIT_NONZERO");
    }

    #[test]
    fn rejects_wrong_command_before_spawn() {
        let dispatch = Dispatch {
            intent: OperationIntent {
                id: "op".into(), actor_id: "a".into(), conversation_id: "c".into(),
                context_id: None, connection_id: "terminal".into(),
                capability_id: "terminal_cargo_fmt_check".into(), resource_id: "repo".into(),
                action: Action::Write, expected_version: None, intent_digest: "0".repeat(64),
            },
            attempt_id: "try".into(),
        };
        assert_eq!(
            execute_approved_terminal(&dispatch, "repo", Path::new("/none"), TerminalTask::CargoTest)
                .unwrap_err().code, "DISPATCH_REJECTED"
        );
    }
}
