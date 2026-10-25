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
            let task = s.tasks.iter().find(|task| task.id == task_id)
                .ok_or_else(|| ServiceError::new("TASK_NOT_FOUND", "任务不存在"))?;
            if task.status != TaskStatus::Draft {
                return Err(ServiceError::new("INVALID_TRANSITION", "任务不是草案，无法重复发布"));
            }
            if task.agent_ids.is_empty() || task.scope.is_empty() || task.acceptance.trim().is_empty() {
                return Err(ServiceError::new("INVALID_INPUT", "发布任务需要成员、工作范围与验收标准"));
            }
            if task.agent_ids.iter().any(|id| !task_member_assigned(s, id, &task.project_id)) {
                return Err(ServiceError::new("INVALID_BINDING", "任务成员不再拥有项目职责"));
            }
            let task = s.tasks.iter_mut().find(|task| task.id == task_id)
                .expect("task checked above");
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
        let task = state.tasks.iter().find(|task| task.id == task_id)
            .ok_or_else(|| ServiceError::new("TASK_NOT_FOUND", "任务不存在"))?;
        if task.status != TaskStatus::Published {
            return Err(ServiceError::new("TASK_NOT_PUBLISHED", "任务尚未发布"));
        }
        if !task.agent_ids.iter().any(|id| id == agent_id)
            || !task_member_assigned(&state, agent_id, &task.project_id)
        {
            return Err(ServiceError::new("AGENT_NOT_ASSIGNED", "成员未获该项目有效任务分配"));
        }
        let mut board = TaskBoard::default();
        board.insert(CoreTask {
            id: task.id.clone(),
            project_id: task.project_id.clone(),
            objective: task.goal.clone(),
            assigned_agent: Some(agent_id.to_owned()),
            assignment_epoch: revision,
            status: CoreStatus::Queued,
            evidence_refs: vec![],
        }).map_err(|_| ServiceError::new("INVALID_TASK", "任务无法映射到调度核心"))?;
        wex_core::dispatch::admit(&board, task_id, &task.project_id, agent_id, revision, true)
            .map_err(|_| ServiceError::new("DISPATCH_REFUSED", "任务未通过调度准入核查"))
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
                return Err(ServiceError::new("INVALID_TRANSITION", "已发布任务不能按草案直接覆盖，请走任务修订流程"));
            }
            if task.project_id != input.project_id
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
        service.mutate(|snapshot| {
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
            });
            Ok(())
        }).unwrap();
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
        assert_eq!(service.publish_task(state.revision, "task_1").unwrap_err().code, "INVALID_TRANSITION");
        assert_eq!(service.update_task(state.revision, "task_1", NewTask {
            project_id: "p_1".into(),
            title: "Overwritten".into(),
            goal: "Changed".into(),
            scope: vec!["src".into()],
            acceptance: "Done".into(),
            agent_ids: vec!["a_1".into()],
        }).unwrap_err().code, "INVALID_TRANSITION");
        assert_eq!(service.snapshot().unwrap().revision, state.revision);
    }

    #[test]
    fn publication_rejects_stale_revision_and_missing_scope_without_mutating_snapshot() {
        let mut service = draft_service();
        let revision = service.snapshot().unwrap().revision;
        assert_eq!(service.publish_task(revision - 1, "task_1").unwrap_err().code, "REVISION_CONFLICT");
        service.mutate(|state| {
            state.tasks[0].scope.clear();
            Ok(())
        }).unwrap();
        let revision = service.snapshot().unwrap().revision;
        assert_eq!(service.publish_task(revision, "task_1").unwrap_err().code, "INVALID_INPUT");
        let state = service.snapshot().unwrap();
        assert_eq!(state.revision, revision);
        assert_eq!(state.tasks[0].status, TaskStatus::Draft);
    }
}
