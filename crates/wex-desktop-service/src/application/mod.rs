use crate::*;
mod execution_policy;
mod member_brief;
mod organization;
mod task_records;
mod tool_reply;
use crate::infrastructure::sqlite::DesktopStore;
use std::{collections::BTreeMap, path::Path};
use wex_core::connection_session::{
    AiSelection, Catalog, CatalogView, ConnectionKind, ConnectionRecord, ConnectionStatus,
    ConversationRecord,
};

pub struct DesktopService {
    store: DesktopStore,
    pending_results: BTreeMap<String, (Option<String>, Option<String>)>,
}

fn invalid(message: &str) -> ServiceError {
    ServiceError::new("INVALID_INPUT", message)
}
fn id(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::new_v4().simple())
}
fn title(value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
        return Err(invalid("名称不能为空、超长或包含控制字符"));
    }
    Ok(value.into())
}
fn text(value: &str) -> Result<()> {
    if value.len() > 1_048_576 || value.contains('\0') {
        return Err(invalid("内容过长或包含空字符"));
    }
    Ok(())
}
fn project_exists(s: &DesktopSnapshot, project_id: &str) -> Result<()> {
    if s.projects.iter().any(|p| p.id == project_id) {
        Ok(())
    } else {
        Err(ServiceError::new("PROJECT_NOT_FOUND", "项目不存在"))
    }
}
fn catalog(s: &DesktopSnapshot) -> Result<Catalog> {
    Catalog::from_view(CatalogView {
        connections: s
            .connections
            .iter()
            .map(|c| ConnectionRecord {
                id: c.id.clone(),
                name: c.name.clone(),
                kind: ConnectionKind::Ai,
                credential_ref: Some(c.credential_ref.clone()),
                status: if c.enabled {
                    ConnectionStatus::Enabled
                } else {
                    ConnectionStatus::Disabled
                },
            })
            .collect(),
        conversations: s
            .conversations
            .iter()
            .map(|c| ConversationRecord {
                id: c.id.clone(),
                title: c.title.clone(),
                selection: Some(AiSelection {
                    connection_id: c.connection_id.clone(),
                    model_id: c.model_id.clone(),
                }),
            })
            .collect(),
        default_selection: None,
    })
    .map_err(|_| ServiceError::new("INVALID_SNAPSHOT", "连接或会话目录无法恢复"))
}
fn selection(s: &DesktopSnapshot, connection_id: &str, model: &str) -> Result<()> {
    let c = s
        .connections
        .iter()
        .find(|c| c.id == connection_id)
        .ok_or_else(|| invalid("连接不存在"))?;
    if !c.enabled {
        return Err(ServiceError::new(
            "CONNECTION_UNAVAILABLE",
            "连接已停用，请在连接设置中检查",
        ));
    }
    if !c.models.iter().any(|m| m == model) {
        return Err(invalid("模型未配置在所选连接中"));
    }
    Ok(())
}

/// A task assignee must have an explicit duty in its project.
/// A user-level secretary duty alone is not a project/task assignment.
fn task_member_assigned(s: &DesktopSnapshot, agent_id: &str, project_id: &str) -> bool {
    s.agents.iter().any(|a| a.id == agent_id)
        && (s
            .agent_duties
            .iter()
            .any(|d| d.agent_id == agent_id && d.project_id.as_deref() == Some(project_id))
            || s.agents
                .iter()
                .any(|a| a.id == agent_id && a.project_id == project_id))
}

/// Validate persisted dispatch facts before permitting a Host to observe them.
fn validate_dispatch_snapshot(snapshot: &DesktopSnapshot) -> Result<()> {
    let mut identities = std::collections::BTreeSet::new();
    // One published task may be assigned to several agents. Deduplicate the
    // dispatch intent per publication + agent, not per publication alone.
    let mut publications = std::collections::BTreeSet::new();
    for attempt in &snapshot.dispatch_attempts {
        let task = snapshot
            .tasks
            .iter()
            .find(|task| task.id == attempt.task_id)
            .ok_or_else(|| ServiceError::new("INVALID_SNAPSHOT", "派工引用的任务不存在"))?;
        if attempt.id.is_empty()
            || attempt.recorded_revision == 0
            || attempt.recorded_revision > snapshot.revision
            || attempt.publication_epoch == 0
            || task.status != TaskStatus::Published
            || task.project_id != attempt.project_id
            || !snapshot.projects.iter().any(|p| p.id == attempt.project_id)
            || !snapshot
                .agents
                .iter()
                .any(|agent| agent.id == attempt.agent_id)
            || task.publication_epoch != attempt.publication_epoch
            || !task.agent_ids.contains(&attempt.agent_id)
            || !identities.insert(attempt.id.clone())
            || !publications.insert((
                attempt.task_id.clone(),
                attempt.publication_epoch,
                attempt.agent_id.clone(),
            ))
        {
            return Err(ServiceError::new(
                "INVALID_SNAPSHOT",
                "派工记录不一致，拒绝恢复",
            ));
        }
    }
    Ok(())
}

