use wex_api::*;

fn options() -> RequestOptions {
    RequestOptions::default()
}

fn frame(json: &str) -> String {
    format!("data: {json}\n\n")
}

fn collect(decoder: &mut Decoder, input: &str) -> Vec<Event> {
    decoder.push(input.as_bytes()).expect("push ok")
}

fn scope() -> ContinuationScope {
    ContinuationScope {
        connection_id: "conn".into(),
        conversation_id: "conv".into(),
        generation_id: "gen".into(),
        model: "m".into(),
        tool_call_ids: vec![],
    }
}

#[test]
fn text_stream_produces_deltas_and_completion() {
    let mut decoder = Decoder::new(options());
    let mut events = Vec::new();
    events.extend(collect(
        &mut decoder,
        &frame(r#"{"choices":[{"index":0,"delta":{"role":"assistant","content":"Hel"}}]}"#),
    ));
    events.extend(collect(
        &mut decoder,
        &frame(r#"{"choices":[{"index":0,"delta":{"content":"lo"}}]}"#),
    ));
    events.extend(collect(
        &mut decoder,
        &frame(r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#),
    ));
    events.extend(collect(&mut decoder, "data: [DONE]\n\n"));

    assert_eq!(events[0], Event::TextDelta("Hel".into()));
    assert_eq!(events[1], Event::TextDelta("lo".into()));
    assert!(matches!(events[2], Event::Completed(_)));

    let completion = decoder.finish().expect("complete");
    assert_eq!(completion.text, "Hello");
    assert!(completion.tool_calls.is_empty());
    assert!(completion.usage.is_none());
}

#[test]
fn utf8_split_across_network_chunks() {
    let mut decoder = Decoder::new(options());
    let body = frame(r#"{"choices":[{"index":0,"delta":{"content":"你好世界"}}]}"#);
    let bytes = body.as_bytes().to_vec();
    let needle = "你".as_bytes();
    let start = bytes
        .windows(needle.len())
        .position(|w| w == needle)
        .expect("found");
    let split = start + 1;
    assert!(decoder.push(&bytes[..split]).is_ok());
    let events = decoder.push(&bytes[split..]).expect("second half ok");
    assert_eq!(events, vec![Event::TextDelta("你好世界".into())]);
}

#[test]
fn crlf_and_multiline_data_are_accepted() {
    let mut decoder = Decoder::new(options());
    let body = concat!(
        "data: {\"choices\":[{\"index\":0,\r\n",
        "data: \"delta\":{\"content\":\"A\"}}]}\r\n",
        "\r\n",
    );
    let events = collect(&mut decoder, body);
    assert_eq!(events, vec![Event::TextDelta("A".into())]);
}

#[test]
fn comments_and_named_fields_are_ignored() {
    let mut decoder = Decoder::new(options());
    let body = concat!(
        ": keep-alive\n\n",
        "event: message\n",
        "id: 7\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"x\"}}]}\n\n"
    );
    let events = collect(&mut decoder, body);
    assert_eq!(events, vec![Event::TextDelta("x".into())]);
}

#[test]
fn tool_call_fragments_aggregate_by_index() {
    let mut decoder = Decoder::new(options());
    collect(
        &mut decoder,
        &frame(
            r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"read_file","arguments":"{\"pa"}}]}}]}"#,
        ),
    );
    collect(
        &mut decoder,
        &frame(
            r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"th\":\"a\"}"}}]}}]}"#,
        ),
    );
    let events = collect(
        &mut decoder,
        &frame(r#"{"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#),
    );
    assert!(matches!(events.last(), Some(Event::Completed(_))));

    let completion = decoder.finish().expect("complete");
    assert_eq!(completion.tool_calls.len(), 1);
    assert_eq!(completion.tool_calls[0].id, "call_1");
    assert_eq!(completion.tool_calls[0].name, "read_file");
    assert_eq!(completion.tool_calls[0].arguments_json, "{\"path\":\"a\"}");
}

