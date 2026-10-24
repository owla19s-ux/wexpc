// Review-only executable invariants against the std-only API-PC-01 proposal.
// Run from repository root:
// rustc --edition=2021 --test -A dead_code reviews/tests/api_pc_01_invariants.rs -o /tmp/api-pc-01-review-tests
// /tmp/api-pc-01-review-tests
// Does NOT exercise HTTP, SSE, request validation, Windows, or a formal wex-api crate.
#[path = "../../contracts/api_pc_01.rs"]
mod api;
use api::*;

fn origin() -> ContinuationScope {
    ContinuationScope {
        connection_id: "conn".into(),
        conversation_id: "conv".into(),
        generation_id: "gen".into(),
        model: "deepseek-chat".into(),
        tool_call_ids: vec!["tool-1".into(), "tool-2".into()],
    }
}

#[test]
fn continuation_rejects_reordered_and_substituted_tool_ids() {
    let saved = Continuation { origin: origin(), payload: "sensitive-payload".into() };
    let mut reordered = origin();
    reordered.tool_call_ids.reverse();
    assert!(!saved.matches_origin(&reordered));
    let mut substituted = origin();
    substituted.tool_call_ids[1] = "tool-3".into();
    assert!(!saved.matches_origin(&substituted));
    assert!(saved.matches_origin(&origin()));
}

#[test]
fn continuation_debug_does_not_leak_payload_even_when_origin_is_printed() {
    let saved = Continuation { origin: origin(), payload: "DO-NOT-LOG-THIS".into() };
    let printed = format!("{saved:?}");
    assert!(!printed.contains("DO-NOT-LOG-THIS"));
    assert!(printed.contains("[REDACTED]"));
}

#[test]
fn cancellation_is_monotonic_across_multiple_clones() {
    let owner = Cancellation::default();
    let clones: Vec<_> = (0..16).map(|_| owner.clone()).collect();
    assert!(clones.iter().all(|c| !c.is_cancelled()));
    clones[7].cancel();
    assert!(owner.is_cancelled());
    assert!(clones.iter().all(Cancellation::is_cancelled));
    owner.cancel();
    assert!(clones.iter().all(Cancellation::is_cancelled));
}

#[test]
fn all_error_kinds_have_distinct_stable_codes() {
    use ErrorKind::*;
    let kinds = [
        InvalidRequest, CredentialUnavailable, Authentication, PermissionDenied,
        RateLimited, ProviderUnavailable, Network, ConnectTimeout, IdleTimeout,
        TotalTimeout, Cancelled, Protocol, LimitExceeded, UnsupportedParameter,
    ];
    let codes: Vec<_> = kinds.iter().map(|&kind| ApiError { kind, http_status: None }.code()).collect();
    let unique: std::collections::HashSet<_> = codes.iter().copied().collect();
    assert_eq!(unique.len(), kinds.len());
    assert!(codes.iter().all(|code| !code.is_empty() && code.chars().all(|c| c.is_ascii_uppercase() || c == '_')));
}
