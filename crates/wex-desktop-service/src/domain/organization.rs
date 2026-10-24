use serde::{Deserialize, Serialize};

/// Local organization, not an account, sharing policy or directory grant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Space {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub id: String,
    pub space_id: String,
    pub name: String,
}

/// General work context. No mandatory code repository, project or AI member.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Context {
    pub id: String,
    pub name: String,
    pub goal: String,
    pub resources: Vec<String>,
}

/// One primary location in this implementation, separate from object identity.
/// Moving this record never changes resource or execution permissions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPlacement {
    pub context_id: String,
    pub space_id: String,
    pub group_id: Option<String>,
}
