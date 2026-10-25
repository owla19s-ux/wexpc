use wex_desktop_service::*;

fn api(app: &mut DesktopService) -> AiConnection {
    app.add_connection(
        NewConnection {
            name: "Test API".into(),
            base_url: "https://api.deepseek.com".into(),
            models: vec!["deepseek-chat".into()],
        },
        "vault_test".into(),
    ).unwrap()
}

#[test]
fn one_member_can_be_secretary_and_project_lead_in_two_projects() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("desktop.sqlite");
    let mut app = DesktopService::open(&file).unwrap();
    let provider = api(&mut app);
    let first = app.create_project("PC".into(), "Build PC".into(), vec![]).unwrap();
    let second = app.create_project("Android".into(), "Build Android".into(), vec![]).unwrap();
    let member = app.add_agent(NewAgent {
        project_id: first.id.clone(),
        name: "Assistant".into(),
        role: AgentRole::Lead,
        connection_id: provider.id,
        model_id: "deepseek-chat".into(),
    }).unwrap();
    let secretary = app.assign_agent_duty(&member.id, None, "secretary".into()).unwrap();
    let second_lead = app.assign_agent_duty(&member.id, Some(second.id.clone()), "lead".into()).unwrap();
    assert_eq!(secretary.agent_id, member.id);
    assert!(secretary.project_id.is_none());
    assert_eq!(second_lead.project_id.as_deref(), Some(second.id.as_str()));
    assert_eq!(app.snapshot().unwrap().agent_duties.len(), 3);
    assert_eq!(app.assign_agent_duty(&member.id, None, "secretary".into()).unwrap_err().code, "DUPLICATE_ASSIGNMENT");
    drop(app);

    let restored = DesktopService::open(&file).unwrap().snapshot().unwrap();
    assert_eq!(restored.agent_duties.len(), 3);
    assert_eq!(restored.agents.len(), 1, "member identity must not be duplicated");
    assert_eq!(restored.agents[0].conversation_id, member.conversation_id);
}

#[test]
fn assignments_reject_unknown_member_project_and_bad_duty_without_writes() {
    let mut app = DesktopService::memory().unwrap();
    let before = app.snapshot().unwrap().revision;
    assert_eq!(app.assign_agent_duty("missing", None, "secretary".into()).unwrap_err().code, "AGENT_NOT_FOUND");
    assert_eq!(app.assign_agent_duty("missing", None, "bad duty".into()).unwrap_err().code, "INVALID_INPUT");
    assert_eq!(app.snapshot().unwrap().revision, before);
    assert!(app.snapshot().unwrap().agent_duties.is_empty());
}

#[test]
fn legacy_snapshot_missing_duties_restores_without_inventing_new_grants() {
    let old = serde_json::json!({
        "revision": 1,
        "connections": [],
        "projects": [],
        "agents": [],
        "tasks": [],
        "conversations": [],
        "turns": []
    });
    let snapshot: DesktopSnapshot = serde_json::from_value(old).unwrap();
    assert!(snapshot.agent_duties.is_empty());
}
