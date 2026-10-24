//! Task execution admission gate: reservations live until the execution lease drops.
//! This module does not execute tools, grant permissions, or persist task state.
use crate::{ReservationError, ScopeRegistry, ScopeReservation};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskPhase { Running, Completed, Failed, Cancelled }

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskError { AlreadyRunning, NotRunning, Scope(ReservationError), Unavailable }

struct RunningTask {
    _lease: ScopeReservation,
    generation: u64,
}

#[derive(Default)]
struct State {
    next_generation: u64,
    running: HashMap<String, RunningTask>,
}

#[derive(Clone, Default)]
pub struct TaskGate {
    registry: ScopeRegistry,
    state: Arc<Mutex<State>>,
}

pub struct TaskLease {
    gate: TaskGate,
    task_id: String,
    generation: u64,
}

impl TaskGate {
    /// Admission is atomic across both the task ID and all requested path scopes.
    pub fn start(&self, task_id: &str, paths: &[String]) -> Result<TaskLease, TaskError> {
        if task_id.trim().is_empty() { return Err(TaskError::Unavailable); }
        let mut state = self.state.lock().map_err(|_| TaskError::Unavailable)?;
        if state.running.contains_key(task_id) { return Err(TaskError::AlreadyRunning); }
        let lease = self.registry.reserve(paths).map_err(TaskError::Scope)?;
        let generation = state.next_generation.checked_add(1).ok_or(TaskError::Unavailable)?;
        state.next_generation = generation;
        state.running.insert(task_id.to_owned(), RunningTask { _lease: lease, generation });
        Ok(TaskLease { gate: self.clone(), task_id: task_id.to_owned(), generation })
    }

    /// Execute a synchronous operation while holding its path reservation.\n    /// The caller remains responsible for authorization, path resolution and persistence.\n    pub fn execute<T, E>(&self, task_id: &str, paths: &[String], action: impl FnOnce() -> Result<T, E>) -> Result<Result<T, E>, TaskError> {\n        let lease = self.start(task_id, paths)?;\n        let result = action();\n        let phase = if result.is_ok() { TaskPhase::Completed } else { TaskPhase::Failed };\n        lease.finish(phase)?;\n        Ok(result)\n    }\n\n    pub fn is_running(&self, task_id: &str) -> Result<bool, TaskError> {
        Ok(self.state.lock().map_err(|_| TaskError::Unavailable)?.running.contains_key(task_id))
    }

    fn release(&self, task_id: &str, generation: u64) -> Result<(), TaskError> {
        let mut state = self.state.lock().map_err(|_| TaskError::Unavailable)?;
        if state.running.get(task_id).map(|t| t.generation) != Some(generation) {
            return Err(TaskError::NotRunning);
        }
        state.running.remove(task_id);
        Ok(())
    }
}

impl TaskLease {
    /// Release on any terminal outcome. Persist the outcome separately before calling this.
    pub fn finish(self, _phase: TaskPhase) -> Result<(), TaskError> {
        self.gate.release(&self.task_id, self.generation)
    }
}

impl Drop for TaskLease {
    fn drop(&mut self) {
        let _ = self.gate.release(&self.task_id, self.generation);
    }
}
