use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyChoice {
    Automatic,
    Ask,
    Block,
}
impl From<PolicyChoice> for wex_core::execution_policy::ExecutionPolicy {
    fn from(value: PolicyChoice) -> Self {
        match value {
            PolicyChoice::Automatic => Self::Automatic,
            PolicyChoice::Ask => Self::Ask,
            PolicyChoice::Block => Self::Block,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolPolicySetting {
    pub conversation_id: String,
    pub capability_id: String,
    pub policy: PolicyChoice,
    pub revision: u64,
}