impl DesktopService {
    pub fn open(path: &Path) -> Result<Self> {
        Self::init(DesktopStore::open(path)?)
    }
    pub fn memory() -> Result<Self> {
        Self::init(DesktopStore::memory()?)
    }
    fn init(store: DesktopStore) -> Result<Self> {
        let mut service = Self {
            store,
            pending_results: BTreeMap::new(),
        };
        let snapshot = service.snapshot()?;
        catalog(&snapshot)?;
        validate_dispatch_snapshot(&snapshot)?;
        // An interrupted Host may have performed an external write.
        // Never turn InFlight into Pending and never launch it automatically.
        if snapshot
            .dispatch_attempts
            .iter()
            .any(|a| a.status == DispatchAttemptStatus::InFlight)
        {
            service.mutate(|s| {
                for attempt in &mut s.dispatch_attempts {
                    if attempt.status == DispatchAttemptStatus::InFlight {
                        attempt.status = DispatchAttemptStatus::OutcomeUnknown;
                    }
                }
                Ok(())
            })?;
        }
        if snapshot
            .turns
            .iter()
            .any(|t| t.status == TurnStatus::Running)
        {
            service.mutate(|s| {
                for turn in &mut s.turns {
                    if turn.status == TurnStatus::Running {
                        turn.status = TurnStatus::Interrupted;
                        turn.error = Some("应用中断，回复结果未知；未自动重新发送".into());
                    }
                }
                Ok(())
            })?;
        }
        Ok(service)
    }
    pub fn snapshot(&self) -> Result<DesktopSnapshot> {
        let mut snapshot = self.store.load()?;
        snapshot.unsaved_turn_ids = self.pending_results.keys().cloned().collect();
        Ok(snapshot)
    }
    fn mutate<T>(&mut self, change: impl FnOnce(&mut DesktopSnapshot) -> Result<T>) -> Result<T> {
        self.store.update(change)
    }
    pub fn create_project(
        &mut self,
        name: String,
        goal: String,
        resources: Vec<String>,
    ) -> Result<Project> {
        let name = title(&name)?;
        text(&goal)?;
        for r in &resources {
            text(r)?;
        }
        self.mutate(|s| {
            let p = Project {
                id: id("p"),
                name,
                goal,
                resources,
                context_id: None,
            };
            s.projects.push(p.clone());
            Ok(p)
        })
    }
    pub fn add_connection(
        &mut self,
        input: NewConnection,
        credential_ref: String,
    ) -> Result<AiConnection> {
        let name = title(&input.name)?;
        chat_endpoint(&input.base_url)?;
        if input.models.is_empty()
            || input.models.iter().any(|m| {
                m.is_empty() || m.len() > 256 || m.trim() != m || m.chars().any(char::is_control)
            })
        {
            return Err(invalid("请配置有效模型名称"));
        }
        self.mutate(|s| {
            let c = AiConnection {
                id: id("c"),
                name,
                base_url: input.base_url.trim_end_matches('/').into(),
                models: input.models,
                credential_ref,
                enabled: true,
            };
            let mut domain = catalog(s)?;
            domain
                .add_connection(&c.id, &c.name, ConnectionKind::Ai, Some(&c.credential_ref))
                .map_err(|_| invalid("连接信息无效"))?;
            s.connections.push(c.clone());
            Ok(c)
        })
    }
    pub fn set_connection_enabled(&mut self, connection_id: &str, enabled: bool) -> Result<()> {
        self.mutate(|s| {
            let mut domain = catalog(s)?;
            domain
                .set_enabled(connection_id, enabled)
                .map_err(|_| invalid("连接不存在或不可用"))?;
            s.connections
                .iter_mut()
                .find(|c| c.id == connection_id)
                .ok_or_else(|| invalid("连接不存在"))?
                .enabled = enabled;
            Ok(())
        })
    }
    pub fn add_agent(&mut self, input: NewAgent) -> Result<Agent> {
        let name = title(&input.name)?;
        self.mutate(|s| {
            project_exists(s, &input.project_id)?;
            selection(s, &input.connection_id, &input.model_id)?;
            let conversation = Conversation {
                context_id: None,
                id: id("conv"),
                project_id: Some(input.project_id.clone()),
                title: name.clone(),
                connection_id: input.connection_id.clone(),
                model_id: input.model_id.clone(),
            };
            s.conversations.push(conversation.clone());
            catalog(s)?;
            let agent = Agent {
                id: id("a"),
                project_id: input.project_id,
                name,
                role: input.role,
                connection_id: input.connection_id,
                model_id: input.model_id,
                conversation_id: conversation.id,
            };
            s.agent_duties.push(AgentDutyAssignment {
                id: id("duty"),
                agent_id: agent.id.clone(),
                project_id: Some(agent.project_id.clone()),
                duty_id: match agent.role {
                    AgentRole::Lead => "lead",
                    AgentRole::Worker => "worker",
                    AgentRole::Verifier => "verifier",
                    AgentRole::Reviewer => "reviewer",
                }
                .into(),
            });
            s.agents.push(agent.clone());
            Ok(agent)
        })
    }
    /// Assign a work responsibility without granting tool or data permissions.
    /// The legacy Agent.project_id and role remain for old UI compatibility.
    pub fn assign_agent_duty(
        &mut self,
        agent_id: &str,
        project_id: Option<String>,
        duty_id: String,
    ) -> Result<AgentDutyAssignment> {
        if !valid_duty_id(&duty_id) {
            return Err(invalid("职责标识无效"));
        }
        self.mutate(|s| {
            if !s.agents.iter().any(|a| a.id == agent_id) {
                return Err(ServiceError::new("AGENT_NOT_FOUND", "成员不存在"));
            }
            if let Some(project_id) = &project_id {
                project_exists(s, project_id)?;
            }
            if s.agent_duties.iter().any(|a| {
                a.agent_id == agent_id && a.project_id == project_id && a.duty_id == duty_id
            }) {
                return Err(ServiceError::new("DUPLICATE_ASSIGNMENT", "职责分配已存在"));
            }
            let assignment = AgentDutyAssignment {
                id: id("duty"),
                agent_id: agent_id.to_owned(),
                project_id,
                duty_id,
            };
            s.agent_duties.push(assignment.clone());
            Ok(assignment)
        })
    }

    pub fn create_task(&mut self, input: NewTask) -> Result<Task> {
        let name = title(&input.title)?;
        text(&input.goal)?;
        text(&input.acceptance)?;
        for scope in &input.scope {
            text(scope)?;
        }
        self.mutate(|s| {
            project_exists(s, &input.project_id)?;
            if input
                .agent_ids
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != input.agent_ids.len()
                || input
                    .agent_ids
                    .iter()
                    .any(|id| !task_member_assigned(s, id, &input.project_id))
            {
                return Err(ServiceError::new(
                    "INVALID_BINDING",
                    "任务成员必须拥有该项目的职责分配",
                ));
            }
            let task = Task {
                id: id("task"),
                project_id: input.project_id,
                title: name,
                goal: input.goal,
                scope: input.scope,
                acceptance: input.acceptance,
                agent_ids: input.agent_ids,
                status: TaskStatus::Draft,
                publication_epoch: 0,
            };
            s.tasks.push(task.clone());
            Ok(task)
        })
    }
    /// Persist a task publication decision in the same transaction as its revision.
    /// Publication does not start a tool or grant OS/connector access.
    pub fn publish_task(&mut self, revision: u64, task_id: &str) -> Result<Task> {
        self.mutate(|s| {
            organization::check_revision(s, revision)?;
            let task = s
                .tasks
                .iter()
                .find(|task| task.id == task_id)
                .ok_or_else(|| ServiceError::new("TASK_NOT_FOUND", "任务不存在"))?;
            if task.status != TaskStatus::Draft {
                return Err(ServiceError::new(
                    "INVALID_TRANSITION",
                    "任务不是草案，无法重复发布",
                ));
            }
            if task.agent_ids.is_empty()
                || task.scope.is_empty()
                || task
                    .scope
                    .iter()
                    .any(|scope| scope.trim().is_empty() || scope.chars().any(char::is_control))
                || task.goal.trim().is_empty()
                || task.acceptance.trim().is_empty()
            {
                return Err(ServiceError::new(
                    "INVALID_INPUT",
                    "发布任务需要目标、成员、工作范围与验收标准",
                ));
            }
            if task
                .agent_ids
                .iter()
                .any(|id| !task_member_assigned(s, id, &task.project_id))
            {
                return Err(ServiceError::new(
                    "INVALID_BINDING",
                    "任务成员不再拥有项目职责",
                ));
            }
            // Publication must never create a task the Core dispatcher will
            // subsequently reject for intrinsic task format constraints.
            let epoch = s
                .revision
                .checked_add(1)
                .ok_or_else(|| ServiceError::new("STORAGE_ERROR", "任务发布版本无法保存"))?;
            // Every assignee must be compatible with Core, not only the first:
            // subsequent dispatch admission can target any assigned member.
            for agent_id in &task.agent_ids {
                let candidate = wex_core::task::Task {
                    id: task.id.clone(),
                    project_id: task.project_id.clone(),
                    objective: task.goal.clone(),
                    assigned_agent: Some(agent_id.clone()),
                    assignment_epoch: epoch,
                    status: wex_core::task::TaskStatus::Queued,
                    evidence_refs: vec![],
                };
                let mut board = wex_core::task::TaskBoard::default();
                board.insert(candidate).map_err(|_| {
                    ServiceError::new("INVALID_TASK", "任务目标或成员标识不符合 Core 调度格式")
                })?;
            }
            let task = s
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .expect("task checked above");
            task.publication_epoch = epoch;
            task.status = TaskStatus::Published;
            Ok(task.clone())
        })
    }

