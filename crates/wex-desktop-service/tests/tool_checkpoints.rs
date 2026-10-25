use wex_api::{Message, Role, ToolCall};
use wex_desktop_service::{load_tool_checkpoint, save_tool_checkpoint, ApiMessage, PreparedChat};
fn request() -> PreparedChat {
    PreparedChat {
        turn_id: "turn_1".into(),
        conversation_id: "conv_1".into(),
        connection_id: "conn_1".into(),
        model_id: "model".into(),
        base_url: "https://example.test/v1".into(),
        credential_ref: "vault_not_a_key".into(),
        messages: vec![ApiMessage {
            role: "user".into(),
            content: "inspect".into(),
        }],
    }
}
fn history() -> Vec<Message> {
    vec![
        Message {
            role: Role::Assistant,
            content: Some("inspect first".into()),
            tool_calls: vec![ToolCall {
                id: "call_1".into(),
                name: "read_file".into(),
                arguments_json: "{}".into(),
            }],
            tool_call_id: None,
        },
        Message {
            role: Role::Tool,
            content: Some("exact saved output".into()),
            tool_calls: vec![],
            tool_call_id: Some("call_1".into()),
        },
    ]
}
#[test]
fn restart_keeps_exact_order_and_frozen_request_binding() {
    let dir = tempfile::tempdir().unwrap();
    let request = request();
    let h = history();
    save_tool_checkpoint(dir.path(), &request, 0, &h[..1], true).unwrap();
    assert!(load_tool_checkpoint(dir.path(), &request).is_err());
    save_tool_checkpoint(dir.path(), &request, 1, &h, false).unwrap();
    assert_eq!(load_tool_checkpoint(dir.path(), &request).unwrap(), h);
    let mut other = request.clone();
    other.model_id = "different-model".into();
    assert!(load_tool_checkpoint(dir.path(), &other).is_err());
    other = request.clone();
    other.messages[0].content = "different request".into();
    assert!(load_tool_checkpoint(dir.path(), &other).is_err());
    other = request.clone();
    other.connection_id = "different-provider".into();
    assert!(load_tool_checkpoint(dir.path(), &other).is_err());
    let bytes = std::fs::read_to_string(dir.path().join("conv_1/turn_1/00000001.json")).unwrap();
    assert!(!bytes.contains("vault_not_a_key"));
}
#[test]
fn later_pending_batch_blocks_older_complete_checkpoint() {
    let dir = tempfile::tempdir().unwrap();
    let request = request();
    let mut h = history();
    save_tool_checkpoint(dir.path(), &request, 0, &h[..1], true).unwrap();
    save_tool_checkpoint(dir.path(), &request, 1, &h, false).unwrap();
    h.push(h[0].clone());
    save_tool_checkpoint(dir.path(), &request, 2, &h, true).unwrap();
    assert!(load_tool_checkpoint(dir.path(), &request).is_err());
    assert!(save_tool_checkpoint(dir.path(), &request, 2, &h, true).is_err());
    assert!(load_tool_checkpoint(dir.path(), &request).is_err());
}
#[test]
fn mismatched_receipts_and_gapped_or_damaged_checkpoints_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let request = request();
    let mut h = history();
    h[1].tool_call_id = Some("foreign_call".into());
    assert!(save_tool_checkpoint(dir.path(), &request, 1, &h, false).is_err());
    h = history();
    assert!(save_tool_checkpoint(dir.path(), &request, 1, &h, false).is_err());
    save_tool_checkpoint(dir.path(), &request, 0, &h[..1], true).unwrap();
    save_tool_checkpoint(dir.path(), &request, 1, &h, false).unwrap();
    std::fs::write(dir.path().join("conv_1/turn_1/00000001.json"), "{").unwrap();
    assert!(load_tool_checkpoint(dir.path(), &request).is_err());
}
#[test]
fn complete_checkpoint_cannot_change_the_pre_dispatch_proposal() {
    let dir = tempfile::tempdir().unwrap();
    let request = request();
    let mut h = history();
    save_tool_checkpoint(dir.path(), &request, 0, &h[..1], true).unwrap();
    h[0].content = Some("changed assistant content".into());
    save_tool_checkpoint(dir.path(), &request, 1, &h, false).unwrap();
    assert!(load_tool_checkpoint(dir.path(), &request).is_err());
}

#[test]
fn restore_recovers_original_context_but_uses_current_credential_reference() {
    use wex_desktop_service::restore_tool_checkpoint;
    let dir = tempfile::tempdir().unwrap();
    let mut original = request();
    original.messages.insert(
        0,
        ApiMessage {
            role: "system".into(),
            content: "original frozen system context".into(),
        },
    );
    let h = history();
    save_tool_checkpoint(dir.path(), &original, 0, &h[..1], true).unwrap();
    save_tool_checkpoint(dir.path(), &original, 1, &h, false).unwrap();
    let mut current = request();
    current.credential_ref = "new_current_vault_reference".into();
    let (restored, messages) = restore_tool_checkpoint(dir.path(), &current).unwrap();
    assert_eq!(restored.messages.len(), 2);
    assert_eq!(
        restored.messages[0].content,
        "original frozen system context"
    );
    assert_eq!(restored.credential_ref, "new_current_vault_reference");
    assert_eq!(messages, h);
    current.messages[0].content = "unrelated turn prompt".into();
    assert!(restore_tool_checkpoint(dir.path(), &current).is_err());
}
