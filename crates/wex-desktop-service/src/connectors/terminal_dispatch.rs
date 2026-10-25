//! Explicitly approved, allowlisted terminal execution (no shell).
//! Cargo builds may execute repository build scripts: always require a separate
//! human-approved command intent, never infer access from a project link.
use crate::{Result, ServiceError};
use std::{io::Read, path::Path};
use wex_core::operation::{Action, Dispatch};

/// The only commands allowed by this first-stage terminal adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalTask {
    CargoTest,
    CargoFmtCheck,
}

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
        Err(ServiceError::new(
            "TOOL_EXIT_NONZERO",
            "Cargo returned a nonzero exit status",
        ))
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
    let report = execute_approved_terminal_report(dispatch, resource_id, trusted_root, task)?;
    require_success(report.success)?;
    Ok(report.output)
}

#[derive(Debug)]
pub struct TerminalReport {
    pub success: bool,
    pub exit_code: Option<i32>,
    pub output: String,
}

/// The caller may record a confirmed nonzero exit and hand bounded diagnostic
/// output back to the model. A nonzero exit is never a success receipt.
pub fn execute_approved_terminal_report(
    dispatch: &Dispatch,
    resource_id: &str,
    trusted_root: &Path,
    task: TerminalTask,
) -> Result<TerminalReport> {
    if dispatch.intent.action != Action::Write
        || dispatch.intent.capability_id != task.capability()
        || dispatch.intent.resource_id != resource_id
    {
        return Err(ServiceError::new(
            "DISPATCH_REJECTED",
            "终端指令未获匹配授权",
        ));
    }
    if !trusted_root.is_absolute() || !trusted_root.is_dir() {
        return Err(ServiceError::new(
            "RESOURCE_UNAVAILABLE",
            "仓库工作目录无效",
        ));
    }
    let root = trusted_root
        .to_str()
        .ok_or_else(|| ServiceError::new("INVALID_INPUT", "目录编码无效"))?;
    super::intent_binding::verify_arguments(dispatch, &[root.as_bytes()])?;
    let report = super::process_dispatch::run_process(
        trusted_root,
        &super::process_dispatch::ProcessRequest {
            program: "cargo".into(),
            arguments: task.arguments().iter().map(|s| (*s).into()).collect(),
            timeout_ms: 180_000,
        },
    )?;
    let truncated = report.stdout_truncated || report.stderr_truncated;
    Ok(TerminalReport {
        success: report.success,
        exit_code: report.exit_code,
        output: format!(
            "{}{}{}",
            report.stdout,
            report.stderr,
            if truncated {
                "\n[输出已截断；退出状态为实际进程结果]"
            } else {
                ""
            }
        ),
    })
}

// Keep at most `limit` bytes in memory, but drain the entire pipe.
// Stopping the read early can make a successful Cargo process fail with a
// broken pipe (or block while writing) before its true exit status is known.
pub(super) fn collect_bounded_output(
    mut source: impl Read,
    limit: usize,
) -> std::io::Result<(Vec<u8>, bool)> {
    let mut output = Vec::new();
    let mut truncated = false;
    let mut chunk = [0u8; 8192];
    loop {
        let count = source.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        let kept = count.min(limit.saturating_sub(output.len()));
        output.extend_from_slice(&chunk[..kept]);
        truncated |= kept < count;
    }
    Ok((output, truncated))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wex_core::operation::OperationIntent;
    #[test]
    fn oversized_output_is_drained_without_unbounded_retention() {
        let mut source = std::io::Cursor::new(vec![b'x'; 128 * 1024]);
        let (output, truncated) = collect_bounded_output(&mut source, 64 * 1024).unwrap();
        assert!(truncated);
        assert_eq!(output.len(), 64 * 1024);
        assert_eq!(source.position(), 128 * 1024);
    }

    #[test]
    fn exact_limit_and_short_output_are_not_truncated() {
        for size in [0, 4096, 64 * 1024] {
            let mut source = std::io::Cursor::new(vec![b'y'; size]);
            let (output, truncated) = collect_bounded_output(&mut source, 64 * 1024).unwrap();
            assert!(!truncated);
            assert_eq!(output.len(), size);
            assert_eq!(source.position(), size as u64);
        }
    }

    #[test]
    fn cargo_failure_is_not_a_successful_receipt() {
        assert!(require_success(true).is_ok());
        assert_eq!(
            require_success(false).unwrap_err().code,
            "TOOL_EXIT_NONZERO"
        );
    }

    #[test]
    fn rejects_wrong_command_before_spawn() {
        let dispatch = Dispatch {
            intent: OperationIntent {
                id: "op".into(),
                actor_id: "a".into(),
                conversation_id: "c".into(),
                context_id: None,
                connection_id: "terminal".into(),
                capability_id: "terminal_cargo_fmt_check".into(),
                resource_id: "repo".into(),
                action: Action::Write,
                expected_version: None,
                intent_digest: "0".repeat(64),
            },
            attempt_id: "try".into(),
        };
        assert_eq!(
            execute_approved_terminal(
                &dispatch,
                "repo",
                Path::new("/none"),
                TerminalTask::CargoTest
            )
            .unwrap_err()
            .code,
            "DISPATCH_REJECTED"
        );
    }
}
