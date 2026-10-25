use wex_desktop_service::*;
fn conversations(app: &mut DesktopService) -> (AiConnection, Conversation, Conversation) {
    let c = app
        .add_connection(
            NewConnection {
                name: "API".into(),
                base_url: "https://api.deepseek.com".into(),
                models: vec!["deepseek-chat".into()],
            },
            "vault".into(),
        )
        .unwrap();
    let a = app
        .create_conversation(None, "A".into(), c.id.clone(), "deepseek-chat".into())
        .unwrap();
    let b = app
        .create_conversation(None, "B".into(), c.id.clone(), "deepseek-chat".into())
        .unwrap();
    (c, a, b)
}
#[test]
fn default_asks_and_persisted_policy_is_scoped_to_one_capability_and_conversation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db.sqlite");
    let mut app = DesktopService::open(&path).unwrap();
    let (_, a, b) = conversations(&mut app);
    assert_eq!(
        app.tool_policy(&a.id, "github_file_read").unwrap().policy,
        PolicyChoice::Ask
    );
    let automatic = app
        .set_tool_policy(&a.id, "github_file_read", 0, PolicyChoice::Automatic)
        .unwrap();
    app.check_tool_policy(&automatic).unwrap();
    assert_eq!(
        app.tool_policy(&a.id, "github_file_write").unwrap().policy,
        PolicyChoice::Ask
    );
    assert_eq!(
        app.tool_policy(&b.id, "github_file_read").unwrap().policy,
        PolicyChoice::Ask
    );
    drop(app);
    let app = DesktopService::open(&path).unwrap();
    assert_eq!(
        app.tool_policy(&a.id, "github_file_read").unwrap(),
        automatic
    );
}
#[test]
fn revocation_and_stale_editor_are_enforced_without_unrelated_revision_binding() {
    let mut app = DesktopService::memory().unwrap();
    let (_, a, b) = conversations(&mut app);
    let automatic = app
        .set_tool_policy(&a.id, "github_file_write", 0, PolicyChoice::Automatic)
        .unwrap();
    app.create_project("Unrelated".into(), "goal".into(), vec![])
        .unwrap();
    app.set_tool_policy(&b.id, "github_file_read", 0, PolicyChoice::Block)
        .unwrap();
    app.check_tool_policy(&automatic).unwrap();
    let blocked = app
        .set_tool_policy(
            &a.id,
            "github_file_write",
            automatic.revision,
            PolicyChoice::Block,
        )
        .unwrap();
    assert_eq!(
        app.check_tool_policy(&automatic).unwrap_err().code,
        "POLICY_CHANGED"
    );
    assert_eq!(
        app.check_tool_policy(&blocked).unwrap_err().code,
        "POLICY_BLOCKED"
    );
    assert_eq!(
        app.set_tool_policy(
            &a.id,
            "github_file_write",
            automatic.revision,
            PolicyChoice::Automatic
        )
        .unwrap_err()
        .code,
        "POLICY_CONFLICT"
    );
}
#[test]
fn preferences_can_be_changed_with_disabled_api_and_do_not_enable_connections() {
    let mut app = DesktopService::memory().unwrap();
    let (c, a, _) = conversations(&mut app);
    app.set_connection_enabled(&c.id, false).unwrap();
    app.set_tool_policy(&a.id, "github_file_create", 0, PolicyChoice::Automatic)
        .unwrap();
    assert!(!app.snapshot().unwrap().connections[0].enabled);
    assert!(app
        .set_tool_policy(&a.id, "terminal_anything", 0, PolicyChoice::Automatic)
        .is_err());
    assert!(app
        .set_tool_policy("missing", "github_file_read", 0, PolicyChoice::Automatic)
        .is_err());
}
#[test]
fn schema_five_migration_preserves_payload_and_does_not_invent_auto_authorization() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db.sqlite");
    let mut app = DesktopService::open(&path).unwrap();
    let (_, a, _) = conversations(&mut app);
    drop(app);
    let db = rusqlite::Connection::open(&path).unwrap();
    let body: String = db
        .query_row("SELECT body FROM desktop_state WHERE id=1", [], |r| {
            r.get(0)
        })
        .unwrap();
    let mut old: serde_json::Value = serde_json::from_str(&body).unwrap();
    old.as_object_mut().unwrap().remove("tool_policies");
    let raw = old.to_string();
    db.execute("UPDATE desktop_state SET body=?1 WHERE id=1", [&raw])
        .unwrap();
    db.pragma_update(None, "user_version", 5).unwrap();
    drop(db);
    let app = DesktopService::open(&path).unwrap();
    assert_eq!(
        app.tool_policy(&a.id, "github_file_read").unwrap().policy,
        PolicyChoice::Ask
    );
    drop(app);
    let db = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        6
    );
    assert_eq!(
        db.query_row("SELECT body FROM desktop_state WHERE id=1", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        raw
    );
}
