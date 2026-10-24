use super::{id, invalid, text};
use crate::*;

/// Internal-only data produced by a trusted executor, never by a model message.
pub(super) struct ExecutorOutcome {
    pub task_id: String,
    pub agent_id: String,
    pub tool_name: String,
    pub invocation_id: String,
    pub succeeded: bool,
    pub summary: String,
}

impl DesktopService {
    #[allow(dead_code)]
    pub(super) fn record_executor_outcome(&mut self, outcome: ExecutorOutcome) -> Result<ExecutionReceipt> {
        if outcome.tool_name.trim().is_empty() || outcome.invocation_id.trim().is_empty() {
            return Err(invalid("工具名称与调用标识不能为空"));
        }
        text(&outcome.tool_name)?;
        text(&outcome.invocation_id)?;
        text(&outcome.summary)?;
        self.mutate(|snapshot| {
            let task = snapshot.tasks.iter().find(|t| t.id == outcome.task_id)
                .ok_or_else(|| ServiceError::new("TASK_NOT_FOUND", "任务不存在"))?;
            if !task.agent_ids.contains(&outcome.agent_id)
                || !snapshot.agents.iter().any(|a| a.id == outcome.agent_id && a.project_id == task.project_id)
            {
                return Err(ServiceError::new("INVALID_BINDING", "执行成员不属于当前任务"));
            }
            if snapshot.execution_receipts.iter().any(|r| r.invocation_id == outcome.invocation_id) {
                return Err(ServiceError::new("DUPLICATE_INVOCATION", "该调用已记录执行结果"));
            }
            let receipt = ExecutionReceipt {
                id: id("receipt"),
                task_id: task.id.clone(),
                agent_id: outcome.agent_id,
                tool_name: outcome.tool_name,
                invocation_id: outcome.invocation_id,
                succeeded: outcome.succeeded,
                summary: outcome.summary,
                task_snapshot: task.clone(),
                recorded_revision: snapshot.revision.checked_add(1)
                    .ok_or_else(|| ServiceError::new("STORAGE_ERROR", "回执序号溢出"))?,
            };
            snapshot.execution_receipts.push(receipt.clone());
            Ok(receipt)
        })
    }
}
