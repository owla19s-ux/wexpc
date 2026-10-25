use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};
use wex_desktop_service::{
    chat_endpoint, execute_chat, execute_chat_stream, execute_chat_stream_with_progress,
    ApiMessage, PreparedChat,
};

#[test]
fn endpoint_preserves_proxy_prefix_and_explicit_version() {
    assert_eq!(
        chat_endpoint("https://api.deepseek.com/").unwrap(),
        "https://api.deepseek.com/v1/chat/completions"
    );
    assert_eq!(
        chat_endpoint("https://example.org/proxy/openai/v1/").unwrap(),
        "https://example.org/proxy/openai/v1/chat/completions"
    );
    assert!(chat_endpoint("http://example.org").is_err());
    assert!(chat_endpoint("https://user:secret@example.org").is_err());
    assert!(chat_endpoint("https://example.org?api_key=secret").is_err());
}

fn request(base_url: String) -> PreparedChat {
    PreparedChat {
        turn_id: "turn".into(),
        conversation_id: "conv".into(),
        connection_id: "c".into(),
        credential_ref: "vault".into(),
        model_id: "deepseek-chat".into(),
        base_url,
        messages: vec![ApiMessage {
            role: "user".into(),
            content: "你好".into(),
        }],
    }
}

fn server(status: &str, body: &str) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let response = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    let handle = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut bytes = vec![];
        let mut chunk = [0u8; 4096];
        loop {
            let size = socket.read(&mut chunk).unwrap();
            if size == 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..size]);
            if let Some(pos) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&bytes[..pos]).to_lowercase();
                let len: usize = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length: "))
                    .unwrap()
                    .parse()
                    .unwrap();
                if bytes.len() >= pos + 4 + len {
                    break;
                }
            }
        }
        socket.write_all(response.as_bytes()).unwrap();
        String::from_utf8(bytes).unwrap()
    });
    (format!("http://{addr}/v1"), handle)
}

#[test]
fn compatible_http_request_and_response_are_real_and_provider_errors_are_sanitized() {
    let (url, handle) = server(
        "200 OK",
        r#"{"choices":[{"message":{"content":"你好，WEX"}}]}"#,
    );
    assert_eq!(
        execute_chat(&request(url), "secret-key").unwrap(),
        "你好，WEX"
    );
    let sent = handle.join().unwrap();
    assert!(sent.starts_with("POST /v1/chat/completions "));
    assert!(sent
        .to_lowercase()
        .contains("authorization: bearer secret-key"));
    let body: serde_json::Value =
        serde_json::from_str(sent.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(body["model"], "deepseek-chat");
    assert_eq!(body["stream"], false);
    assert!(body.get("stream_options").is_none());
    assert_eq!(body["messages"][0]["content"], "你好");
    assert!(body.get("tools").is_none());
    let (url, handle) = server(
        "401 Unauthorized",
        r#"{"error":"secret-key echoed by provider"}"#,
    );
    let error = execute_chat(&request(url), "secret-key").unwrap_err();
    handle.join().unwrap();
    assert_eq!(error.code, "API_HTTP_ERROR");
    assert!(!serde_json::to_string(&error)
        .unwrap()
        .contains("secret-key"));
}

fn sse_server(frames: Vec<String>) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut request_bytes = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let n = socket.read(&mut chunk).unwrap();
            if n == 0 {
                break;
            }
            request_bytes.extend_from_slice(&chunk[..n]);
            if let Some(pos) = request_bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&request_bytes[..pos]).to_lowercase();
                let length: usize = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length: "))
                    .unwrap()
                    .parse()
                    .unwrap();
                if request_bytes.len() >= pos + 4 + length {
                    break;
                }
            }
        }
        let length: usize = frames.iter().map(String::len).sum();
        let headers = format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n");
        socket.write_all(headers.as_bytes()).unwrap();
        for frame in frames {
            socket.write_all(frame.as_bytes()).unwrap();
        }
        String::from_utf8(request_bytes).unwrap()
    });
    (format!("http://{addr}/v1"), handle)
}

