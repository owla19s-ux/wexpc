use serde::{Deserialize, Serialize};

/// A member can hold different work duties in different projects. This is
/// descriptive assignment metadata, NEVER a filesystem or tool grant.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentDutyAssignment {
    pub id: String,
    pub agent_id: String,
    /// None means a user-level duty (for example secretary).
    pub project_id: Option<String>,
    /// Extensible duty identity. Existing names are lead/worker/reviewer/verifier;
    /// "secretary" is reserved for the user-level interaction duty.
    pub duty_id: String,
}

pub fn valid_duty_id(duty: &str) -> bool {
    !duty.is_empty()
        && duty.len() <= 64
        && duty
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_extensible_duties_but_rejects_malformed_names() {
        assert!(valid_duty_id("secretary"));
        assert!(valid_duty_id("project_architect"));
        assert!(!valid_duty_id(""));
        assert!(!valid_duty_id("project admin"));
        assert!(!valid_duty_id("../root"));
    }
}