    /// A read-only bridge to Core admission. This does not persist a dispatch
    /// attempt, reserve a lease or grant a tool permission.
    pub fn task_dispatch_candidate(
        &self,
        revision: u64,
        task_id: &str,
        agent_id: &str,
    ) -> Result<wex_core::dispatch::DispatchTicket> {
        use wex_core::task::{Task as CoreTask, TaskBoard, TaskStatus as CoreStatus};
        let state = self.snapshot()?;
        organization::check_revision(&state, revision)?;
        let task = state
            .tasks
            .iter()
            .find(|task| task.id == task_id)
            .ok_or_else(|| ServiceError::new("TASK_NOT_FOUND", "任务不存在"))?;
        if task.status != TaskStatus::Published {
            return Err(ServiceError::new("TASK_NOT_PUBLISHED", "任务尚未发布"));
        }
        if task.publication_epoch == 0 {
            return Err(ServiceError::new("INVALID_TASK", "任务缺少可信发布版本"));
        }
        if !task.agent_ids.iter().any(|id| id == agent_id)
            || !task_member_assigned(&state, agent_id, &task.project_id)
        {
            return Err(ServiceError::new(
                "AGENT_NOT_ASSIGNED",
                "成员未获该项目有效任务分配",
            ));
        }
        let mut board = TaskBoard::default();
        board
            .insert(CoreTask {
                id: task.id.clone(),
                project_id: task.project_id.clone(),
                objective: task.goal.clone(),
                assigned_agent: Some(agent_id.to_owned()),
                assignment_epoch: task.publication_epoch,
                status: CoreStatus::Queued,
                evidence_refs: vec![],
            })
            .map_err(|_| ServiceError::new("INVALID_TASK", "任务无法映射到调度核心"))?;
        wex_core::dispatch::admit(
            &board,
            task_id,
            &task.project_id,
            agent_id,
            task.publication_epoch,
            true,
        )
        .map_err(|_| ServiceError::new("DISPATCH_REFUSED", "任务未通过调度准入核查"))
    }

    /// Atomically record one intent for a published task and a selected agent.
    /// This is a queue record, NOT a tool launch or authorization grant.
    pub fn queue_dispatch_attempt(
        &mut self,
        revision: u64,
        task_id: &str,
        agent_id: &str,
    ) -> Result<DispatchAttempt> {
        // Read-only Core admission and duty checks before the store transaction.
        let ticket = self.task_dispatch_candidate(revision, task_id, agent_id)?;
        self.mutate(|s| {
            organization::check_revision(s, revision)?;
            let task = s
                .tasks
                .iter()
                .find(|t| t.id == ticket.task_id)
                .ok_or_else(|| ServiceError::new("TASK_NOT_FOUND", "任务不存在"))?;
            if task.status != TaskStatus::Published
                || task.publication_epoch != ticket.assignment_epoch
                || !task.agent_ids.contains(&ticket.agent_id)
                || !task_member_assigned(s, &ticket.agent_id, &task.project_id)
            {
                return Err(ServiceError::new(
                    "DISPATCH_REFUSED",
                    "任务或成员分配在领取前发生变化",
                ));
            }
            if s.dispatch_attempts.iter().any(|a| {
                a.task_id == ticket.task_id
                    && a.publication_epoch == ticket.assignment_epoch
                    && a.agent_id == ticket.agent_id
            }) {
                return Err(ServiceError::new(
                    "DUPLICATE_DISPATCH",
                    "该发布版本已为此成员登记派工尝试，不能重复登记",
                ));
            }
            let attempt = DispatchAttempt {
                id: id("attempt"),
                task_id: ticket.task_id,
                project_id: ticket.project_id,
                agent_id: ticket.agent_id,
                publication_epoch: ticket.assignment_epoch,
                status: DispatchAttemptStatus::Pending,
                recorded_revision: s
                    .revision
                    .checked_add(1)
                    .ok_or_else(|| ServiceError::new("STORAGE_ERROR", "派工版本无法保存"))?,
            };
            s.dispatch_attempts.push(attempt.clone());
            Ok(attempt)
        })
    }

    /// Claim a persisted dispatch intent exactly once. This records a safety
    /// boundary before any external I/O; it DOES NOT execute the task.
    /// Recovery converts this state into OutcomeUnknown, never Pending.
    pub fn claim_dispatch_attempt(
        &mut self,
        revision: u64,
        attempt_id: &str,
    ) -> Result<DispatchAttempt> {
        self.mutate(|s| {
            organization::check_revision(s, revision)?;
            let prior = s
                .dispatch_attempts
                .iter()
                .find(|a| a.id == attempt_id)
                .ok_or_else(|| ServiceError::new("ATTEMPT_NOT_FOUND", "派工记录不存在"))?;
            if prior.status != DispatchAttemptStatus::Pending {
                return Err(ServiceError::new(
                    "ATTEMPT_NOT_PENDING",
                    "派工记录已领取或需要人工核对",
                ));
            }
            let task = s
                .tasks
                .iter()
                .find(|t| t.id == prior.task_id)
                .ok_or_else(|| ServiceError::new("TASK_NOT_FOUND", "原任务不存在"))?;
            if task.status != TaskStatus::Published
                || task.project_id != prior.project_id
                || task.publication_epoch != prior.publication_epoch
                || !task.agent_ids.contains(&prior.agent_id)
                || !task_member_assigned(s, &prior.agent_id, &task.project_id)
            {
                return Err(ServiceError::new(
                    "DISPATCH_REFUSED",
                    "任务版本或成员授权关系已变化",
                ));
            }
            // Temporary fail-closed global fence until the Host can hold
            // canonical, path-overlap-aware leases for entire tool lifetimes.
            // Projects can share resources; serialize all claims until scoped leases exist.
            // A restored OutcomeUnknown in another project may still refer to
            // a shared filesystem or connector. Without durable scoped leases,
            // globally refuse another claim until the uncertainty is resolved.
            if s.dispatch_attempts.iter().any(|other| {
                other.id != attempt_id && other.status == DispatchAttemptStatus::OutcomeUnknown
            }) {
                return Err(ServiceError::new(
                    "UNKNOWN_DISPATCH_BLOCKED",
                    "存在结果未知的历史操作，未核查资源范围前禁止继续领取",
                ));
            }
            if s.dispatch_attempts.iter().any(|other| {
                other.id != attempt_id && other.status == DispatchAttemptStatus::InFlight
            }) {
                return Err(ServiceError::new(
                    "ACTIVE_DISPATCH_BLOCKED",
                    "存在执行中的派工，资源隔离接通前禁止并发领取",
                ));
            }
            let attempt = s
                .dispatch_attempts
                .iter_mut()
                .find(|a| a.id == attempt_id)
                .expect("attempt checked above");
            attempt.status = DispatchAttemptStatus::InFlight;
            Ok(attempt.clone())
        })
    }