#[test]
fn streaming_adapter_decodes_fragmented_sse_and_sends_stream_request() {
    let (url, handle) = sse_server(vec![
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"你好\"}}]}\n\n".into(),
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"，WEX\"},\"finish_reason\":\"stop\"}]}\n\n".into(),
        "data: [DONE]\n\n".into(),
    ]);
    let result = execute_chat_stream(&request(url), "secret-key").unwrap();
    assert_eq!(result, "你好，WEX");
    let sent = handle.join().unwrap();
    let body: serde_json::Value =
        serde_json::from_str(sent.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(body["stream"], true);
    assert_eq!(body["stream_options"]["include_usage"], true);
    assert_eq!(body["model"], "deepseek-chat");
}

#[test]
fn progress_is_provisional_until_a_terminal_frame() {
    let (url, handle) = sse_server(vec![
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"片段\"}}]}\n\n".into(),
    ]);
    let mut deltas = Vec::new();
    let error = execute_chat_stream_with_progress(&request(url), "secret-key", |delta| {
        deltas.push(delta.to_owned());
    })
    .unwrap_err();
    handle.join().unwrap();
    assert_eq!(deltas, vec!["片段"]);
    assert_eq!(error.code, "PROTOCOL");
}

#[test]
fn streaming_adapter_rejects_missing_terminal_frame() {
    let (url, handle) = sse_server(vec![
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"partial\"}}]}\n\n".into(),
    ]);
    let error = execute_chat_stream(&request(url), "secret-key").unwrap_err();
    handle.join().unwrap();
    assert_eq!(error.code, "PROTOCOL");
}

#[test]
fn streaming_adapter_does_not_execute_provider_tool_calls() {
    let (url, handle) = sse_server(vec![
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"write_file\",\"arguments\":\"{}\"}}]}}]}\n\n".into(),
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n".into(),
        "data: [DONE]\n\n".into(),
    ]);
    let error = execute_chat_stream(&request(url), "secret-key").unwrap_err();
    handle.join().unwrap();
    assert_eq!(error.code, "TOOLS_NOT_CONNECTED");
}

#[test]
fn streaming_adapter_rejects_json_success_body() {
    let (url, handle) = server(
        "200 OK",
        r#"{"choices":[{"message":{"content":"not an SSE stream"}}]}"#,
    );
    let error = execute_chat_stream(&request(url), "secret-key").unwrap_err();
    handle.join().unwrap();
    assert_eq!(error.code, "API_RESPONSE_ERROR");
}

#[test]
fn streaming_adapter_rejects_missing_credential_before_network() {
    let error = execute_chat_stream(&request("https://api.deepseek.com".into()), "").unwrap_err();
    assert_eq!(error.code, "CREDENTIAL_UNAVAILABLE");
}

#[test]
fn streaming_adapter_redacts_provider_http_errors() {
    let (url, handle) = server(
        "429 Too Many Requests",
        r#"{"error":{"message":"secret-key provider echo"}}"#,
    );
    let error = execute_chat_stream(&request(url), "secret-key").unwrap_err();
    handle.join().unwrap();
    assert_eq!(error.code, "RATE_LIMITED");
    assert!(!error.message.contains("secret-key"));
}

#[test]
fn streaming_adapter_rejects_incomplete_tool_calls() {
    let (url, handle) = sse_server(vec![
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"write_file\",\"arguments\":\"{\"}}]}}]}\n\n".into(),
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n".into(),
        "data: [DONE]\n\n".into(),
    ]);
    let error = execute_chat_stream(&request(url), "secret-key").unwrap_err();
    handle.join().unwrap();
    assert_eq!(error.code, "PROTOCOL");
}

#[test]
fn non_streaming_malformed_tool_calls_never_become_text_success() {
    let (url, handle) = server(
        "200 OK",
        r#"{"choices":[{"message":{"content":"looks successful","tool_calls":{"unexpected":"object"}}}]}"#,
    );
    let error = execute_chat(&request(url), "secret-key").unwrap_err();
    handle.join().unwrap();
    assert_eq!(error.code, "API_RESPONSE_ERROR");
}

#[test]
fn non_streaming_valid_tool_intent_is_not_executed_or_committed_as_text() {
    let (url, handle) = server(
        "200 OK",
        r#"{"choices":[{"message":{"content":"looks successful","tool_calls":[{"id":"call_1","type":"function","function":{"name":"write_file","arguments":"{}"}}]}}]}"#,
    );
    let error = execute_chat(&request(url), "secret-key").unwrap_err();
    handle.join().unwrap();
    assert_eq!(error.code, "TOOLS_NOT_CONNECTED");
}

#[test]
fn both_transports_reject_unsupported_transcript_roles_before_network() {
    let mut invalid = request("https://api.deepseek.com".into());
    invalid.messages[0].role = "tool".into();
    let non_stream = execute_chat(&invalid, "secret-key").unwrap_err();
    let stream = execute_chat_stream(&invalid, "secret-key").unwrap_err();
    assert_eq!(non_stream.code, "INVALID_INPUT");
    assert_eq!(stream.code, "INVALID_INPUT");
}

#[test]
fn both_transports_encode_the_same_ordered_transcript() {
    let (url, handle) = server("200 OK", r#"{"choices":[{"message":{"content":"ok"}}]}"#);
    let mut complete = request(url);
    complete.messages.insert(
        0,
        ApiMessage {
            role: "system".into(),
            content: "project context is not permission".into(),
        },
    );
    complete.messages.push(ApiMessage {
        role: "assistant".into(),
        content: "previous reply".into(),
    });
    assert_eq!(execute_chat(&complete, "secret-key").unwrap(), "ok");
    let sent = handle.join().unwrap();
    let complete_body: serde_json::Value =
        serde_json::from_str(sent.split("\r\n\r\n").nth(1).unwrap()).unwrap();

    let (url, handle) = sse_server(vec![
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"ok\"},\"finish_reason\":\"stop\"}]}\n\n".into(),
        "data: [DONE]\n\n".into(),
    ]);
    let mut streaming = complete;
    streaming.base_url = url;
    assert_eq!(execute_chat_stream(&streaming, "secret-key").unwrap(), "ok");
    let sent = handle.join().unwrap();
    let stream_body: serde_json::Value =
        serde_json::from_str(sent.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(complete_body["messages"], stream_body["messages"]);
    assert_eq!(complete_body["messages"][0]["role"], "system");
    assert_eq!(complete_body["messages"][2]["role"], "assistant");
}

#[test]
fn non_streaming_rejects_truncated_reply_even_with_text() {
    let (url, handle) = server(
        "200 OK",
        r#"{"choices":[{"finish_reason":"length","message":{"content":"unfinished"}}]}"#,
    );
    let error = execute_chat(&request(url), "secret-key").unwrap_err();
    handle.join().unwrap();
    assert_eq!(error.code, "API_RESPONSE_INCOMPLETE");
}

#[test]
fn non_streaming_rejects_tool_finish_reason_without_tool_payload() {
    let (url, handle) = server(
        "200 OK",
        r#"{"choices":[{"finish_reason":"tool_calls","message":{"content":"not a final reply"}}]}"#,
    );
    let error = execute_chat(&request(url), "secret-key").unwrap_err();
    handle.join().unwrap();
    assert_eq!(error.code, "TOOLS_NOT_CONNECTED");
}

#[test]
fn non_streaming_rejects_multiple_candidates_and_unknown_finish_reason() {
    for body in [
        r#"{"choices":[{"message":{"content":"one"}},{"message":{"content":"two"}}]}"#,
        r#"{"choices":[{"finish_reason":"unexpected","message":{"content":"one"}}]}"#,
    ] {
        let (url, handle) = server("200 OK", body);
        let error = execute_chat(&request(url), "secret-key").unwrap_err();
        handle.join().unwrap();
        assert_eq!(error.code, "API_RESPONSE_ERROR");
    }
}

#[test]
fn non_streaming_accepts_explicit_stop_finish_reason() {
    let (url, handle) = server(
        "200 OK",
        r#"{"choices":[{"finish_reason":"stop","message":{"content":"complete"}}]}"#,
    );
    assert_eq!(
        execute_chat(&request(url), "secret-key").unwrap(),
        "complete"
    );
    handle.join().unwrap();
}

#[test]
fn legacy_function_call_cannot_be_committed_as_plain_text() {
    let (url, handle) = server(
        "200 OK",
        r#"{"choices":[{"message":{"content":"looks complete","function_call":{"name":"write_file","arguments":"{}"}}}]}"#,
    );
    let error = execute_chat(&request(url), "secret-key").unwrap_err();
    handle.join().unwrap();
    assert_eq!(error.code, "TOOLS_NOT_CONNECTED");
}

#[test]
fn approved_tool_round_trip_sends_exact_tool_receipt_to_next_model_step() {
    use wex_desktop_service::drive_tool_rounds;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let mut second_body = serde_json::Value::Null;
        for round in 0..2 {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut incoming = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                let n = socket.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                incoming.extend_from_slice(&buf[..n]);
                if let Some(pos) = incoming.windows(4).position(|v| v == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&incoming[..pos]).to_lowercase();
                    let len: usize = head
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length: "))
                        .unwrap()
                        .parse()
                        .unwrap();
                    if incoming.len() >= pos + 4 + len {
                        break;
                    }
                }
            }
            let text = String::from_utf8(incoming).unwrap();
            let body: serde_json::Value =
                serde_json::from_str(text.split("\r\n\r\n").nth(1).unwrap()).unwrap();
            if round == 1 {
                second_body = body;
            }
            let answer = if round == 0 {
                r#"{"choices":[{"finish_reason":"tool_calls","message":{"role":"assistant","tool_calls":[{"id":"call_1","type":"function","function":{"name":"read_file","arguments":"{}"}}]}}]}"#
            } else {
                r#"{"choices":[{"finish_reason":"stop","message":{"role":"assistant","content":"读取完成"}}]}"#
            };
            let header = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", answer.len());
            socket.write_all(header.as_bytes()).unwrap();
            socket.write_all(answer.as_bytes()).unwrap();
        }
        second_body
    });
    let mut approvals = 0;
    let result = drive_tool_rounds(
        &request(base),
        "fake-token",
        vec![wex_api::ToolDefinition {
            name: "read_file".into(),
            description: "test".into(),
            parameters_json: "{}".into(),
        }],
        |calls| {
            approvals += 1;
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0].id, "call_1");
            Ok(vec![("call_1".into(), "trusted receipt".into())])
        },
    )
    .unwrap();
    assert_eq!(result, "读取完成");
    assert_eq!(approvals, 1);
    let body = server.join().unwrap();
    assert_eq!(body["messages"][1]["role"], "assistant");
    assert_eq!(body["messages"][1]["tool_calls"][0]["id"], "call_1");
    assert_eq!(body["messages"][2]["role"], "tool");
    assert_eq!(body["messages"][2]["tool_call_id"], "call_1");
    assert_eq!(body["messages"][2]["content"], "trusted receipt");
}

#[test]
fn tool_round_rejection_must_not_issue_second_api_request() {
    use wex_desktop_service::drive_tool_rounds;
    let (base, handle) = server(
        "200 OK",
        r#"{"choices":[{"finish_reason":"tool_calls","message":{"role":"assistant","tool_calls":[{"id":"call_1","type":"function","function":{"name":"read_file","arguments":"{}"}}]}}]}"#,
    );
    let result = drive_tool_rounds(
        &request(base),
        "fake-token",
        vec![wex_api::ToolDefinition {
            name: "read_file".into(),
            description: "test".into(),
            parameters_json: "{}".into(),
        }],
        |_| {
            Err(wex_desktop_service::ServiceError::new(
                "PERMISSION_DENIED",
                "user rejected",
            ))
        },
    );
    handle.join().unwrap();
    assert_eq!(result.unwrap_err().code, "PERMISSION_DENIED");
}
