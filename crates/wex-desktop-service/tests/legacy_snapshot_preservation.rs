//! Unknown legacy top-level collections must survive typed snapshot writes.
//! Keeping bytes as data does not admit them as current trusted tool evidence.
use rusqlite::Connection;
use serde_json::{json, Value};
use std::path::Path;
use wex_desktop_service::*;

fn raw_snapshot(path: &Path) -> String {
    Connection::open(path)
        .unwrap()
        .query_row("SELECT body FROM desktop_state WHERE id=1", [], |row| {
            row.get(0)
        })
        .unwrap()
}

fn seed_legacy_fields(path: &Path, version: u32) -> Value {
    let mut body: Value = serde_json::from_str(&raw_snapshot(path)).unwrap();
    body["execution_receipts"] = json!([{
        "id": "legacy_receipt",
        "task_id": "historical_task",
        "agent_id": "historical_agent",
        "invocation_id": "legacy_call",
        "succeeded": false,
        "summary": "historical data; not current execution evidence"
    }]);
    body["agent_work_reports"] = json!([{
        "id": "legacy_report",
        "body": "unverified historical report",
        "evidence_refs": ["old-reference"]
    }]);
    body["legacy_extension"] = json!({"nested": [null, false, 17, "原始资料"]});
    let db = Connection::open(path).unwrap();
    db.execute(
        "UPDATE desktop_state SET body=?1 WHERE id=1",
        [body.to_string()],
    )
    .unwrap();
    db.pragma_update(None, "user_version", version).unwrap();
    body
}

fn assert_legacy_fields(expected: &Value, actual: &Value) {
    for key in [
        "execution_receipts",
        "agent_work_reports",
        "legacy_extension",
    ] {
        assert_eq!(actual.get(key), expected.get(key), "{key} was erased");
    }
}

#[test]
fn ordinary_write_preserves_legacy_collections_after_every_supported_upgrade() {
    for version in 1..=8 {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("legacy.sqlite");
        drop(DesktopService::open(&path).unwrap());
        let original = seed_legacy_fields(&path, version);
        let mut app = DesktopService::open(&path).unwrap();
        let revision = app.snapshot().unwrap().revision;
        let project = app
            .create_project("New project".into(), "current goal".into(), vec![])
            .unwrap();
        let actual: Value = serde_json::from_str(&raw_snapshot(&path)).unwrap();
        assert_legacy_fields(&original, &actual);
        assert_eq!(actual["revision"], json!(revision + 1));
        assert_eq!(actual["projects"][0]["id"], json!(project.id));
        // Opaque compatibility data must not become a frontend/tool DTO.
        let projection = serde_json::to_value(app.snapshot().unwrap()).unwrap();
        assert!(projection.get("execution_receipts").is_none());
        assert!(projection.get("agent_work_reports").is_none());
        drop(app);
        let reopened = DesktopService::open(&path).unwrap();
        assert_eq!(reopened.snapshot().unwrap().projects[0].id, project.id);
        let actual: Value = serde_json::from_str(&raw_snapshot(&path)).unwrap();
        assert_legacy_fields(&original, &actual);
    }
}

#[test]
fn interrupted_turn_recovery_preserves_opaque_legacy_history() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("recovery.sqlite");
    let mut app = DesktopService::open(&path).unwrap();
    let api = app
        .add_connection(
            NewConnection {
                name: "API".into(),
                base_url: "https://api.example.com".into(),
                models: vec!["model".into()],
            },
            "vault-reference".into(),
        )
        .unwrap();
    let conversation = app
        .create_conversation(None, "Chat".into(), api.id, "model".into())
        .unwrap();
    let request = app
        .prepare_chat(&conversation.id, "do not replay".into())
        .unwrap();
    drop(app);
    let original = seed_legacy_fields(&path, 5);
    let recovered = DesktopService::open(&path).unwrap();
    let snapshot = recovered.snapshot().unwrap();
    assert_eq!(snapshot.turns[0].id, request.turn_id);
    assert_eq!(snapshot.turns[0].status, TurnStatus::Interrupted);
    let actual: Value = serde_json::from_str(&raw_snapshot(&path)).unwrap();
    assert_legacy_fields(&original, &actual);
    assert_eq!(actual["revision"], json!(snapshot.revision));
}

#[test]
fn failed_write_rolls_back_and_retry_preserves_legacy_history() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("retry.sqlite");
    drop(DesktopService::open(&path).unwrap());
    let original = seed_legacy_fields(&path, 8);
    let mut app = DesktopService::open(&path).unwrap();
    let before = raw_snapshot(&path);
    let revision = app.snapshot().unwrap().revision;
    let db = Connection::open(&path).unwrap();
    db.execute_batch(
        "CREATE TRIGGER fail_update BEFORE UPDATE ON desktop_state
         BEGIN SELECT RAISE(ABORT, 'injected write failure'); END;",
    )
    .unwrap();
    let error = app
        .create_project("Retry project".into(), String::new(), vec![])
        .unwrap_err();
    assert_eq!(error.code, "STORAGE_ERROR");
    assert_eq!(raw_snapshot(&path), before);
    assert_eq!(app.snapshot().unwrap().revision, revision);
    assert!(app.snapshot().unwrap().projects.is_empty());
    db.execute_batch("DROP TRIGGER fail_update").unwrap();
    app.create_project("Retry project".into(), String::new(), vec![])
        .unwrap();
    let actual: Value = serde_json::from_str(&raw_snapshot(&path)).unwrap();
    assert_legacy_fields(&original, &actual);
    assert_eq!(actual["revision"], json!(revision + 1));
    assert_eq!(actual["projects"].as_array().unwrap().len(), 1);
}

#[test]
fn opaque_numbers_are_preserved_without_float_rounding() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("precise.sqlite");
    drop(DesktopService::open(&path).unwrap());
    let mut raw = raw_snapshot(&path);
    assert_eq!(raw.pop(), Some('}'));
    raw.push_str(",\"legacy_precise\":123456789012345678901234567890}");
    let db = Connection::open(&path).unwrap();
    db.execute("UPDATE desktop_state SET body=?1 WHERE id=1", [&raw])
        .unwrap();
    drop(db);
    let mut app = DesktopService::open(&path).unwrap();
    app.create_project("Keep exact history".into(), String::new(), vec![])
        .unwrap();
    assert!(
        raw_snapshot(&path).contains("\"legacy_precise\":123456789012345678901234567890"),
        "opaque legacy numbers must not be erased or rounded"
    );
}
