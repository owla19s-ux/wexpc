//! Persistent-friendly task assignment facts; no tool execution or implicit authorization.
//! A task is not required for ordinary conversation tool calls.
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskStatus {
    Queued,
    Running,
    Blocked,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub id: String,
    pub project_id: String,
    pub objective: String,
    pub assigned_agent: Option<String>,
    pub status: TaskStatus,
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskError {
    InvalidInput,
    AlreadyExists,
    NotFound,
    InvalidTransition,
}

#[derive(Debug, Default)]
pub struct TaskBoard {
    tasks: BTreeMap<String, Task>,
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

fn valid(task: &Task) -> bool {
    valid_id(&task.id)
        && valid_id(&task.project_id)
        && !task.objective.trim().is_empty()
        && task.objective.len() <= 8192
        && task.assigned_agent.as_deref().is_none_or(valid_id)
        && task.evidence_refs.iter().all(|id| valid_id(id))
        && (!matches!(task.status, TaskStatus::Running) || task.assigned_agent.is_some())
}

impl TaskBoard {
    pub fn restore(tasks: Vec<Task>) -> Result<Self, TaskError> {
        let mut board = Self::default();
        for mut task in tasks {
            // An interrupted execution is not proof of success or failure.
            if matches!(task.status, TaskStatus::Running) {
                task.status = TaskStatus::Blocked;
            }
            board.insert(task)?;
        }
        Ok(board)
    }

    pub fn get(&self, id: &str) -> Option<&Task> {
        self.tasks.get(id)
    }

    pub fn tasks_for_project(&self, project_id: &str) -> Vec<Task> {
        self.tasks.values().filter(|task| task.project_id == project_id).cloned().collect()
    }

    pub fn tasks_for_agent(&self, agent_id: &str) -> Vec<Task> {
        self.tasks.values().filter(|task| task.assigned_agent.as_deref() == Some(agent_id)).cloned().collect()
    }

    pub fn snapshot(&self) -> Vec<Task> {
        self.tasks.values().cloned().collect()
    }

    pub fn insert(&mut self, task: Task) -> Result<(), TaskError> {
        if !valid(&task) {
            return Err(TaskError::InvalidInput);
        }
        if self.tasks.contains_key(&task.id) {
            return Err(TaskError::AlreadyExists);
        }
        self.tasks.insert(task.id.clone(), task);
        Ok(())
    }

    pub fn assign(&mut self, id: &str, agent: String) -> Result<(), TaskError> {
        if !valid_id(&agent) {
            return Err(TaskError::InvalidInput);
        }
        let task = self.tasks.get_mut(id).ok_or(TaskError::NotFound)?;
        if !matches!(task.status, TaskStatus::Queued | TaskStatus::Blocked) {
            return Err(TaskError::InvalidTransition);
        }
        task.assigned_agent = Some(agent);
        Ok(())
    }

    pub fn transition(
        &mut self,
        id: &str,
        next: TaskStatus,
        evidence: Option<String>,
    ) -> Result<(), TaskError> {
        if evidence.as_deref().is_some_and(|id| !valid_id(id)) {
            return Err(TaskError::InvalidInput);
        }
        let task = self.tasks.get_mut(id).ok_or(TaskError::NotFound)?;
        let allowed = matches!(
            (&task.status, &next),
            (TaskStatus::Queued, TaskStatus::Running)
                | (TaskStatus::Running, TaskStatus::Blocked)
                | (TaskStatus::Running, TaskStatus::Completed)
                | (TaskStatus::Running, TaskStatus::Failed)
                | (TaskStatus::Blocked, TaskStatus::Running)
                | (TaskStatus::Failed, TaskStatus::Queued)
        );
        if !allowed || (matches!(next, TaskStatus::Running) && task.assigned_agent.is_none()) {
            return Err(TaskError::InvalidTransition);
        }
        if let Some(reference) = evidence {
            task.evidence_refs.push(reference);
        }
        task.status = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn task(id: &str) -> Task {
        Task {
            id: id.into(), project_id: "wexpc".into(), objective: "Implement adapter".into(),
            assigned_agent: None, status: TaskStatus::Queued, evidence_refs: vec![],
        }
    }
    #[test]
    fn distinct_agents_and_evidence() {
        let mut board = TaskBoard::default();
        board.insert(task("a")).unwrap();
        board.insert(task("b")).unwrap();
        board.assign("a", "agent_one".into()).unwrap();
        board.assign("b", "agent_two".into()).unwrap();
        board.transition("a", TaskStatus::Running, None).unwrap();
        board.transition("b", TaskStatus::Running, None).unwrap();
        board.transition("a", TaskStatus::Completed, Some("commit_1".into())).unwrap();
        let snapshot = board.snapshot();
        assert_eq!(snapshot[0].evidence_refs, vec!["commit_1"]);
        assert_eq!(snapshot[1].assigned_agent.as_deref(), Some("agent_two"));
    }
    #[test]
    fn restore_does_not_claim_running_work_completed() {
        let mut t = task("a");
        t.assigned_agent = Some("agent".into());
        t.status = TaskStatus::Running;
        let board = TaskBoard::restore(vec![t]).unwrap();
        assert_eq!(board.snapshot()[0].status, TaskStatus::Blocked);
    }
    #[test]
    fn project_and_agent_views_are_isolated() {
        let mut board = TaskBoard::default();
        let mut other = task("b");
        other.project_id = "android".into();
        board.insert(task("a")).unwrap();
        board.insert(other).unwrap();
        board.assign("a", "agent_one".into()).unwrap();
        board.assign("b", "agent_two".into()).unwrap();
        assert_eq!(board.tasks_for_project("wexpc").len(), 1);
        assert_eq!(board.tasks_for_project("android").len(), 1);
        assert_eq!(board.tasks_for_agent("agent_one")[0].id, "a");
        assert_eq!(board.get("b").unwrap().project_id, "android");
    }
    #[test]
    fn cold_restore_preserves_evidence_and_blocks_only_inflight_work() {
        let mut board = TaskBoard::default();
        board.insert(task("a")).unwrap();
        board.insert(task("b")).unwrap();
        board.assign("a", "agent_one".into()).unwrap();
        board.transition("a", TaskStatus::Running, None).unwrap();
        board.transition("a", TaskStatus::Completed, Some("commit_a".into())).unwrap();
        board.assign("b", "agent_two".into()).unwrap();
        board.transition("b", TaskStatus::Running, None).unwrap();
        let restored = TaskBoard::restore(board.snapshot()).unwrap();
        assert_eq!(restored.get("a").unwrap().status, TaskStatus::Completed);
        assert_eq!(restored.get("a").unwrap().evidence_refs, vec!["commit_a"]);
        assert_eq!(restored.get("b").unwrap().status, TaskStatus::Blocked);
    }
    #[test]
    fn invalid_transitions_and_duplicate_ids() {
        let mut board = TaskBoard::default();
        board.insert(task("a")).unwrap();
        assert_eq!(board.insert(task("a")), Err(TaskError::AlreadyExists));
        assert_eq!(board.transition("a", TaskStatus::Completed, None), Err(TaskError::InvalidTransition));
        assert_eq!(board.transition("a", TaskStatus::Running, None), Err(TaskError::InvalidTransition));
    }
}
