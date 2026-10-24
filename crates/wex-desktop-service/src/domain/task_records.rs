use super::Task;
use serde::{Deserialize, Serialize};

/// User-authored notes, never trusted execution/verification receipts.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskRecordKind {
    Progress,
    Blocker,
    Handoff,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskRecordSource {
    User,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewTaskRecord {
    pub task_id: String,
    pub kind: TaskRecordKind,
    pub body: String,
    /// Unverified references; recording does not access these resources.
    pub references: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRecord {
    pub source: TaskRecordSource,
    pub id: String,
    pub task_id: String,
    pub kind: TaskRecordKind,
    pub body: String,
    pub references: Vec<String>,
    /// Exact task content at admission; later edits cannot rewrite this context.
    pub task_snapshot: Task,
    /// Durable append order, assigned inside the same store transaction.
    pub recorded_revision: u64,
}

/// A manually supplied review note. It is not a trusted tool receipt.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewReviewNote {
    pub task_id: String,
    pub body: String,
    pub references: Vec<String>,
}

/// Self-reported Agent progress is distinct from user notes and trusted tool receipts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentWorkReport {
    pub id: String,
    pub task_id: String,
    pub agent_id: String,
    pub body: String,
    pub references: Vec<String>,
    pub task_snapshot: Task,
    pub recorded_revision: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewAgentWorkReport {
    pub task_id: String,
    pub agent_id: String,
    pub body: String,
    pub references: Vec<String>,
}
