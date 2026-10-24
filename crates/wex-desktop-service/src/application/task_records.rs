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
