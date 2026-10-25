#[path = "../api_pc_01.rs"]
mod api;
use api::*;

#[test]
fn cancellation_is_shared_between_owner_and_consumer() {
    let owner = Cancellation::default();
    let consumer = owner.clone();
    assert!(!consumer.is_cancelled());
    owner.cancel();
    assert!(consumer.is_cancelled());
}
#[test]
fn continuation_requires_exact_origin_and_complete_tool_ids() {
    let scope = ContinuationScope {
        connection_id: "c".into(),
        conversation_id: "s".into(),
        generation_id: "g".into(),
        model: "m".into(),
        tool_call_ids: vec!["a".into(), "b".into()],
    };
    let saved = Continuation {
        origin: scope.clone(),
        payload: "private reasoning".into(),
    };
    assert!(saved.matches_origin(&scope));
    for field in 0..5 {
        let mut changed = scope.clone();
        match field {
            0 => changed.connection_id = "other".into(),
            1 => changed.conversation_id = "other".into(),
            2 => changed.generation_id = "other".into(),
            3 => changed.model = "other".into(),
            _ => {
                changed.tool_call_ids.pop();
            }
        }
        assert!(!saved.matches_origin(&changed));
    }
    assert!(!format!("{saved:?}").contains("private reasoning"));
}
#[test]
fn credential_debug_never_contains_secret() {
    let key = Credential::new("key-secret".into());
    assert!(!format!("{key:?}").contains("key-secret"));
    assert_eq!(key.expose(), "key-secret");
}
#[test]
fn provider_error_has_only_stable_code_and_status() {
    let error = ApiError {
        kind: ErrorKind::RateLimited,
        http_status: Some(429),
    };
    assert_eq!(error.code(), "RATE_LIMITED");
    assert_eq!(error.http_status, Some(429));
}
