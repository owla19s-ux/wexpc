use serde_json::Value;
use std::time::Duration;
use wex_api::*;

fn request() -> ApiRequest {
    ApiRequest {
        request_id: "req-1".into(),
        connection_id: "conn-1".into(),
        conversation_id: "conv-1".into(),
        model: "deepseek-chat".into(),
        endpoint: "https://api.example.test/v1".into(),
        messages: vec![Message {
            role: Role::User,
            content: Some("hi".into()),
            tool_calls: vec![],
            tool_call_id: None,
        }],
        tools: vec![],
        parameters_json: "{}".into(),
        continuation: None,
    }
}

fn assistant_call_request() -> ApiRequest {
    let mut req = request();
    req.messages.push(Message {
        role: Role::Assistant,
        content: None,
        tool_calls: vec![ToolCall {
            id: "call-1".into(),
            name: "read_file".into(),
            arguments_json: "{\"path\":\"a\"}".into(),
        }],
        tool_call_id: None,
    });
    req
}

#[test]
fn encodes_basic_streaming_text_request() {
    let body = encode_request(&request(), EncodeOptions::default()).expect("encode");
    assert_eq!(body["model"], "deepseek-chat");
    assert_eq!(body["stream"], true);
    assert_eq!(body["stream_options"]["include_usage"], true);
    assert_eq!(body["messages"][0]["role"], "user");
    assert_eq!(body["messages"][0]["content"], "hi");
    assert!(body.get("tools").is_none());
}

#[test]
fn non_streaming_request_omits_stream_fields() {
    let options = EncodeOptions {
        stream: false,
        include_usage: false,
        replay_continuation: true,
    };
    let body = encode_request(&request(), options).expect("encode");
    assert!(body.get("stream").is_none());
    assert!(body.get("stream_options").is_none());
}

#[test]
fn merges_parameters_into_body() {
    let mut req = request();
    req.parameters_json = r#"{"temperature":0.2,"top_p":0.9}"#.into();
    let body = encode_request(&req, EncodeOptions::default()).expect("encode");
    assert_eq!(body["temperature"], 0.2);
    assert_eq!(body["top_p"], 0.9);
}

#[test]
fn reserved_parameters_are_rejected() {
    let mut req = request();
    req.parameters_json = r#"{"stream":false}"#.into();
    assert_eq!(
        encode_request(&req, EncodeOptions::default())
            .expect_err("reserved")
            .kind,
        ErrorKind::InvalidRequest
    );
}

#[test]
fn encodes_tools_and_assistant_tool_calls() {
    let mut req = assistant_call_request();
    req.tools.push(ToolDefinition {
        name: "read_file".into(),
        description: "read".into(),
        parameters_json: r#"{"type":"object"}"#.into(),
    });
    let body = encode_request(&req, EncodeOptions::default()).expect("encode");
    assert_eq!(body["tools"][0]["type"], "function");
    assert_eq!(body["tools"][0]["function"]["name"], "read_file");
    assert_eq!(body["messages"][1]["content"], Value::Null);
    assert_eq!(body["messages"][1]["tool_calls"][0]["id"], "call-1");
    assert_eq!(
        body["messages"][1]["tool_calls"][0]["function"]["arguments"],
        r#"{"path":"a"}"#
    );
}

#[test]
fn tool_message_keeps_tool_call_id() {
    let mut req = request();
    req.messages.push(Message {
        role: Role::Tool,
        content: Some("done".into()),
        tool_calls: vec![],
        tool_call_id: Some("call-1".into()),
    });
    let body = encode_request(&req, EncodeOptions::default()).expect("encode");
    assert_eq!(body["messages"][1]["role"], "tool");
    assert_eq!(body["messages"][1]["tool_call_id"], "call-1");
}

#[test]
fn replays_continuation_into_originating_assistant_message() {
    let mut req = assistant_call_request();
    let scope = ContinuationScope {
        connection_id: req.connection_id.clone(),
        conversation_id: req.conversation_id.clone(),
        generation_id: "gen-1".into(),
        model: req.model.clone(),
        tool_call_ids: vec!["call-1".into()],
    };
    req.continuation = Some(Continuation::new(scope, "opaque-reasoning".into()).expect("ok"));
    let body = encode_request(&req, EncodeOptions::default()).expect("encode");
    assert_eq!(body["messages"][1]["reasoning_content"], "opaque-reasoning");
}

