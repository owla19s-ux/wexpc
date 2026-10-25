use wex_desktop_service::*;

fn connection(app: &mut DesktopService) -> AiConnection {
    app.add_connection(
        NewConnection {
            name: "DS".into(),
            base_url: "https://api.deepseek.com".into(),
            models: vec!["deepseek-chat".into()],
        },
        "PRIVATE_VAULT_REFERENCE".into(),
    )
    .unwrap()
}
fn member(
    app: &mut DesktopService,
    project_id: &str,
    connection_id: &str,
    role: AgentRole,
    name: &str,
) -> Agent {
    app.add_agent(NewAgent {
        project_id: project_id.into(),
        name: name.into(),
        role,
        connection_id: connection_id.into(),
        model_id: "deepseek-chat".into(),
    })
    .unwrap()
}
fn task(app: &mut DesktopService, project_id: &str, name: &str, agent_ids: Vec<String>) -> Task {
    let t = app
        .create_task(NewTask {
            project_id: project_id.into(),
            title: name.into(),
            goal: "任务目标".into(),
            scope: vec!["src/".into()],
            acceptance: "按真实结果验收".into(),
            agent_ids,
        })
        .unwrap();
    let rev = app.snapshot().unwrap().revision;
    app.append_task_record(
        rev,
        NewTaskRecord {
            task_id: t.id.clone(),
            kind: TaskRecordKind::Handoff,
            body: format!("{name}_NOTE"),
            references: vec!["commit:unverified".into()],
        },
    )
    .unwrap();
    t
}

#[test]
fn lead_sees_its_project_team_unassigned_tasks_and_notes_without_other_projects_or_history() {
    let mut app = DesktopService::memory().unwrap();
    let c = connection(&mut app);
    let p = app
        .create_project("PC".into(), "完整目标".into(), vec![])
        .unwrap();
    let other = app
        .create_project("OTHER_PROJECT_SENTINEL".into(), "".into(), vec![])
        .unwrap();
    let lead = member(&mut app, &p.id, &c.id, AgentRole::Lead, "Lead");
    let worker = member(&mut app, &p.id, &c.id, AgentRole::Worker, "Worker");
    let outsider = member(
        &mut app,
        &other.id,
        &c.id,
        AgentRole::Reviewer,
        "OTHER_MEMBER_SENTINEL",
    );
    let a = task(
        &mut app,
        &p.id,
        "ASSIGNED_TO_WORKER",
        vec![worker.id.clone()],
    );
    let b = task(&mut app, &p.id, "UNASSIGNED", vec![]);
    task(
        &mut app,
        &other.id,
        "OTHER_TASK_SENTINEL",
        vec![outsider.id],
    );
    let turn = app
        .prepare_chat(&worker.conversation_id, "PRIVATE_WORKER_HISTORY".into())
        .unwrap();
    app.finish_chat(&turn.turn_id, "PRIVATE_WORKER_ANSWER".into())
        .unwrap();
    let brief = app.member_brief(&lead.id).unwrap();
    assert_eq!(brief.scope, MemberBriefScope::Project);
    assert_eq!(brief.member.id, lead.id);
    assert_eq!(
        brief
            .members
            .iter()
            .map(|a| a.id.clone())
            .collect::<Vec<_>>(),
        vec![lead.id, worker.id]
    );
    assert_eq!(
        brief.tasks.iter().map(|t| t.id.clone()).collect::<Vec<_>>(),
        vec![a.id, b.id]
    );
    assert_eq!(brief.task_records.len(), 2);
    let json = serde_json::to_string(&brief).unwrap();
    for forbidden in [
        "OTHER_PROJECT_SENTINEL",
        "OTHER_MEMBER_SENTINEL",
        "OTHER_TASK_SENTINEL",
        "PRIVATE_WORKER_HISTORY",
        "PRIVATE_WORKER_ANSWER",
        "PRIVATE_VAULT_REFERENCE",
        "credential_ref",
        "base_url",
    ] {
        assert!(!json.contains(forbidden), "leaked {forbidden}");
    }
    assert!(json.contains("UNASSIGNED_NOTE"));
}