#[test]
fn usage_tail_after_terminal_is_allowed_once() {
    let mut decoder = Decoder::new(options());
    let mut events = collect(
        &mut decoder,
        &frame(r#"{"choices":[{"index":0,"delta":{"content":"hi"},"finish_reason":"stop"}]}"#),
    );
    events.extend(collect(
        &mut decoder,
        &frame(r#"{"choices":[],"usage":{"prompt_tokens":5,"completion_tokens":3}}"#),
    ));
    events.extend(collect(&mut decoder, "data: [DONE]\n\n"));

    let completed = events
        .iter()
        .filter(|event| matches!(event, Event::Completed(_)))
        .count();
    assert_eq!(completed, 1);
    assert!(events.iter().any(|event| matches!(event, Event::Usage(_))));
    let completion = decoder.finish().expect("complete");
    assert_eq!(completion.usage.expect("usage").input_tokens, Some(5));
}

#[test]
fn eof_without_terminal_frame_fails() {
    let mut decoder = Decoder::new(options());
    collect(
        &mut decoder,
        &frame(r#"{"choices":[{"index":0,"delta":{"content":"partial"}}]}"#),
    );
    assert_eq!(
        decoder.finish().expect_err("must fail").kind,
        ErrorKind::Protocol
    );
}

#[test]
fn truncated_finish_reason_is_not_success() {
    let mut decoder = Decoder::new(options());
    let error = decoder
        .push(frame(r#"{"choices":[{"index":0,"delta":{},"finish_reason":"length"}]}"#).as_bytes())
        .expect_err("length must fail");
    assert_eq!(error.kind, ErrorKind::Protocol);
}

#[test]
fn duplicate_terminal_frame_fails() {
    let mut decoder = Decoder::new(options());
    collect(
        &mut decoder,
        &frame(r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#),
    );
    let error = decoder
        .push(frame(r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#).as_bytes())
        .expect_err("duplicate must fail");
    assert_eq!(error.kind, ErrorKind::Protocol);
}

#[test]
fn content_after_terminal_fails() {
    let mut decoder = Decoder::new(options());
    collect(
        &mut decoder,
        &frame(r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#),
    );
    let error = decoder
        .push(frame(r#"{"choices":[{"index":0,"delta":{"content":"late"}}]}"#).as_bytes())
        .expect_err("late content must fail");
    assert_eq!(error.kind, ErrorKind::Protocol);
}

#[test]
fn response_size_limit_is_enforced() {
    let mut opts = options();
    opts.max_response_bytes = 32;
    let mut decoder = Decoder::new(opts);
    let mut chunk = vec![b'a'; 40];
    chunk.push(b'\n');
    let error = decoder.push(&chunk).expect_err("limit");
    assert_eq!(error.kind, ErrorKind::LimitExceeded);
}

#[test]
fn unterminated_frame_limit_is_enforced() {
    let mut opts = options();
    opts.max_frame_bytes = 16;
    let mut decoder = Decoder::new(opts);
    let error = decoder.push(&[b'x'; 32]).expect_err("frame limit");
    assert_eq!(error.kind, ErrorKind::LimitExceeded);
}

#[test]
fn frames_after_done_fail_immediately() {
    let mut decoder = Decoder::new(options());
    collect(
        &mut decoder,
        &frame(r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#),
    );
    let input = concat!(
        "data: [DONE]\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"late\"}}]}\n\n",
    );
    let error = decoder
        .push(input.as_bytes())
        .expect_err("frame after DONE");
    assert_eq!(error.kind, ErrorKind::Protocol);
}

#[test]
fn cancellation_is_observed() {
    let cancellation = Cancellation::default();
    cancellation.cancel();
    let mut decoder = Decoder::new(options()).with_cancellation(cancellation);
    let error = decoder.push(b"data: {}\n\n").expect_err("cancelled");
    assert_eq!(error.kind, ErrorKind::Cancelled);
}

#[test]
fn invalid_json_frame_fails() {
    let mut decoder = Decoder::new(options());
    let error = decoder.push(b"data: not-json\n\n").expect_err("bad json");
    assert_eq!(error.kind, ErrorKind::Protocol);
}

#[test]
fn stop_with_pending_tool_calls_fails() {
    let mut decoder = Decoder::new(options());
    collect(
        &mut decoder,
        &frame(
            r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"f","arguments":"{\"a\":1}"}}]}}]}"#,
        ),
    );
    let error = decoder
        .push(frame(r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#).as_bytes())
        .expect_err("stop with pending calls must fail");
    assert_eq!(error.kind, ErrorKind::Protocol);
}

#[test]
fn incomplete_tool_call_missing_name_fails() {
    let mut decoder = Decoder::new(options());
    collect(
        &mut decoder,
        &frame(
            r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"arguments":"{\"a\":1}"}}]}}]}"#,
        ),
    );
    let error = decoder
        .push(
            frame(r#"{"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#)
                .as_bytes(),
        )
        .expect_err("missing name must fail");
    assert_eq!(error.kind, ErrorKind::Protocol);
}

#[test]
fn reasoning_requires_scope_and_is_redacted() {
    let mut decoder = Decoder::new(options());
    collect(
        &mut decoder,
        &frame(r#"{"choices":[{"index":0,"delta":{"reasoning_content":"secret"}}]}"#),
    );
    let error = decoder
        .push(frame(r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#).as_bytes())
        .expect_err("reasoning without scope must fail");
    assert_eq!(error.kind, ErrorKind::Protocol);

    let mut decoder = Decoder::new(options()).with_continuation_scope(scope());
    collect(
        &mut decoder,
        &frame(r#"{"choices":[{"index":0,"delta":{"reasoning_content":"secret"}}]}"#),
    );
    collect(
        &mut decoder,
        &frame(r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#),
    );
    let completion = decoder.finish().expect("complete");
    let continuation = completion.continuation.expect("reasoning payload");
    assert_eq!(continuation.payload(), "secret");
    assert!(!format!("{continuation:?}").contains("secret"));
}

#[test]
fn credential_debug_is_redacted() {
    let credential = Credential::new("sk-secret".into());
    assert!(!format!("{credential:?}").contains("sk-secret"));
}

#[test]
fn request_validation_rejects_reserved_parameters() {
    let request = ApiRequest {
        request_id: "r".into(),
        connection_id: "c".into(),
        conversation_id: "v".into(),
        model: "m".into(),
        endpoint: "https://example.test/v1".into(),
        messages: vec![Message {
            role: Role::User,
            content: Some("hi".into()),
            tool_calls: vec![],
            tool_call_id: None,
        }],
        tools: vec![],
        parameters_json: r#"{"model":"override"}"#.into(),
        continuation: None,
    };
    let error = request.validate().expect_err("reserved key");
    assert_eq!(error.kind, ErrorKind::InvalidRequest);
}

#[test]
fn tool_message_requires_tool_call_id() {
    let message = Message {
        role: Role::Tool,
        content: Some("result".into()),
        tool_calls: vec![],
        tool_call_id: None,
    };
    assert_eq!(
        message.validate().expect_err("needs id").kind,
        ErrorKind::InvalidRequest
    );
}

#[test]
fn proxy_validation_rejects_userinfo() {
    assert!(Proxy::explicit("http://user:pass@host:8080").is_err());
    assert!(Proxy::explicit("socks5://127.0.0.1:1080").is_ok());
}

#[test]
fn continuation_size_is_bounded() {
    let payload = "x".repeat(DEFAULT_MAX_CONTINUATION_BYTES + 1);
    let error = Continuation::new(scope(), payload).expect_err("too large");
    assert_eq!(error.kind, ErrorKind::LimitExceeded);
}
