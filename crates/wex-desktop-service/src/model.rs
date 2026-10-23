use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct ServiceError {
    pub code: &'static str,
    pub message: String,
}

impl ServiceError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for ServiceError {}
pub type Result<T> = std::result::Result<T, ServiceError>;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewConnection {
    pub name: String,
    pub base_url: String,
    pub models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConnection {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub models: Vec<String>,
    pub credential_ref: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub goal: String,
    /// References only; this association does not grant any tool permission.
    pub resources: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentRole {
    Lead,
    Worker,
    Verifier,
    Reviewer,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewAgent {
    pub project_id: String,
    pub name: String,
    pub role: AgentRole,
    pub connection_id: String,
    pub model_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub role: AgentRole,
    pub connection_id: String,
    pub model_id: String,
    /// Every assignment has an independent work conversation, including when
    /// several roles select the same underlying provider/model.
    pub conversation_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewTask {
    pub project_id: String,
    pub title: String,
    pub goal: String,
    pub scope: Vec<String>,
    pub acceptance: String,
    pub agent_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Draft,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub goal: String,
    pub scope: Vec<String>,
    pub acceptance: String,
    pub agent_ids: Vec<String>,
    pub status: TaskStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub project_id: Option<String>,
    pub title: String,
    pub connection_id: String,
    pub model_id: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TurnStatus {
    Running,
    Succeeded,
    Failed,
    Interrupted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatTurn {
    pub id: String,
    pub conversation_id: String,
    pub user: String,
    pub assistant: Option<String>,
    pub status: TurnStatus,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DesktopSnapshot {
    pub revision: u64,
    pub connections: Vec<AiConnection>,
    pub projects: Vec<Project>,
    pub agents: Vec<Agent>,
    pub tasks: Vec<Task>,
    pub conversations: Vec<Conversation>,
    pub turns: Vec<ChatTurn>,
    /// Runtime-only pending settlements, populated in the returned projection.
    /// The durable snapshot always stores an empty list.
    #[serde(default)]
    pub unsaved_turn_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApiMessage {
    pub role: String,
    pub content: String,
}

/// Trusted internal request snapshot. Never contains a secret or tool grant.
#[derive(Debug, Clone)]
pub struct PreparedChat {
    pub turn_id: String,
    pub conversation_id: String,
    pub connection_id: String,
    pub credential_ref: String,
    pub model_id: String,
    pub base_url: String,
    pub messages: Vec<ApiMessage>,
}
