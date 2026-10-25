use super::invalid;
use crate::*;

pub(super) fn build(s: &DesktopSnapshot, agent_id: &str) -> Result<MemberWorkBrief> {
    let member = s
        .agents
        .iter()
        .find(|a| a.id == agent_id)
        .ok_or_else(|| ServiceError::new("AGENT_NOT_FOUND", "成员不存在"))?;
    let project = s
        .projects
        .iter()
        .find(|p| p.id == member.project_id)
        .ok_or_else(|| invalid("成员项目不存在"))?;
    let lead = member.role == AgentRole::Lead;
    let tasks: Vec<_> = s
        .tasks
        .iter()
        .filter(|t| t.project_id == project.id && (lead || t.agent_ids.contains(&member.id)))
        .cloned()
        .collect();
    let task_records = s
        .task_records
        .iter()
        .filter(|r| tasks.iter().any(|t| t.id == r.task_id))
        .cloned()
        .collect();
    let members = s
        .agents
        .iter()
        .filter(|a| a.project_id == project.id && (lead || a.id == member.id))
        .cloned()
        .collect();
    Ok(MemberWorkBrief {
        revision: s.revision,
        scope: if lead {
            MemberBriefScope::Project
        } else {
            MemberBriefScope::AssignedTasks
        },
        member: member.clone(),
        project: project.clone(),
        members,
        tasks,
        task_records,
    })
}

impl DesktopService {
    pub fn member_brief(&self, agent_id: &str) -> Result<MemberWorkBrief> {
        build(&self.snapshot()?, agent_id)
    }
}
