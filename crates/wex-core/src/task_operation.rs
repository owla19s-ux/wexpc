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
        if link.task_id.is_empty() || link.operation_id.is_empty() || link.agent_id.is_empty() {
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
        match record.state {
            OperationState::Succeeded { evidence_ref, .. } => {
                tasks.record_evidence(&link.task_id, evidence_ref)
                    .map_err(LinkError::Task)?;
            }
            OperationState::Failed { evidence_ref, .. } => {
                if task.status == TaskStatus::Running {
                    tasks.transition(&link.task_id, TaskStatus::Failed, Some(evidence_ref))
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
