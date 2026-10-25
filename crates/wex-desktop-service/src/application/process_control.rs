//! Live Host process controls, not a durable task queue or recovery authority.
use crate::{ProcessRequest, Result, ServiceError};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc, Mutex,
    },
    time::Instant,
};

#[derive(Clone, Default)]
// 0 active; 1 cancellation requested; 2 completed; 3 cancelled and completed.
// A single atomic prevents accepting a late cancellation after completion wins.
pub struct ProcessCancellation(Arc<AtomicU8>);
impl ProcessCancellation {
    /// The cancellation request wins only while the attempt is unsettled.
    pub fn cancel(&self) -> bool {
        match self
            .0
            .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
        {
            Ok(_) | Err(1) => true,
            Err(_) => false,
        }
    }
    pub fn is_cancelled(&self) -> bool {
        matches!(self.0.load(Ordering::SeqCst), 1 | 3)
    }
    pub fn is_finished(&self) -> bool {
        self.0.load(Ordering::SeqCst) >= 2
    }
    pub(crate) fn finish(&self) -> bool {
        loop {
            let current = self.0.load(Ordering::SeqCst);
            let next = match current {
                0 => 2,
                1 => 3,
                _ => return current == 2,
            };
            if self
                .0
                .compare_exchange(current, next, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return current == 0;
            }
        }
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct ActiveProcess {
    pub conversation_id: String,
    pub attempt_id: String,
    pub operation_id: String,
    pub program: String,
    pub arguments: Vec<String>,
    pub cancellation_requested: bool,
    pub finished: bool,
    pub elapsed_ms: u64,
}
struct Entry {
    view: ActiveProcess,
    control: ProcessCancellation,
    start: Instant,
}
#[derive(Default)]
pub struct ProcessSupervisor {
    entries: Mutex<BTreeMap<String, Entry>>,
}
pub struct ProcessLease {
    supervisor: Arc<ProcessSupervisor>,
    attempt: String,
    control: ProcessCancellation,
}
fn unavailable() -> ServiceError {
    ServiceError::new("PROCESS_CONTROL_ERROR", "当前进程控制状态不可用")
}
impl ProcessSupervisor {
    pub fn start(
        self: &Arc<Self>,
        conversation: &str,
        attempt: &str,
        operation: &str,
        request: &ProcessRequest,
    ) -> Result<ProcessLease> {
        request.validate()?;
        if [conversation, attempt, operation]
            .iter()
            .any(|v| v.is_empty() || v.len() > 128)
        {
            return Err(unavailable());
        }
        let mut entries = self.entries.lock().map_err(|_| unavailable())?;
        if entries.contains_key(attempt) {
            return Err(ServiceError::new(
                "PROCESS_ALREADY_RUNNING",
                "执行尝试已登记",
            ));
        }
        let control = ProcessCancellation::default();
        entries.insert(
            attempt.into(),
            Entry {
                view: ActiveProcess {
                    conversation_id: conversation.into(),
                    attempt_id: attempt.into(),
                    operation_id: operation.into(),
                    program: request.program.clone(),
                    arguments: request.arguments.clone(),
                    cancellation_requested: false,
                    finished: false,
                    elapsed_ms: 0,
                },
                control: control.clone(),
                start: Instant::now(),
            },
        );
        Ok(ProcessLease {
            supervisor: self.clone(),
            attempt: attempt.into(),
            control,
        })
    }
    pub fn list(&self, conversation: &str) -> Result<Vec<ActiveProcess>> {
        Ok(self
            .entries
            .lock()
            .map_err(|_| unavailable())?
            .values()
            .filter(|e| e.view.conversation_id == conversation)
            .map(|e| {
                let mut view = e.view.clone();
                view.cancellation_requested = e.control.is_cancelled();
                view.finished = e.control.is_finished();
                view.elapsed_ms = e.start.elapsed().as_millis().try_into().unwrap_or(u64::MAX);
                view
            })
            .collect())
    }
    pub fn cancel(&self, conversation: &str, attempt: &str) -> Result<()> {
        let entries = self.entries.lock().map_err(|_| unavailable())?;
        let entry = entries
            .get(attempt)
            .filter(|e| e.view.conversation_id == conversation)
            .ok_or_else(|| {
                ServiceError::new(
                    "PROCESS_NOT_RUNNING",
                    "此会话没有对应的运行进程，请刷新状态",
                )
            })?;
        if entry.control.cancel() {
            Ok(())
        } else {
            Err(ServiceError::new(
                "PROCESS_FINISHED",
                "进程已结束，正在保存回执，未接受新的取消请求",
            ))
        }
    }
}
impl ProcessLease {
    pub fn control(&self) -> &ProcessCancellation {
        &self.control
    }
}
impl Drop for ProcessLease {
    fn drop(&mut self) {
        if let Ok(mut entries) = self.supervisor.entries.lock() {
            entries.remove(&self.attempt);
        }
    }
}
