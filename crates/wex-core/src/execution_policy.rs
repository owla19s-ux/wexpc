//! Execution strategy only. This never authenticates a connection, grants a
//! resource, bypasses OS checks, or creates an execution token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionPolicy {
    Automatic,
    Ask,
    Block,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyDecision {
    Execute,
    RequestApproval,
    Blocked,
}
pub fn decide(policy: ExecutionPolicy) -> PolicyDecision {
    match policy {
        ExecutionPolicy::Automatic => PolicyDecision::Execute,
        ExecutionPolicy::Ask => PolicyDecision::RequestApproval,
        ExecutionPolicy::Block => PolicyDecision::Blocked,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strategies_remain_distinct() {
        assert_eq!(decide(ExecutionPolicy::Automatic), PolicyDecision::Execute);
        assert_eq!(
            decide(ExecutionPolicy::Ask),
            PolicyDecision::RequestApproval
        );
        assert_eq!(decide(ExecutionPolicy::Block), PolicyDecision::Blocked);
    }
}
