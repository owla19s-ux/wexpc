use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};
use wex_desktop_service::{chat_endpoint, execute_chat, ApiMessage, PreparedChat};

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