    pub fn update_task(&mut self, revision: u64, task_id: &str, input: NewTask) -> Result<()> {
        let name = title(&input.title)?;
        text(&input.goal)?;
        text(&input.acceptance)?;
        for scope in &input.scope {
            text(scope)?;
        }
        self.mutate(|s| {
            organization::check_revision(s, revision)?;
            let task = s
                .tasks
                .iter()
                .find(|t| t.id == task_id)
                .ok_or_else(|| ServiceError::new("TASK_NOT_FOUND", "任务不存在"))?;
            if task.status != TaskStatus::Draft {
                return Err(ServiceError::new(
                    "INVALID_TRANSITION",
                    "已发布任务不能按草案直接覆盖，请走任务修订流程",
                ));
            }
            if task.project_id != input.project_id
                || input
                    .agent_ids
                    .iter()
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    != input.agent_ids.len()
                || input
                    .agent_ids
                    .iter()
                    .any(|id| !task_member_assigned(s, id, &task.project_id))
            {
                return Err(ServiceError::new(
                    "INVALID_BINDING",
                    "任务项目不可通过编辑变更，成员必须拥有该项目职责分配",
                ));
            }
            let task = s
                .tasks
                .iter_mut()
                .find(|t| t.id == task_id)
                .expect("task checked above");
            task.title = name;
            task.goal = input.goal;
            task.scope = input.scope;
            task.acceptance = input.acceptance;
            task.agent_ids = input.agent_ids;
            Ok(())
        })
    }
    pub fn create_conversation(
        &mut self,
        project_id: Option<String>,
        name: String,
        connection_id: String,
        model_id: String,
    ) -> Result<Conversation> {
        let name = title(&name)?;
        self.mutate(|s| {
            if let Some(project_id) = &project_id {
                project_exists(s, project_id)?;
            }
            selection(s, &connection_id, &model_id)?;
            let c = Conversation {
                context_id: None,
                id: id("conv"),
                project_id,
                title: name,
                connection_id,
                model_id,
            };
            s.conversations.push(c.clone());
            catalog(s)?;
            Ok(c)
        })
    }
    pub fn prepare_chat(&mut self, conversation_id: &str, prompt: String) -> Result<PreparedChat> {
        text(&prompt)?;
        if prompt.trim().is_empty() {
            return Err(invalid("消息不能为空"));
        }
        self.mutate(|s| {
            let selected=catalog(s)?.resolve_conversation(conversation_id).map_err(|_|ServiceError::new("CONNECTION_UNAVAILABLE", "会话绑定的连接或模型不可用，未切换到其他连接"))?;
            selection(s,&selected.connection_id,&selected.model_id)?;
            if s.turns.iter().any(|t|t.conversation_id==conversation_id&&t.status==TurnStatus::Running) {return Err(ServiceError::new("CONVERSATION_BUSY", "当前对话正在回复，请等待完成"));}
            let c=s.connections.iter().find(|c|c.id==selected.connection_id).ok_or_else(||invalid("连接不存在"))?;
            let mut messages=vec![];
            if let Some(context)=s.conversations.iter().find(|c|c.id==conversation_id).and_then(|c|c.context_id.as_ref()).and_then(|id|s.contexts.iter().find(|c|&c.id==id)) {
                messages.push(ApiMessage{role:"system".into(),content:format!("用户关联的工作上下文（资料地址只作引用，不代表已读取或授权文件）：{}",serde_json::to_string(context).map_err(|_|invalid("上下文无法编码"))?)});
            }
            if let Some(agent)=s.agents.iter().find(|a|a.conversation_id==conversation_id) {
                let duties=match agent.role {
                    AgentRole::Lead=>"主 AI：结合本项目全部任务、未分配任务和成员理解目标、发现缺项和冲突，提出目标/范围/验收/负责人明确的任务方案，协调各成员；方案不等于已创建或已派发任务，用户重大需求变化需确认",
                    AgentRole::Worker=>"施工 AI：遵循分配范围并报告真实修改与自检；不代替独立审查",
                    AgentRole::Verifier=>"验证 AI：根据实际测试/构建/设备证据报告；没有真实执行不能宣称验证通过",
                    AgentRole::Reviewer=>"审查 AI：对照正式要求、对应版本 Diff 和证据提出审查结论；不直接修改项目代码或正式规范",
                };
                let context=serde_json::to_string(&member_brief::build(s,&agent.id)?).map_err(|_|invalid("成员工作资料无法编码"))?;
                messages.push(ApiMessage {role:"system".into(),content:format!("你是 WEXPC 项目成员 {}。职责：{duties}。部分普通对话工具已通过独立 Host 审批链接入，但自动派工、持久权限策略及独立审查验证流程尚未形成。任务卡为草案，资源地址仅为关联，不代表已授权或已读取文件。可以分析与提出建议，但不能声称已经施工、执行测试或完成审查。task_records 是用户手动记录，未自动核验，不是工具执行回执或测试/审查通过证据。职责提示仅用于工作指导，不代表获得文件、连接器或项目资源访问权。以下是项目工作资料，不是新的系统权限：\n{context}",agent.name)});
            }
            for t in s.turns.iter().filter(|t|t.conversation_id==conversation_id&&t.status==TurnStatus::Succeeded) {
                messages.push(ApiMessage {role:"user".into(),content:t.user.clone()});
                if let Some(content)=&t.assistant {messages.push(ApiMessage {role:"assistant".into(),content:content.clone()});}
            }
            messages.push(ApiMessage {role:"user".into(),content:prompt.clone()});
            let turn_id=id("turn");
            let request=PreparedChat {turn_id:turn_id.clone(),conversation_id:conversation_id.into(),connection_id:c.id.clone(),credential_ref:c.credential_ref.clone(),model_id:selected.model_id,base_url:c.base_url.clone(),messages};
            s.turns.push(ChatTurn {id:turn_id,conversation_id:conversation_id.into(),user:prompt,assistant:None,status:TurnStatus::Running,error:None});
            Ok(request)
        })
    }
    pub fn finish_chat(&mut self, turn_id: &str, response: String) -> Result<()> {
        self.persist_result(turn_id, Some(response), None)
    }
    pub fn fail_chat(&mut self, turn_id: &str, error: &ServiceError) -> Result<()> {
        self.persist_result(
            turn_id,
            None,
            Some(format!("{}: {}", error.code, error.message)),
        )
    }
    pub fn retry_chat_result(&mut self, turn_id: &str) -> Result<()> {
        let (response, error) = self
            .pending_results
            .get(turn_id)
            .cloned()
            .ok_or_else(|| ServiceError::new("NO_PENDING_RESULT", "没有可重新保存的结果"))?;
        self.persist_result(turn_id, response, error)
    }
    fn persist_result(
        &mut self,
        turn_id: &str,
        response: Option<String>,
        error: Option<String>,
    ) -> Result<()> {
        self.pending_results
            .insert(turn_id.into(), (response.clone(), error.clone()));
        let result = self.settle(turn_id, response, error);
        if !matches!(&result,Err(e) if e.code=="STORAGE_ERROR") {
            self.pending_results.remove(turn_id);
        }
        result.map_err(|e| {
            if e.code == "STORAGE_ERROR" {
                ServiceError::new(
                    "STORAGE_ERROR",
                    "API 请求已结束，但结果保存失败。可重试保存，不会重新调用 API；请保留应用打开",
                )
            } else {
                e
            }
        })
    }
    fn settle(
        &mut self,
        turn_id: &str,
        response: Option<String>,
        error: Option<String>,
    ) -> Result<()> {
        self.mutate(|s| {
            let turn = s
                .turns
                .iter_mut()
                .find(|t| t.id == turn_id)
                .ok_or_else(|| invalid("请求不存在"))?;
            if turn.status != TurnStatus::Running {
                return Err(ServiceError::new(
                    "INVALID_STATE",
                    "请求已经结束，未覆盖先前结果",
                ));
            }
            turn.status = if response.is_some() {
                TurnStatus::Succeeded
            } else {
                TurnStatus::Failed
            };
            turn.assistant = response;
            turn.error = error;
            Ok(())
        })
    }
}
pub(crate) mod process_control;