#[test]
fn continuation_can_be_disabled() {
    let mut req = assistant_call_request();
    let scope = ContinuationScope {
        connection_id: req.connection_id.clone(),
        conversation_id: req.conversation_id.clone(),
        generation_id: "gen-1".into(),
        model: req.model.clone(),
        tool_call_ids: vec!["call-1".into()],
    };
    req.continuation = Some(Continuation::new(scope, "opaque".into()).expect("ok"));
    let options = EncodeOptions {
        replay_continuation: false,
        ..EncodeOptions::default()
    };
    let body = encode_request(&req, options).expect("encode");
    assert!(body["messages"][1].get("reasoning_content").is_none());
}

#[test]
fn continuation_scope_mismatch_is_rejected() {
    let mut req = assistant_call_request();
    let scope = ContinuationScope {
        connection_id: req.connection_id.clone(),
        conversation_id: "other-conv".into(),
        generation_id: "gen-1".into(),
        model: req.model.clone(),
        tool_call_ids: vec!["call-1".into()],
    };
    req.continuation = Some(Continuation::new(scope, "opaque".into()).expect("ok"));
    assert_eq!(
        encode_request(&req, EncodeOptions::default())
            .expect_err("scope mismatch")
            .kind,
        ErrorKind::InvalidRequest
    );
}

#[test]
fn continuation_tool_ids_mismatch_is_rejected() {
    let mut req = assistant_call_request();
    let scope = ContinuationScope {
        connection_id: req.connection_id.clone(),
        conversation_id: req.conversation_id.clone(),
        generation_id: "gen-1".into(),
        model: req.model.clone(),
        tool_call_ids: vec!["call-9".into()],
    };
    req.continuation = Some(Continuation::new(scope, "opaque".into()).expect("ok"));
    assert_eq!(
        encode_request(&req, EncodeOptions::default())
            .expect_err("tool ids")
            .kind,
        ErrorKind::InvalidRequest
    );
}

#[test]
fn http_status_maps_to_stable_kinds() {
    let cases = [
        (400, ErrorKind::InvalidRequest),
        (401, ErrorKind::Authentication),
        (403, ErrorKind::PermissionDenied),
        (408, ErrorKind::Network),
        (413, ErrorKind::LimitExceeded),
        (429, ErrorKind::RateLimited),
        (500, ErrorKind::ProviderUnavailable),
        (503, ErrorKind::ProviderUnavailable),
    ];
    for (status, kind) in cases {
        let error = error_for_status(status, None);
        assert_eq!(error.kind, kind, "status {status}");
        assert_eq!(error.http_status, Some(status));
    }
}

#[test]
fn rate_limit_keeps_bounded_retry_after() {
    let error = error_for_status(429, Some(Duration::from_secs(7)));
    assert_eq!(error.retry_after, Some(Duration::from_secs(7)));
    assert_eq!(error.code(), "RATE_LIMITED");
    assert_eq!(
        error_for_status(500, Some(Duration::from_secs(7))).retry_after,
        None
    );
}

#[test]
fn provider_error_body_is_classified_without_echoing_text() {
    let body = br#"{"error":{"code":"invalid_api_key","message":"sk-secret-token-123 rejected"}}"#;
    let error = provider_error_from_body(body).expect("detected");
    assert_eq!(error.kind, ErrorKind::Authentication);
    assert!(!error.detail.contains("sk-secret-token-123"));
    assert!(!error.detail.contains("rejected"));
}

#[test]
fn ordinary_body_is_not_mistaken_for_error() {
    assert!(provider_error_from_body(br#"{"choices":[]}"#).is_none());
    assert!(provider_error_from_body(b"not json").is_none());
}

#[test]
fn provider_codes_classify_stably() {
    assert_eq!(
        classify_provider_code("rate_limit_exceeded"),
        ErrorKind::RateLimited
    );
    assert_eq!(
        classify_provider_code("context_length_exceeded"),
        ErrorKind::LimitExceeded
    );
    assert_eq!(
        classify_provider_code("overloaded"),
        ErrorKind::ProviderUnavailable
    );
    assert_eq!(
        classify_provider_code("unsupported_parameter"),
        ErrorKind::UnsupportedParameter
    );
    assert_eq!(
        classify_provider_code("something_else"),
        ErrorKind::Protocol
    );
}

#[test]
fn stream_options_cannot_override_usage_policy() {
    let mut req = request();
    req.parameters_json = r#"{"stream_options":{"include_usage":false}}"#.into();
    assert_eq!(
        encode_request(&req, EncodeOptions::default())
            .expect_err("stream options reserved")
            .kind,
        ErrorKind::InvalidRequest
    );
}

#[test]
fn retry_after_is_bounded_at_error_mapping_boundary() {
    let error = error_for_status(429, Some(Duration::from_secs(86_400)));
    assert_eq!(error.retry_after, Some(Duration::from_secs(3_600)));
    assert_eq!(error_for_status(429, None).retry_after, None);
}
