use super::*;
fn supported(capability: &str) -> bool {
    matches!(
        capability,
        "github_file_read" | "github_file_create" | "github_file_write"
    )
}
fn setting(
    snapshot: &DesktopSnapshot,
    conversation: &str,
    capability: &str,
) -> Result<ToolPolicySetting> {
    if !supported(capability) {
        return Err(ServiceError::new(
            "POLICY_CAPABILITY_UNAVAILABLE",
            "该能力尚未接入可配置执行策略",
        ));
    }
    if !snapshot.conversations.iter().any(|c| c.id == conversation) {
        return Err(ServiceError::new("CONVERSATION_NOT_FOUND", "对话不存在"));
    }
    let rows: Vec<_> = snapshot
        .tool_policies
        .iter()
        .filter(|p| p.conversation_id == conversation && p.capability_id == capability)
        .collect();
    if rows.len() > 1 || rows.first().is_some_and(|p| p.revision == 0) {
        return Err(ServiceError::new("INVALID_SNAPSHOT", "执行策略记录冲突"));
    }
    Ok(rows
        .first()
        .map(|p| (*p).clone())
        .unwrap_or(ToolPolicySetting {
            conversation_id: conversation.into(),
            capability_id: capability.into(),
            policy: PolicyChoice::Ask,
            revision: 0,
        }))
}
impl DesktopService {
    pub fn tool_policy(&self, conversation: &str, capability: &str) -> Result<ToolPolicySetting> {
        setting(&self.store.load()?, conversation, capability)
    }
    pub fn set_tool_policy(
        &mut self,
        conversation: &str,
        capability: &str,
        expected_revision: u64,
        policy: PolicyChoice,
    ) -> Result<ToolPolicySetting> {
        self.mutate(|s| {
            let current = setting(s, conversation, capability)?;
            if current.revision != expected_revision {
                return Err(ServiceError::new(
                    "POLICY_CONFLICT",
                    "执行策略已变化，请刷新后核对",
                ));
            }
            let next = ToolPolicySetting {
                policy,
                revision: current.revision.checked_add(1).ok_or_else(|| {
                    ServiceError::new("REVISION_EXHAUSTED", "执行策略修订号已用尽")
                })?,
                ..current
            };
            if let Some(row) = s
                .tool_policies
                .iter_mut()
                .find(|p| p.conversation_id == conversation && p.capability_id == capability)
            {
                *row = next.clone();
            } else {
                s.tool_policies.push(next.clone());
            }
            Ok(next)
        })
    }
    pub fn check_tool_policy(&self, expected: &ToolPolicySetting) -> Result<()> {
        let current = self.tool_policy(&expected.conversation_id, &expected.capability_id)?;
        if current != *expected {
            return Err(ServiceError::new(
                "POLICY_CHANGED",
                "执行策略已变化，本次未派发工具",
            ));
        }
        if wex_core::execution_policy::decide(current.policy.into())
            == wex_core::execution_policy::PolicyDecision::Blocked
        {
            return Err(ServiceError::new(
                "POLICY_BLOCKED",
                "本对话已拦截该工具能力",
            ));
        }
        Ok(())
    }
}