#[cfg(test)]
mod publication_tests {
    use super::*;

    fn draft_service() -> DesktopService {
        let mut service = DesktopService::memory().unwrap();
        service
            .mutate(|snapshot| {
                snapshot.projects.push(Project {
                    id: "p_1".into(),
                    name: "Project".into(),
                    goal: String::new(),
                    resources: vec![],
                    context_id: None,
                });
                snapshot.agents.push(Agent {
                    id: "a_1".into(),
                    project_id: "p_1".into(),
                    name: "Worker".into(),
                    role: AgentRole::Worker,
                    connection_id: "c_1".into(),
                    model_id: "model".into(),
                    conversation_id: "conv_1".into(),
                });
                snapshot.tasks.push(Task {
                    id: "task_1".into(),
                    project_id: "p_1".into(),
                    title: "Implement".into(),
                    goal: "Make change".into(),
                    scope: vec!["src".into()],
                    acceptance: "Tests pass".into(),
                    agent_ids: vec!["a_1".into()],
                    status: TaskStatus::Draft,
                    publication_epoch: 0,
                });
                Ok(())
            })
            .unwrap();
        service
    }

    #[test]
    fn publication_persists_and_cannot_be_repeated_or_edited_as_draft() {
        let mut service = draft_service();
        let revision = service.snapshot().unwrap().revision;
        let published = service.publish_task(revision, "task_1").unwrap();
        assert_eq!(published.status, TaskStatus::Published);
        let state = service.snapshot().unwrap();
        assert_eq!(state.tasks[0].status, TaskStatus::Published);
        assert_eq!(
            service
                .publish_task(state.revision, "task_1")
                .unwrap_err()
                .code,
            "INVALID_TRANSITION"
        );
        assert_eq!(
            service
                .update_task(
                    state.revision,
                    "task_1",
                    NewTask {
                        project_id: "p_1".into(),
                        title: "Overwritten".into(),
                        goal: "Changed".into(),
                        scope: vec!["src".into()],
                        acceptance: "Done".into(),
                        agent_ids: vec!["a_1".into()],
                    }
                )
                .unwrap_err()
                .code,
            "INVALID_TRANSITION"
        );
        assert_eq!(service.snapshot().unwrap().revision, state.revision);
    }

    #[test]
    fn published_task_reaches_core_admission_only_with_current_revision_and_member() {
        let mut service = draft_service();
        let before = service.snapshot().unwrap().revision;
        assert_eq!(
            service
                .task_dispatch_candidate(before, "task_1", "a_1")
                .unwrap_err()
                .code,
            "TASK_NOT_PUBLISHED"
        );
        service.publish_task(before, "task_1").unwrap();
        let current = service.snapshot().unwrap().revision;
        let ticket = service
            .task_dispatch_candidate(current, "task_1", "a_1")
            .unwrap();
        assert_eq!(ticket.task_id, "task_1");
        assert_eq!(ticket.agent_id, "a_1");
        assert_eq!(ticket.assignment_epoch, current);
        assert_eq!(
            service
                .task_dispatch_candidate(before, "task_1", "a_1")
                .unwrap_err()
                .code,
            "REVISION_CONFLICT"
        );
        assert_eq!(
            service
                .task_dispatch_candidate(current, "task_1", "a_2")
                .unwrap_err()
                .code,
            "AGENT_NOT_ASSIGNED"
        );
    }

    #[test]
    fn dispatch_queue_is_durable_and_duplicate_registration_is_refused() {
        let mut service = draft_service();
        let initial = service.snapshot().unwrap().revision;
        service.publish_task(initial, "task_1").unwrap();
        let revision = service.snapshot().unwrap().revision;
        let attempt = service
            .queue_dispatch_attempt(revision, "task_1", "a_1")
            .unwrap();
        assert_eq!(attempt.status, DispatchAttemptStatus::Pending);
        let saved = service.snapshot().unwrap();
        assert_eq!(saved.dispatch_attempts.len(), 1);
        assert_eq!(saved.dispatch_attempts[0].id, attempt.id);
        assert_eq!(
            saved.dispatch_attempts[0].publication_epoch,
            saved.tasks[0].publication_epoch
        );
        assert_eq!(
            service
                .queue_dispatch_attempt(saved.revision, "task_1", "a_1")
                .unwrap_err()
                .code,
            "DUPLICATE_DISPATCH"
        );
        assert_eq!(service.snapshot().unwrap().revision, saved.revision);
    }