#[test]
fn worker_verifier_reviewer_only_get_current_assignments_and_self_member_metadata() {
    let mut app = DesktopService::memory().unwrap();
    let c = connection(&mut app);
    let p = app.create_project("PC".into(), "".into(), vec![]).unwrap();
    let mut agents = vec![];
    for (role, name) in [
        (AgentRole::Worker, "Worker"),
        (AgentRole::Verifier, "Verifier"),
        (AgentRole::Reviewer, "Reviewer"),
    ] {
        agents.push(member(&mut app, &p.id, &c.id, role, name));
    }
    let assigned = task(
        &mut app,
        &p.id,
        "COMMON_ASSIGNED",
        agents.iter().map(|a| a.id.clone()).collect(),
    );
    task(&mut app, &p.id, "UNASSIGNED_SHOULD_NOT_APPEAR", vec![]);
    for a in agents {
        let brief = app.member_brief(&a.id).unwrap();
        assert_eq!(brief.scope, MemberBriefScope::AssignedTasks);
        assert_eq!(brief.members.len(), 1);
        assert_eq!(brief.members[0].id, a.id);
        assert_eq!(brief.tasks.len(), 1);
        assert_eq!(brief.tasks[0].id, assigned.id);
        assert_eq!(brief.task_records.len(), 1);
        assert!(!serde_json::to_string(&brief)
            .unwrap()
            .contains("UNASSIGNED_SHOULD_NOT_APPEAR"));
    }
}

#[test]
fn preview_needs_no_enabled_api_and_does_not_write_or_dispatch() {
    let mut app = DesktopService::memory().unwrap();
    let c = connection(&mut app);
    let p = app.create_project("PC".into(), "".into(), vec![]).unwrap();
    let a = member(&mut app, &p.id, &c.id, AgentRole::Lead, "Lead");
    app.set_connection_enabled(&c.id, false).unwrap();
    let before = serde_json::to_value(app.snapshot().unwrap()).unwrap();
    let brief = app.member_brief(&a.id).unwrap();
    assert_eq!(brief.revision, app.snapshot().unwrap().revision);
    assert!(brief.tasks.is_empty());
    assert_eq!(
        app.member_brief("missing").unwrap_err().code,
        "AGENT_NOT_FOUND"
    );
    assert_eq!(
        serde_json::to_value(app.snapshot().unwrap()).unwrap(),
        before
    );
    assert!(app
        .prepare_chat(&a.conversation_id, "hello".into())
        .is_err());
}

#[test]
fn real_request_uses_same_brief_as_preview_and_updates_after_assignment_changes() {
    let mut app = DesktopService::memory().unwrap();
    let c = connection(&mut app);
    let p = app.create_project("PC".into(), "".into(), vec![]).unwrap();
    let lead = member(&mut app, &p.id, &c.id, AgentRole::Lead, "Lead");
    let worker = member(&mut app, &p.id, &c.id, AgentRole::Worker, "Worker");
    let t = task(&mut app, &p.id, "UNASSIGNED", vec![]);
    let preview = app.member_brief(&lead.id).unwrap();
    let request = app
        .prepare_chat(&lead.conversation_id, "梳理计划".into())
        .unwrap();
    let expected = serde_json::to_string(&preview).unwrap();
    let content = &request.messages[0].content;
    assert!(content.ends_with(&expected));
    assert!(content.contains("手动记录"));
    assert!(content.contains("未自动核验"));
    assert!(content.contains("自动派工"));
    assert!(content.contains("自动派工、持久权限策略及独立审查验证流程尚未形成"));
    assert!(content.contains("不代表已授权或已读取文件"));
    assert!(!content.contains("PRIVATE_VAULT_REFERENCE"));
    let before = app.member_brief(&worker.id).unwrap();
    assert!(before.tasks.is_empty());
    let rev = app.snapshot().unwrap().revision;
    app.update_task(
        rev,
        &t.id,
        NewTask {
            project_id: p.id,
            title: t.title,
            goal: t.goal,
            scope: t.scope,
            acceptance: t.acceptance,
            agent_ids: vec![worker.id.clone()],
        },
    )
    .unwrap();
    let preview = app.member_brief(&worker.id).unwrap();
    assert_eq!(preview.tasks.len(), 1);
    let request = app
        .prepare_chat(&worker.conversation_id, "核对任务".into())
        .unwrap();
    assert!(request.messages[0]
        .content
        .ends_with(&serde_json::to_string(&preview).unwrap()));
}
