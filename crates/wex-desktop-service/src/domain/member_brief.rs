use super::{Agent, Project, Task, TaskRecord};
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemberBriefScope {
    Project,
    AssignedTasks,
}

/// Read-only work context, without credentials, tool grants or chat history.
#[derive(Debug, Clone, Serialize)]
pub struct MemberWorkBrief {
    pub revision: u64,
    pub scope: MemberBriefScope,
    pub member: Agent,
    pub project: Project,
    pub members: Vec<Agent>,
    pub tasks: Vec<Task>,
    pub task_records: Vec<TaskRecord>,
}
