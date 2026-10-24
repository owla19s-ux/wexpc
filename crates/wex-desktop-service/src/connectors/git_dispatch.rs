//! Read-only local Git status, admitted only via a consumed Core Dispatch.
use crate::{Result, ServiceError};
use std::{io::Read, path::Path, process::{Command, Stdio}, thread, time::{Duration, Instant}};
use wex_core::operation::{Action, Dispatch};

pub fn approved_git_status(dispatch: &Dispatch, repo_id: &str, trusted_root: &Path) -> Result<String> {
    if dispatch.intent.action != Action::Read
        || dispatch.intent.capability_id != "git_status"
        || dispatch.intent.resource_id != repo_id
    {
        return Err(ServiceError::new("DISPATCH_REJECTED", "Git 操作不符合已批准范围"));
    }
    if !trusted_root.is_absolute() || !trusted_root.is_dir() {
        return Err(ServiceError::new("RESOURCE_UNAVAILABLE", "受信任仓库目录不可用"));
    }
    let root = trusted_root.to_str()
        .ok_or_else(|| ServiceError::new("INVALID_INPUT", "仓库路径编码不可用"))?;
    super::intent_binding::verify_arguments(dispatch, &[root.as_bytes()])?;
    let mut child = Command::new("git")
        .arg("--no-optional-locks")
        .args(["-C"]).arg(trusted_root)
        .args(["status", "--porcelain=v1", "-uno"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null())
        .spawn()
        .map_err(|_| ServiceError::new("TOOL_UNAVAILABLE", "Git 进程无法启动"))?;
    let mut stdout = child.stdout.take().ok_or_else(|| {
        ServiceError::new("TOOL_UNAVAILABLE", "Git 输出不可用")
    })?;
    let reader = thread::spawn(move || {
        let mut output = Vec::new();
        stdout.by_ref().take(64 * 1024 + 1).read_to_end(&mut output).map(|_| output)
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < Duration::from_secs(15) => {
                thread::sleep(Duration::from_millis(30));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ServiceError::new("TOOL_TIMEOUT", "Git 状态读取超时或中断"));
            }
        }
    };
    let output = reader.join().map_err(|_| ServiceError::new("TOOL_ERROR", "Git 读取线程失败"))?
        .map_err(|_| ServiceError::new("TOOL_ERROR", "Git 输出读取失败"))?;
    if !status.success() || output.len() > 64 * 1024 {
        return Err(ServiceError::new("TOOL_ERROR", "Git 返回错误或输出超过上限"));
    }
    String::from_utf8(output)
        .map_err(|_| ServiceError::new("TOOL_ERROR", "Git 输出不是 UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wex_core::operation::OperationIntent;
    #[test]
    fn refuses_unmatched_operation_before_spawning_git() {
        let dispatch = Dispatch {
            intent: OperationIntent {
                id: "op".into(), actor_id: "a".into(), conversation_id: "c".into(),
                context_id: None, connection_id: "local".into(),
                capability_id: "git_status".into(), resource_id: "repository".into(),
                action: Action::Read, expected_version: None, intent_digest: "0".repeat(64),
            },
            attempt_id: "attempt".into(),
        };
        assert_eq!(
            approved_git_status(&dispatch, "different", Path::new("/not/a/repository"))
                .unwrap_err().code, "DISPATCH_REJECTED"
        );
    }
}