    #[test]
    fn one_published_task_can_queue_distinct_assignees_but_not_duplicate_one() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("desktop.sqlite");
        let mut service = DesktopService::open(&path).unwrap();
        service
            .mutate(|snapshot| {
                snapshot.projects.push(Project {
                    id: "p_1".into(),
                    name: "Project".into(),
                    goal: String::new(),
                    resources: vec![],
                    context_id: None,
                });
                snapshot.agents.push(Agent {
                    id: "a_1".into(),
                    project_id: "p_1".into(),
                    name: "Worker".into(),
                    role: AgentRole::Worker,
                    connection_id: "c_1".into(),
                    model_id: "model".into(),
                    conversation_id: "conv_1".into(),
                });
                snapshot.tasks.push(Task {
                    id: "task_1".into(),
                    project_id: "p_1".into(),
                    title: "Implement".into(),
                    goal: "Make change".into(),
                    scope: vec!["src".into()],
                    acceptance: "Tests pass".into(),
                    agent_ids: vec!["a_1".into()],
                    status: TaskStatus::Draft,
                    publication_epoch: 0,
                });
                Ok(())
            })
            .unwrap();
        service
            .mutate(|snapshot| {
                snapshot.agents.push(Agent {
                    id: "a_2".into(),
                    project_id: "p_1".into(),
                    name: "Reviewer".into(),
                    role: AgentRole::Reviewer,
                    connection_id: "c_1".into(),
                    model_id: "model".into(),
                    conversation_id: "conv_2".into(),
                });
                snapshot.tasks[0].agent_ids.push("a_2".into());
                Ok(())
            })
            .unwrap();
        let revision = service.snapshot().unwrap().revision;
        service.publish_task(revision, "task_1").unwrap();
        let revision = service.snapshot().unwrap().revision;
        let first = service
            .queue_dispatch_attempt(revision, "task_1", "a_1")
            .unwrap();
        let revision = service.snapshot().unwrap().revision;
        let second = service
            .queue_dispatch_attempt(revision, "task_1", "a_2")
            .unwrap();
        assert_ne!(first.id, second.id);
        assert_eq!(first.publication_epoch, second.publication_epoch);
        let saved = service.snapshot().unwrap();
        assert_eq!(saved.dispatch_attempts.len(), 2);
        assert_eq!(
            service
                .queue_dispatch_attempt(saved.revision, "task_1", "a_1")
                .unwrap_err()
                .code,
            "DUPLICATE_DISPATCH"
        );
        assert_eq!(service.snapshot().unwrap().dispatch_attempts.len(), 2);
        drop(service);

