use wex_desktop_service::chat_endpoint;

#[test]
fn unsafe_remote_http_endpoint_is_rejected() {
    assert!(chat_endpoint("http://api.example.org/v1").is_err());
}

#[test]
fn embedded_url_credentials_are_rejected() {
    assert!(chat_endpoint("https://user:pass@api.example.org/v1").is_err());
    assert!(chat_endpoint("https://api.example.org/v1?token=redacted").is_err());
    assert!(chat_endpoint("https://api.example.org/v1#fragment").is_err());
}

#[test]
fn endpoint_path_keeps_custom_proxy_prefix() {
    assert_eq!(
        chat_endpoint("https://api.example.org/proxy/openai/v1/").unwrap(),
        "https://api.example.org/proxy/openai/v1/chat/completions"
    );
}
