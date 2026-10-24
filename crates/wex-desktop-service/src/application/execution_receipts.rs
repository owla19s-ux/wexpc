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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unassigned_executor_outcome_without_writing_receipt() {
        let mut app = DesktopService::memory().unwrap();
        let project = app.create_project("PC".into(), "".into(), vec![]).unwrap();
        let task = app.create_task(NewTask {
            project_id: project.id,
            title: "Test".into(),
            goal: "".into(),
            scope: vec![],
            acceptance: "".into(),
            agent_ids: vec![],
        }).unwrap();
        let error = app.record_executor_outcome(ExecutorOutcome {
            task_id: task.id,
            agent_id: "not_assigned".into(),
            tool_name: "test".into(),
            invocation_id: "invocation-1".into(),
            succeeded: true,
            summary: "untrusted fixture".into(),
        }).unwrap_err();
        assert_eq!(error.code, "INVALID_BINDING");
        assert!(app.snapshot().unwrap().execution_receipts.is_empty());
    }

    #[test]
    fn rejects_blank_invocation_before_any_storage_change() {
        let mut app = DesktopService::memory().unwrap();
        let before = app.snapshot().unwrap().revision;
        let error = app.record_executor_outcome(ExecutorOutcome {
            task_id: "missing".into(),
            agent_id: "missing".into(),
            tool_name: "tool".into(),
            invocation_id: " ".into(),
            succeeded: false,
            summary: "".into(),
        }).unwrap_err();
        assert_eq!(error.code, "INVALID_INPUT");
        assert_eq!(app.snapshot().unwrap().revision, before);
    }
    #[test]
    fn assigned_outcome_persists_and_duplicate_invocation_is_rejected() {
        let mut app = DesktopService::memory().unwrap();
        let project = app.create_project("PC".into(), "".into(), vec![]).unwrap();
        let connection = app.add_connection(NewConnection {
            name: "API".into(),
            base_url: "https://api.deepseek.com".into(),
            models: vec!["deepseek-chat".into()],
        }, "vault".into()).unwrap();
        let agent = app.add_agent(NewAgent {
            project_id: project.id.clone(),
            name: "Worker".into(),
            role: AgentRole::Worker,
            connection_id: connection.id,
            model_id: "deepseek-chat".into(),
        }).unwrap();
        let task = app.create_task(NewTask {
            project_id: project.id,
            title: "Execute".into(),
            goal: "".into(),
            scope: vec![],
            acceptance: "".into(),
            agent_ids: vec![agent.id.clone()],
        }).unwrap();
        let make_outcome = || ExecutorOutcome {
            task_id: task.id.clone(),
            agent_id: agent.id.clone(),
            tool_name: "fixture".into(),
            invocation_id: "same-call".into(),
            succeeded: false,
            summary: "simulated result; no tool was invoked".into(),
        };
        let before = app.snapshot().unwrap().revision;
        let receipt = app.record_executor_outcome(make_outcome()).unwrap();
        assert_eq!(receipt.recorded_revision, before + 1);
        assert!(!receipt.succeeded);
        assert_eq!(app.member_brief(&agent.id).unwrap().execution_receipts.len(), 1);
        let after = app.snapshot().unwrap().revision;
        assert_eq!(app.record_executor_outcome(make_outcome()).unwrap_err().code, "DUPLICATE_INVOCATION");
        assert_eq!(app.snapshot().unwrap().revision, after);
        assert_eq!(app.snapshot().unwrap().execution_receipts.len(), 1);
        assert_eq!(app.snapshot().unwrap().tasks[0].status, TaskStatus::Draft);
    }

}