        // Startup revalidates the persisted queue; both distinct assignees
        // must survive recovery without weakening duplicate protection.
        let restored = DesktopService::open(&path).unwrap();
        let restored_state = restored.snapshot().unwrap();
        assert_eq!(restored_state.dispatch_attempts.len(), 2);
        validate_dispatch_snapshot(&restored_state).unwrap();
        assert_eq!(
            restored_state
                .dispatch_attempts
                .iter()
                .map(|attempt| attempt.agent_id.as_str())
                .collect::<std::collections::BTreeSet<_>>(),
            ["a_1", "a_2"].into_iter().collect()
        );
    }

    #[test]
    fn claim_is_single_use_and_recovery_blocks_automatic_replay() {
        let mut service = draft_service();
        let revision = service.snapshot().unwrap().revision;
        service.publish_task(revision, "task_1").unwrap();
        let revision = service.snapshot().unwrap().revision;
        let queued = service
            .queue_dispatch_attempt(revision, "task_1", "a_1")
            .unwrap();
        let revision = service.snapshot().unwrap().revision;
        let claimed = service
            .claim_dispatch_attempt(revision, &queued.id)
            .unwrap();
        assert_eq!(claimed.status, DispatchAttemptStatus::InFlight);
        let saved = service.snapshot().unwrap();
        assert_eq!(
            service
                .claim_dispatch_attempt(saved.revision, &queued.id)
                .unwrap_err()
                .code,
            "ATTEMPT_NOT_PENDING"
        );
        assert_eq!(service.snapshot().unwrap().revision, saved.revision);
        let original = saved.dispatch_attempts[0].clone();
        service
            .mutate(|s| {
                assert_eq!(s.dispatch_attempts[0].id, original.id);
                s.dispatch_attempts[0].status = DispatchAttemptStatus::OutcomeUnknown;
                Ok(())
            })
            .unwrap();
        assert_eq!(
            service
                .claim_dispatch_attempt(service.snapshot().unwrap().revision, &queued.id)
                .unwrap_err()
                .code,
            "ATTEMPT_NOT_PENDING"
        );
    }

    #[test]
    fn claiming_second_task_in_same_project_is_blocked_by_inflight_or_unknown() {
        let mut service = draft_service();
        let mut second = service.snapshot().unwrap().tasks[0].clone();
        second.id = "task_2".into();
        service
            .mutate(|snapshot| {
                snapshot.tasks.push(second);
                Ok(())
            })
            .unwrap();
        let rev = service.snapshot().unwrap().revision;
        service.publish_task(rev, "task_1").unwrap();
        let rev = service.snapshot().unwrap().revision;
        service.publish_task(rev, "task_2").unwrap();
        let rev = service.snapshot().unwrap().revision;
        let first = service
            .queue_dispatch_attempt(rev, "task_1", "a_1")
            .unwrap();
        let rev = service.snapshot().unwrap().revision;
        let second = service
            .queue_dispatch_attempt(rev, "task_2", "a_1")
            .unwrap();
        let rev = service.snapshot().unwrap().revision;
        service.claim_dispatch_attempt(rev, &first.id).unwrap();
        let rev = service.snapshot().unwrap().revision;
        assert_eq!(
            service
                .claim_dispatch_attempt(rev, &second.id)
                .unwrap_err()
                .code,
            "ACTIVE_DISPATCH_BLOCKED"
        );
        assert_eq!(service.snapshot().unwrap().revision, rev);
        service
            .mutate(|snapshot| {
                snapshot
                    .dispatch_attempts
                    .iter_mut()
                    .find(|a| a.id == first.id)
                    .unwrap()
                    .status = DispatchAttemptStatus::OutcomeUnknown;
                Ok(())
            })
            .unwrap();
        let rev = service.snapshot().unwrap().revision;
        assert_eq!(
            service
                .claim_dispatch_attempt(rev, &second.id)
                .unwrap_err()
                .code,
            "UNKNOWN_DISPATCH_BLOCKED"
        );
        assert_eq!(
            service
                .snapshot()
                .unwrap()
                .dispatch_attempts
                .iter()
                .find(|a| a.id == second.id)
                .unwrap()
                .status,
            DispatchAttemptStatus::Pending
        );
    }

    #[test]
    fn historical_dispatch_survives_member_duty_changes_at_restart() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("desktop.sqlite");
        let attempt_id = {
            let mut service = DesktopService::open(&db).unwrap();
            service
                .mutate(|snapshot| {
                    snapshot.projects.push(Project {
                        id: "p_1".into(),
                        name: "Project".into(),
                        goal: String::new(),
                        resources: vec![],
                        context_id: None,
                    });
                    snapshot.agents.push(Agent {
                        id: "a_1".into(),
                        project_id: "p_1".into(),
                        name: "Worker".into(),
                        role: AgentRole::Worker,
                        connection_id: "c_1".into(),
                        model_id: "model".into(),
                        conversation_id: "conv_1".into(),
                    });
                    snapshot.tasks.push(Task {
                        id: "task_1".into(),
                        project_id: "p_1".into(),
                        title: "Implement".into(),
                        goal: "Make change".into(),
                        scope: vec!["src".into()],
                        acceptance: "Tests pass".into(),
                        agent_ids: vec!["a_1".into()],
                        status: TaskStatus::Draft,
                        publication_epoch: 0,
                    });
                    Ok(())
                })
                .unwrap();
            let revision = service.snapshot().unwrap().revision;
            service.publish_task(revision, "task_1").unwrap();
            let revision = service.snapshot().unwrap().revision;
            let attempt = service
                .queue_dispatch_attempt(revision, "task_1", "a_1")
                .unwrap();
            service
                .mutate(|snapshot| {
                    snapshot.agents[0].project_id = "another_project".into();
                    Ok(())
                })
                .unwrap();
            attempt.id
        };

        // A real SQLite reopen must preserve the historical record while
        // refusing to treat the old membership as current authority.
        let mut restored = DesktopService::open(&db).unwrap();
        let persisted = restored.snapshot().unwrap();
        assert!(validate_dispatch_snapshot(&persisted).is_ok());
        assert_eq!(
            restored
                .claim_dispatch_attempt(persisted.revision, &attempt_id)
                .unwrap_err()
                .code,
            "DISPATCH_REFUSED"
        );
    }

    #[test]
    fn disk_restart_preserves_pending_attempt_without_claiming_it() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("desktop.sqlite");
        let attempt_id = {
            let mut service = DesktopService::open(&db).unwrap();
            service
                .mutate(|snapshot| {
                    snapshot.projects.push(Project {
                        id: "p_1".into(),
                        name: "Project".into(),
                        goal: String::new(),
                        resources: vec![],
                        context_id: None,
                    });
                    snapshot.agents.push(Agent {
                        id: "a_1".into(),
                        project_id: "p_1".into(),
                        name: "Worker".into(),
                        role: AgentRole::Worker,
                        connection_id: "c_1".into(),
                        model_id: "model".into(),
                        conversation_id: "conv_1".into(),
                    });
                    snapshot.tasks.push(Task {
                        id: "task_1".into(),
                        project_id: "p_1".into(),
                        title: "Implement".into(),
                        goal: "Change file".into(),
                        scope: vec!["src".into()],
                        acceptance: "Review".into(),
                        agent_ids: vec!["a_1".into()],
                        status: TaskStatus::Draft,
                        publication_epoch: 0,
                    });
                    Ok(())
                })
                .unwrap();
            let revision = service.snapshot().unwrap().revision;
            service.publish_task(revision, "task_1").unwrap();
            let revision = service.snapshot().unwrap().revision;
            service
                .queue_dispatch_attempt(revision, "task_1", "a_1")
                .unwrap()
                .id
        };
        let recovered = DesktopService::open(&db).unwrap();
        let snapshot = recovered.snapshot().unwrap();
        assert_eq!(snapshot.dispatch_attempts.len(), 1);
        assert_eq!(snapshot.dispatch_attempts[0].id, attempt_id);
        assert_eq!(
            snapshot.dispatch_attempts[0].status,
            DispatchAttemptStatus::Pending
        );
    }

    #[test]
    fn disk_restart_turns_claimed_attempt_unknown_without_replaying() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("desktop.sqlite");
        let attempt_id = {
            let mut service = DesktopService::open(&db).unwrap();
            service
                .mutate(|snapshot| {
                    snapshot.projects.push(Project {
                        id: "p_1".into(),
                        name: "Project".into(),
                        goal: String::new(),
                        resources: vec![],
                        context_id: None,
                    });
                    snapshot.agents.push(Agent {
                        id: "a_1".into(),
                        project_id: "p_1".into(),
                        name: "Worker".into(),
                        role: AgentRole::Worker,
                        connection_id: "c_1".into(),
                        model_id: "model".into(),
                        conversation_id: "conv_1".into(),
                    });
                    snapshot.tasks.push(Task {
                        id: "task_1".into(),
                        project_id: "p_1".into(),
                        title: "Implement".into(),
                        goal: "Change file".into(),
                        scope: vec!["src".into()],
                        acceptance: "Review".into(),
                        agent_ids: vec!["a_1".into()],
                        status: TaskStatus::Draft,
                        publication_epoch: 0,
                    });
                    Ok(())
                })
                .unwrap();
            let revision = service.snapshot().unwrap().revision;
            service.publish_task(revision, "task_1").unwrap();
            let revision = service.snapshot().unwrap().revision;
            let attempt = service
                .queue_dispatch_attempt(revision, "task_1", "a_1")
                .unwrap();
            let revision = service.snapshot().unwrap().revision;
            service
                .claim_dispatch_attempt(revision, &attempt.id)
                .unwrap();
            attempt.id
        };
        let mut recovered = DesktopService::open(&db).unwrap();
        let restored = recovered.snapshot().unwrap();
        assert_eq!(restored.dispatch_attempts.len(), 1);
        assert_eq!(restored.dispatch_attempts[0].id, attempt_id);
        assert_eq!(
            restored.dispatch_attempts[0].status,
            DispatchAttemptStatus::OutcomeUnknown
        );
        assert_eq!(
            recovered
                .claim_dispatch_attempt(restored.revision, &attempt_id)
                .unwrap_err()
                .code,
            "ATTEMPT_NOT_PENDING"
        );
        assert_eq!(recovered.snapshot().unwrap().dispatch_attempts.len(), 1);
    }

    #[test]
    fn corrupted_dispatch_snapshot_is_rejected_before_recovery() {
        let mut service = draft_service();
        let revision = service.snapshot().unwrap().revision;
        service.publish_task(revision, "task_1").unwrap();
        let revision = service.snapshot().unwrap().revision;
        let attempt = service
            .queue_dispatch_attempt(revision, "task_1", "a_1")
            .unwrap();
        let valid = service.snapshot().unwrap();
        assert!(validate_dispatch_snapshot(&valid).is_ok());

        let mut duplicate = valid.clone();
        duplicate.dispatch_attempts.push(attempt.clone());
        assert_eq!(
            validate_dispatch_snapshot(&duplicate).unwrap_err().code,
            "INVALID_SNAPSHOT"
        );

        let mut orphan = valid.clone();
        orphan.dispatch_attempts[0].task_id = "missing".into();
        assert_eq!(
            validate_dispatch_snapshot(&orphan).unwrap_err().code,
            "INVALID_SNAPSHOT"
        );

        let mut forged_epoch = valid.clone();
        forged_epoch.dispatch_attempts[0].publication_epoch += 1;
        assert_eq!(
            validate_dispatch_snapshot(&forged_epoch).unwrap_err().code,
            "INVALID_SNAPSHOT"
        );

        let mut forged_agent = valid.clone();
        forged_agent.dispatch_attempts[0].agent_id = "other_agent".into();
        assert_eq!(
            validate_dispatch_snapshot(&forged_agent).unwrap_err().code,
            "INVALID_SNAPSHOT"
        );

        let mut future_revision = valid;
        future_revision.dispatch_attempts[0].recorded_revision = future_revision.revision + 1;
        assert_eq!(
            validate_dispatch_snapshot(&future_revision)
                .unwrap_err()
                .code,
            "INVALID_SNAPSHOT"
        );
    }

    #[test]
    fn unknown_attempt_blocks_claims_even_in_another_project() {
        let mut service = draft_service();
        let mut other = service.snapshot().unwrap().tasks[0].clone();
        other.id = "task_other".into();
        other.project_id = "p_other".into();
        service
            .mutate(|snapshot| {
                snapshot.projects.push(Project {
                    id: "p_other".into(),
                    name: "Other".into(),
                    goal: String::new(),
                    resources: vec![],
                    context_id: None,
                });
                snapshot.agents.push(Agent {
                    id: "a_other".into(),
                    project_id: "p_other".into(),
                    name: "Other worker".into(),
                    role: AgentRole::Worker,
                    connection_id: "c_1".into(),
                    model_id: "model".into(),
                    conversation_id: "conv_other".into(),
                });
                other.agent_ids = vec!["a_other".into()];
                snapshot.tasks.push(other);
                Ok(())
            })
            .unwrap();
        let rev = service.snapshot().unwrap().revision;
        service.publish_task(rev, "task_1").unwrap();
        let rev = service.snapshot().unwrap().revision;
        service.publish_task(rev, "task_other").unwrap();
        let rev = service.snapshot().unwrap().revision;
        let first = service
            .queue_dispatch_attempt(rev, "task_1", "a_1")
            .unwrap();
        let rev = service.snapshot().unwrap().revision;
        let second = service
            .queue_dispatch_attempt(rev, "task_other", "a_other")
            .unwrap();
        let rev = service.snapshot().unwrap().revision;
        service.claim_dispatch_attempt(rev, &first.id).unwrap();
        let active_rev = service.snapshot().unwrap().revision;
        assert_eq!(
            service
                .claim_dispatch_attempt(active_rev, &second.id)
                .unwrap_err()
                .code,
            "ACTIVE_DISPATCH_BLOCKED"
        );
        assert_eq!(service.snapshot().unwrap().revision, active_rev);
        service
            .mutate(|snapshot| {
                snapshot
                    .dispatch_attempts
                    .iter_mut()
                    .find(|a| a.id == first.id)
                    .unwrap()
                    .status = DispatchAttemptStatus::OutcomeUnknown;
                Ok(())
            })
            .unwrap();
        let rev = service.snapshot().unwrap().revision;
        assert_eq!(
            service
                .claim_dispatch_attempt(rev, &second.id)
                .unwrap_err()
                .code,
            "UNKNOWN_DISPATCH_BLOCKED"
        );
    }

    #[test]
    fn dispatch_queue_rejects_stale_revision_without_creating_attempt() {
        let mut service = draft_service();
        let initial = service.snapshot().unwrap().revision;
        service.publish_task(initial, "task_1").unwrap();
        assert_eq!(
            service
                .queue_dispatch_attempt(initial, "task_1", "a_1")
                .unwrap_err()
                .code,
            "REVISION_CONFLICT"
        );
        assert!(service.snapshot().unwrap().dispatch_attempts.is_empty());
    }

    #[test]
    fn unrelated_snapshot_revision_does_not_change_published_assignment_epoch() {
        let mut service = draft_service();
        let initial = service.snapshot().unwrap().revision;
        let published = service.publish_task(initial, "task_1").unwrap();
        assert_eq!(published.publication_epoch, initial + 1);
        service
            .mutate(|snapshot| {
                snapshot.projects[0].goal = "Unrelated update".into();
                Ok(())
            })
            .unwrap();
        let after = service.snapshot().unwrap();
        assert!(after.revision > published.publication_epoch);
        assert_eq!(
            after.tasks[0].publication_epoch,
            published.publication_epoch
        );
        let ticket = service
            .task_dispatch_candidate(after.revision, "task_1", "a_1")
            .unwrap();
        assert_eq!(ticket.assignment_epoch, published.publication_epoch);
    }

    #[test]
    fn publication_rejects_blank_and_control_character_scopes() {
        for invalid_scope in [" ", "src\nother"] {
            let mut service = draft_service();
            service
                .mutate(|snapshot| {
                    snapshot.tasks[0].scope = vec![invalid_scope.into()];
                    Ok(())
                })
                .unwrap();
            let revision = service.snapshot().unwrap().revision;
            assert_eq!(
                service.publish_task(revision, "task_1").unwrap_err().code,
                "INVALID_INPUT"
            );
            assert_eq!(service.snapshot().unwrap().revision, revision);
            assert_eq!(
                service.snapshot().unwrap().tasks[0].status,
                TaskStatus::Draft
            );
        }
    }

    #[test]
    fn publication_checks_all_assignees_not_only_first() {
        let mut service = draft_service();
        service
            .mutate(|snapshot| {
                let other = "a".repeat(65);
                snapshot.agents.push(Agent {
                    id: other.clone(),
                    project_id: "p_1".into(),
                    name: "Invalid Core agent".into(),
                    role: AgentRole::Worker,
                    connection_id: "c_1".into(),
                    model_id: "model".into(),
                    conversation_id: "conv_other".into(),
                });
                snapshot.tasks[0].agent_ids.push(other);
                Ok(())
            })
            .unwrap();
        let before = service.snapshot().unwrap();
        assert_eq!(
            service
                .publish_task(before.revision, "task_1")
                .unwrap_err()
                .code,
            "INVALID_TASK"
        );
        let after = service.snapshot().unwrap();
        assert_eq!(after.revision, before.revision);
        assert_eq!(after.tasks[0].status, TaskStatus::Draft);
        assert_eq!(after.tasks[0].publication_epoch, 0);
    }

    #[test]
    fn publish_rejects_objective_exceeding_core_limit_without_durable_changes() {
        let mut service = draft_service();
        service
            .mutate(|snapshot| {
                snapshot.tasks[0].goal = "x".repeat(8193);
                Ok(())
            })
            .unwrap();
        let before = service.snapshot().unwrap();
        assert_eq!(
            service
                .publish_task(before.revision, "task_1")
                .unwrap_err()
                .code,
            "INVALID_TASK"
        );
        let after = service.snapshot().unwrap();
        assert_eq!(after.revision, before.revision);
        assert_eq!(after.tasks[0].status, TaskStatus::Draft);
        assert_eq!(after.tasks[0].publication_epoch, 0);
    }

    #[test]
    fn publication_rejects_stale_revision_and_missing_scope_without_mutating_snapshot() {
        let mut service = draft_service();
        let revision = service.snapshot().unwrap().revision;
        assert_eq!(
            service
                .publish_task(revision - 1, "task_1")
                .unwrap_err()
                .code,
            "REVISION_CONFLICT"
        );
        service
            .mutate(|state| {
                state.tasks[0].scope.clear();
                Ok(())
            })
            .unwrap();
        let revision = service.snapshot().unwrap().revision;
        assert_eq!(
            service.publish_task(revision, "task_1").unwrap_err().code,
            "INVALID_INPUT"
        );
        let state = service.snapshot().unwrap();
        assert_eq!(state.revision, revision);
        assert_eq!(state.tasks[0].status, TaskStatus::Draft);
    }
}
