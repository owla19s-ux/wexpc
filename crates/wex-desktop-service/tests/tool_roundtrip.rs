//! Loopback proof of the model -> inert proposal -> Host callback ->
//! tool-role result -> resumed model request shape. Never performs real tools.
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::Duration,
};
use wex_api::ToolDefinition;
use wex_desktop_service::{drive_tool_rounds, ApiMessage, PreparedChat};

fn mock_exchange() -> (String, thread::JoinHandle<Vec<serde_json::Value>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let worker = thread::spawn(move || {
        let mut captured = Vec::new();
        for (index, response) in [
            r#"{"choices":[{"finish_reason":"tool_calls","message":{"role":"assistant","tool_calls":[{"id":"call_1","type":"function","function":{"name":"read_file","arguments":"{\"file_name\":\"demo.txt\"}"}}]}}]}"#,
            r#"{"choices":[{"finish_reason":"stop","message":{"role":"assistant","content":"verified reply"}}]}"#,
        ].iter().enumerate() {
            let (mut socket, _) = listener.accept().unwrap();
            socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut received = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let n = socket.read(&mut chunk).unwrap();
                assert!(n > 0);
                received.extend_from_slice(&chunk[..n]);
                if let Some(sep) = received.windows(4).position(|s| s == b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&received[..sep]).to_ascii_lowercase();
                    let len: usize = header.lines()
                        .find_map(|l| l.strip_prefix("content-length: "))
                        .unwrap().parse().unwrap();
                    if received.len() >= sep + 4 + len {
                        let value = serde_json::from_slice(&received[sep + 4..sep + 4 + len]).unwrap();
                        captured.push(value);
                        break;
                    }
                }
            }
            let body = response.as_bytes();
            let wire = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            socket.write_all(wire.as_bytes()).unwrap();
            socket.write_all(body).unwrap();
            assert_eq!(captured.len(), index + 1);
        }
        captured
    });
    (url, worker)
}

#[test]
fn approved_tool_receipt_is_bound_to_next_model_round() {
    let (base_url, worker) = mock_exchange();
    let chat = PreparedChat {
        turn_id: "turn_1".into(),
        conversation_id: "conversation_1".into(),
        connection_id: "connection_1".into(),
        credential_ref: "unused_mock".into(),
        model_id: "mock-model".into(),
        base_url,
        messages: vec![ApiMessage {role:"user".into(), content:"Inspect demo".into()}],
    };
    let tools = vec![ToolDefinition {
        name: "read_file".into(),
        description: "Test-only inert proposal".into(),
        parameters_json: r#"{"type":"object","properties":{"file_name":{"type":"string"}},"required":["file_name"]}"#.into(),
    }];
    let mut host_invocations = 0;
    let answer = drive_tool_rounds(&chat, "mock-key", tools, |calls| {
        host_invocations += 1;
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].name, "read_file");
        Ok(vec![(
            calls[0].id.clone(),
            r#"{"evidence_ref":"ev_1","content":"approved"}"#.into(),
        )])
    }).unwrap();
    assert_eq!(host_invocations, 1);
    assert_eq!(answer, "verified reply");
    let requests = worker.join().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests[0]["messages"].as_array().unwrap().iter().all(|m| m["role"] != "tool"));
    let messages = requests[1]["messages"].as_array().unwrap();
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[1]["tool_calls"][0]["id"], "call_1");
    assert_eq!(messages[2]["role"], "tool");
    assert_eq!(messages[2]["tool_call_id"], "call_1");
    assert!(messages[2]["content"].as_str().unwrap().contains("ev_1"));
}
