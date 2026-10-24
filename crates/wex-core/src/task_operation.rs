//! Trusted, read-only projection from operation ledger receipts into task progress.
//! This does not approve, dispatch, verify or persist operations. The host must
//! persist operation and task changes transactionally before exposing them.
use crate::operation::{OperationLedger, OperationState};
use crate::task::{TaskBoard, TaskError, TaskStatus};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskOperationLink {
    pub task_id: String,
    pub operation_id: String,
    pub agent_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkError {
    InvalidLink,
    DuplicateOperation,
    MissingTask,
    MissingOperation,
    AgentMismatch,
    Task(TaskError),
}

#[derive(Debug, Default)]
pub struct TaskOperationIndex {
    links: BTreeMap<String, TaskOperationLink>,
}

fn valid_link_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

impl TaskOperationIndex {
    /// Restore only after the host validates the saved snapshot's provenance.
    pub fn restore(
        links: Vec<TaskOperationLink>,
        tasks: &TaskBoard,
        ledger: &OperationLedger,
    ) -> Result<Self, LinkError> {
        let mut index = Self::default();
        for link in links {
            index.attach(link, tasks, ledger)?;
        }
        Ok(index)
    }

    pub fn snapshot(&self) -> Vec<TaskOperationLink> {
        self.links.values().cloned().collect()
    }

    /// A link never grants authority to execute an operation.
    pub fn attach(
        &mut self,
        link: TaskOperationLink,
        tasks: &TaskBoard,
        ledger: &OperationLedger,
    ) -> Result<(), LinkError> {
        if !valid_link_id(&link.task_id) || !valid_link_id(&link.operation_id) || !valid_link_id(&link.agent_id) {
            return Err(LinkError::InvalidLink);
        }
        if self.links.contains_key(&link.operation_id) {
            return Err(LinkError::DuplicateOperation);
        }
        let task = tasks.get(&link.task_id).ok_or(LinkError::MissingTask)?;
        if task.assigned_agent.as_deref() != Some(&link.agent_id) {
            return Err(LinkError::AgentMismatch);
        }
        let record = ledger.view().records.into_iter()
            .find(|record| record.intent.id == link.operation_id)
            .ok_or(LinkError::MissingOperation)?;
        if record.intent.actor_id != link.agent_id {
            return Err(LinkError::AgentMismatch);
        }
        if matches!(task.status, TaskStatus::Completed | TaskStatus::Failed) {
            return Err(LinkError::InvalidLink);
        }
        self.links.insert(link.operation_id.clone(), link);
        Ok(())
    }

    /// A successful platform receipt is evidence of an operation, not proof of
    /// review, test acceptance or overall task completion.
    pub fn reconcile(
        &self,
        operation_id: &str,
        tasks: &mut TaskBoard,
        ledger: &OperationLedger,
    ) -> Result<(), LinkError> {
        let link = self.links.get(operation_id).ok_or(LinkError::MissingOperation)?;
        let record = ledger.view().records.into_iter()
            .find(|record| record.intent.id == operation_id)
            .ok_or(LinkError::MissingOperation)?;
        if record.intent.actor_id != link.agent_id {
            return Err(LinkError::AgentMismatch);
        }
        let task = tasks.get(&link.task_id).ok_or(LinkError::MissingTask)?;
        if task.assigned_agent.as_deref() != Some(&link.agent_id) {
            return Err(LinkError::AgentMismatch);
        }
        let status = task.status.clone();
        match record.state {
            OperationState::Succeeded { evidence_ref, .. } => {
                tasks.record_evidence(&link.task_id, evidence_ref)
                    .map_err(LinkError::Task)?;
            }
            OperationState::Failed { evidence_ref, .. } => {
                if status != TaskStatus::Completed {
                    tasks.record_evidence(&link.task_id, evidence_ref)
                        .map_err(LinkError::Task)?;
                }
                if status == TaskStatus::Running {
                    tasks.transition(&link.task_id, TaskStatus::Failed, None)
                        .map_err(LinkError::Task)?;
                }
            }
            OperationState::OutcomeUnknown { .. } => {
                if task.status == TaskStatus::Running {
                    tasks.transition(&link.task_id, TaskStatus::Blocked, None)
                        .map_err(LinkError::Task)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operation::{Action, ExecutionChecks, OperationIntent, Outcome};
    use crate::task::Task;

    fn setup() -> (TaskBoard, OperationLedger) {
        let mut tasks = TaskBoard::default();
        tasks.insert(Task {
            id: "task_1".into(), project_id: "pc".into(),
            objective: "Build project".into(), assigned_agent: Some("agent_a".into()),
            status: TaskStatus::Running, evidence_refs: vec![],
        }).unwrap();
        let mut ledger = OperationLedger::default();
        let intent = OperationIntent {
            id: "op_1".into(), actor_id: "agent_a".into(),
            conversation_id: "chat_1".into(), context_id: None,
            connection_id: "local".into(), capability_id: "build".into(),
            resource_id: "project".into(), action: Action::Write,
            expected_version: None, intent_digest: "a".repeat(64),
        };
        ledger.propose(intent.clone()).unwrap();
        ledger.approve("op_1", &intent, 1).unwrap();
        ledger.begin("op_1", "attempt_1", ExecutionChecks {
            authority_revision: 1, connection_enabled: true, authenticated: true,
            platform_available: true, resource_allowed: true,
        }).unwrap();
        (tasks, ledger)
    }

    fn link() -> TaskOperationLink {
        TaskOperationLink {
            task_id: "task_1".into(), operation_id: "op_1".into(),
            agent_id: "agent_a".into(),
        }
    }

    #[test]
    fn success_records_evidence_without_completing_task() {
        let (mut tasks, mut ledger) = setup();
        let mut index = TaskOperationIndex::default();
        index.attach(link(), &tasks, &ledger).unwrap();
        ledger.finish("op_1", "attempt_1", Outcome::Succeeded {
            evidence_ref: "build_log_1".into(),
        }).unwrap();
        index.reconcile("op_1", &mut tasks, &ledger).unwrap();
        index.reconcile("op_1", &mut tasks, &ledger).unwrap();
        let task = tasks.get("task_1").unwrap();
        assert_eq!(task.status, TaskStatus::Running);
        assert_eq!(task.evidence_refs, vec!["build_log_1"]);
    }

    #[test]
    fn unknown_outcome_blocks_task_without_false_success() {
        let (mut tasks, mut ledger) = setup();
        let index = TaskOperationIndex::restore(vec![link()], &tasks, &ledger).unwrap();
        ledger.finish("op_1", "attempt_1", Outcome::Unknown).unwrap();
        index.reconcile("op_1", &mut tasks, &ledger).unwrap();
        assert_eq!(tasks.get("task_1").unwrap().status, TaskStatus::Blocked);
        assert!(tasks.get("task_1").unwrap().evidence_refs.is_empty());
    }

    #[test]
    fn failed_receipt_is_recorded_once_and_marks_running_task_failed() {
        let (mut tasks, mut ledger) = setup();
        let index = TaskOperationIndex::restore(vec![link()], &tasks, &ledger).unwrap();
        ledger.finish("op_1", "attempt_1", Outcome::Failed {
            evidence_ref: "error_log_1".into(),
        }).unwrap();
        index.reconcile("op_1", &mut tasks, &ledger).unwrap();
        index.reconcile("op_1", &mut tasks, &ledger).unwrap();
        assert_eq!(tasks.get("task_1").unwrap().status, TaskStatus::Failed);
        assert_eq!(tasks.get("task_1").unwrap().evidence_refs, vec!["error_log_1"]);
    }

    #[test]
    fn reject_agent_mismatch_and_duplicate_operation_link() {
        let (tasks, ledger) = setup();
        let mut index = TaskOperationIndex::default();
        let mut wrong = link();
        wrong.agent_id = "agent_b".into();
        assert_eq!(index.attach(wrong, &tasks, &ledger), Err(LinkError::AgentMismatch));
        index.attach(link(), &tasks, &ledger).unwrap();
        assert_eq!(index.attach(link(), &tasks, &ledger), Err(LinkError::DuplicateOperation));
    }
}
