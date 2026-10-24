use super::{id, invalid, organization::check_revision, text};
use crate::*;

impl DesktopService {
    /// Store a manual review comment without implying that any verification ran.
    pub fn append_review_note(
        &mut self,
        revision: u64,
        input: NewReviewNote,
    ) -> Result<TaskRecord> {
        self.append_task_record(revision, NewTaskRecord {
            task_id: input.task_id,
            kind: TaskRecordKind::Handoff,
            body: input.body,
            references: input.references,
        })
    }

    /// Records a self-reported Agent update, not a trusted execution receipt.
    pub fn append_agent_work_report(
        &mut self,
        revision: u64,
        input: NewAgentWorkReport,
    ) -> Result<AgentWorkReport> {
        text(&input.body)?;
        if input.body.trim().is_empty() {
            return Err(invalid("报告内容不能为空"));
        }
        for reference in &input.references {
            text(reference)?;
        }
        self.mutate(|s| {
            check_revision(s, revision)?;
            let task = s.tasks.iter().find(|t| t.id == input.task_id)
                .ok_or_else(|| ServiceError::new("TASK_NOT_FOUND", "任务不存在"))?;
            if !task.agent_ids.contains(&input.agent_id) {
                return Err(ServiceError::new("INVALID_BINDING", "报告成员未分配到该任务"));
            }
            if matches!(task.status, TaskStatus::Completed | TaskStatus::Cancelled) {
                return Err(ServiceError::new("TASK_CLOSED", "已关闭任务不能新增施工报告"));
            }
            if !s.agents.iter().any(|agent| agent.id == input.agent_id && agent.project_id == task.project_id) {
                return Err(ServiceError::new("INVALID_BINDING", "报告成员与任务项目不一致"));
            }
            if s.agent_work_reports.iter().any(|r| r.task_id == input.task_id && r.agent_id == input.agent_id && r.body == input.body && r.references == input.references) {
                return Err(ServiceError::new("DUPLICATE_REPORT", "相同成员的重复报告未写入"));
            }
            let report = AgentWorkReport {
                id: id("agent_report"),
                task_id: task.id.clone(),
                agent_id: input.agent_id,
                body: input.body,
                references: input.references,
                task_snapshot: task.clone(),
                recorded_revision: s.revision.checked_add(1)
                    .ok_or_else(|| ServiceError::new("STORAGE_ERROR", "记录序号无法保存"))?,
            };
            s.agent_work_reports.push(report.clone());
            Ok(report)
        })
    }

    pub fn append_task_record(
        &mut self,
        revision: u64,
        input: NewTaskRecord,
    ) -> Result<TaskRecord> {
        text(&input.body)?;
        if input.body.trim().is_empty() {
            return Err(invalid("记录内容不能为空"));
        }
        for reference in &input.references {
            text(reference)?;
        }
        self.mutate(|s| {
            check_revision(s, revision)?;
            let task = s
                .tasks
                .iter()
                .find(|t| t.id == input.task_id)
                .ok_or_else(|| ServiceError::new("TASK_NOT_FOUND", "任务不存在"))?;
            if matches!(task.status, TaskStatus::Completed | TaskStatus::Cancelled)
                && input.kind == TaskRecordKind::Progress
            {
                return Err(ServiceError::new(
                    "TASK_CLOSED",
                    "已关闭的任务不能继续登记施工进度",
                ));
            }
            let record = TaskRecord {
                source: TaskRecordSource::User,
                id: id("note"),
                task_id: task.id.clone(),
                kind: input.kind,
                body: input.body,
                references: input.references,
                task_snapshot: task.clone(),
                recorded_revision: s.revision.checked_add(1).ok_or_else(|| {
                    ServiceError::new("STORAGE_ERROR", "记录序号无法保存，未写入数据")
                })?,
            };
            s.task_records.push(record.clone());
            Ok(record)
        })
    }
}
