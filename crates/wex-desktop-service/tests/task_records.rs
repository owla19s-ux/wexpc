use wex_desktop_service::*;

fn task(app: &mut DesktopService, name: &str) -> Task {
    let p = app
        .create_project(name.into(), "项目目标".into(), vec![])
        .unwrap();
    app.create_task(NewTask {
        project_id: p.id,
        title: name.into(),
        goal: "原目标".into(),
        scope: vec!["src/".into()],
        acceptance: "按实际结果验收".into(),
        agent_ids: vec![],
    })
    .unwrap()
}
fn note(task_id: &str, kind: TaskRecordKind, body: &str) -> NewTaskRecord {
    NewTaskRecord {
        task_id: task_id.into(),
        kind,
        body: body.into(),
        references: vec![],
    }
}

#[test]
fn records_preserve_task_baseline_order_and_references_after_edit_and_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("records.sqlite");
    let mut app = DesktopService::open(&path).unwrap();
    let t = task(&mut app, "WEXPC");
    let mut ids = vec![];
    for (kind, body) in [
        (TaskRecordKind::Progress, "写好接口，尚未运行"),
        (TaskRecordKind::Blocker, "等待 Windows 环境"),
        (TaskRecordKind::Handoff, "后续核对真实测试结果"),
    ] {
        let rev = app.snapshot().unwrap().revision;
        let mut input = note(&t.id, kind, body);
        input.references = vec![
            "commit:unverified-sha".into(),
            "D:\\WEX\\logs\\run.txt".into(),
        ];
        let r = app.append_task_record(rev, input).unwrap();
        assert_eq!(r.recorded_revision, rev + 1);
        assert_eq!(r.kind, kind);
        assert_eq!(r.source, TaskRecordSource::User);
        ids.push(r.id);
    }
    let rev = app.snapshot().unwrap().revision;
    app.update_task(
        rev,
        &t.id,
        NewTask {
            project_id: t.project_id.clone(),
            title: "新任务目标".into(),
            goal: "已调整".into(),
            scope: vec!["tests/".into()],
            acceptance: "新条件".into(),
            agent_ids: vec![],
        },
    )
    .unwrap();
    drop(app);
    let app = DesktopService::open(&path).unwrap();
    let s = app.snapshot().unwrap();
    assert_eq!(s.task_records.len(), 3);
    assert_eq!(
        s.task_records
            .iter()
            .map(|r| r.id.clone())
            .collect::<Vec<_>>(),
        ids
    );
    for r in &s.task_records {
        assert_eq!(r.task_id, t.id);
        assert_eq!(r.task_snapshot.id, t.id);
        assert_eq!(r.task_snapshot.title, "WEXPC");
        assert_eq!(r.task_snapshot.goal, "原目标");
        assert_eq!(r.task_snapshot.scope, vec!["src/"]);
        assert_eq!(r.task_snapshot.acceptance, "按实际结果验收");
        assert_eq!(r.task_snapshot.status, TaskStatus::Draft);
        assert_eq!(
            r.references,
            vec!["commit:unverified-sha", "D:\\WEX\\logs\\run.txt"]
        );
    }
    assert_eq!(s.task_records[1].body, "等待 Windows 环境");
    assert_eq!(s.tasks[0].title, "新任务目标");
    assert_eq!(s.tasks[0].status, TaskStatus::Draft);
    assert!(s.turns.is_empty());
    assert!(
        s.connections.is_empty(),
        "recording needs no API or tool permission"
    );
}

#[test]
fn invalid_records_and_stale_revision_leave_existing_records_and_tasks_untouched() {
    let mut app = DesktopService::memory().unwrap();
    let t = task(&mut app, "任务");
    let rev = app.snapshot().unwrap().revision;
    app.append_task_record(rev, note(&t.id, TaskRecordKind::Progress, "已有记录"))
        .unwrap();
    let rev = rev + 1;
    let before = serde_json::to_value(app.snapshot().unwrap()).unwrap();
    let mut bad_ref = note(&t.id, TaskRecordKind::Handoff, "交接");
    bad_ref.references = vec!["bad\0ref".into()];
    for (revision, input, code) in [
        (
            rev - 1,
            note(&t.id, TaskRecordKind::Blocker, "旧编辑基准"),
            "REVISION_CONFLICT",
        ),
        (
            rev,
            note("missing", TaskRecordKind::Progress, "未知任务"),
            "TASK_NOT_FOUND",
        ),
        (
            rev,
            note(&t.id, TaskRecordKind::Progress, " \n "),
            "INVALID_INPUT",
        ),
        (
            rev,
            note(&t.id, TaskRecordKind::Progress, "bad\0note"),
            "INVALID_INPUT",
        ),
        (rev, bad_ref, "INVALID_INPUT"),
    ] {
        assert_eq!(
            app.append_task_record(revision, input).unwrap_err().code,
            code
        );
        assert_eq!(
            serde_json::to_value(app.snapshot().unwrap()).unwrap(),
            before
        );
    }
}

#[test]
fn schema_three_upgrade_preserves_exact_payload_and_future_version_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.sqlite");
    let mut app = DesktopService::open(&path).unwrap();
    let t = task(&mut app, "旧任务");
    let mut payload = serde_json::to_value(app.snapshot().unwrap()).unwrap();
    payload.as_object_mut().unwrap().remove("task_records");
    let raw = format!("  {}\n", payload);
    drop(app);
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute("UPDATE desktop_state SET body=?1", [&raw])
        .unwrap();
    db.pragma_update(None, "user_version", 3).unwrap();
    drop(db);
    let app = DesktopService::open(&path).unwrap();
    let s = app.snapshot().unwrap();
    assert_eq!(s.tasks[0].id, t.id);
    assert!(s.task_records.is_empty());
    drop(app);
    let db = rusqlite::Connection::open(&path).unwrap();
    let stored: String = db
        .query_row("SELECT body FROM desktop_state WHERE id=1", [], |r| {
            r.get(0)
        })
        .unwrap();
    let version: u32 = db
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(stored, raw);
    assert_eq!(
        version, 4,
        "older binaries must refuse and cannot erase task records"
    );
    db.pragma_update(None, "user_version", 5).unwrap();
    drop(db);
    assert_eq!(
        DesktopService::open(&path).err().unwrap().code,
        "UNSUPPORTED_VERSION"
    );
}

#[test]
fn damaged_old_payload_does_not_advance_schema_or_replace_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.sqlite");
    drop(DesktopService::open(&path).unwrap());
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute("UPDATE desktop_state SET body=?1", ["{invalid}"])
        .unwrap();
    db.pragma_update(None, "user_version", 3).unwrap();
    drop(db);
    assert_eq!(
        DesktopService::open(&path).err().unwrap().code,
        "INVALID_SNAPSHOT"
    );
    let db = rusqlite::Connection::open(&path).unwrap();
    let version: u32 = db
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    let raw: String = db
        .query_row("SELECT body FROM desktop_state WHERE id=1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(version, 3);
    assert_eq!(raw, "{invalid}");
}

#[test]
fn member_prompt_includes_only_current_assignment_records_as_unverified_data() {
    let mut app = DesktopService::memory().unwrap();
    let a = task(&mut app, "PC");
    let b = task(&mut app, "Android");
    let unassigned = app
        .create_task(NewTask {
            project_id: a.project_id.clone(),
            title: "同项目未分配".into(),
            goal: "".into(),
            scope: vec![],
            acceptance: "".into(),
            agent_ids: vec![],
        })
        .unwrap();
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
    let agent = app
        .add_agent(NewAgent {
            project_id: a.project_id.clone(),
            name: "Worker".into(),
            role: AgentRole::Worker,
            connection_id: c.id.clone(),
            model_id: "deepseek-chat".into(),
        })
        .unwrap();
    let mut rev = app.snapshot().unwrap().revision;
    app.update_task(
        rev,
        &a.id,
        NewTask {
            project_id: a.project_id.clone(),
            title: a.title.clone(),
            goal: a.goal.clone(),
            scope: a.scope.clone(),
            acceptance: a.acceptance.clone(),
            agent_ids: vec![agent.id.clone()],
        },
    )
    .unwrap();
    rev += 1;
    app.append_task_record(
        rev,
        note(&a.id, TaskRecordKind::Handoff, "PC_HANDOFF_UNVERIFIED"),
    )
    .unwrap();
    rev += 1;
    app.append_task_record(
        rev,
        note(
            &b.id,
            TaskRecordKind::Progress,
            "OTHER_PROJECT_PRIVATE_NOTE",
        ),
    )
    .unwrap();
    rev = app.snapshot().unwrap().revision;
    app.append_task_record(
        rev,
        note(
            &unassigned.id,
            TaskRecordKind::Progress,
            "SAME_PROJECT_UNASSIGNED_NOTE",
        ),
    )
    .unwrap();
    let req = app
        .prepare_chat(&agent.conversation_id, "继续讨论".into())
        .unwrap();
    let system = req
        .messages
        .iter()
        .filter(|m| m.role == "system")
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(system.contains("PC_HANDOFF_UNVERIFIED"));
    assert!(system.contains("手动记录"));
    assert!(system.contains("未自动核验"));
    assert!(!system.contains("OTHER_PROJECT_PRIVATE_NOTE"));
    assert!(!system.contains("SAME_PROJECT_UNASSIGNED_NOTE"));
    app.finish_chat(&req.turn_id, "仅讨论".into()).unwrap();
    rev = app.snapshot().unwrap().revision;
    app.update_task(
        rev,
        &a.id,
        NewTask {
            project_id: a.project_id,
            title: a.title,
            goal: a.goal,
            scope: a.scope,
            acceptance: a.acceptance,
            agent_ids: vec![],
        },
    )
    .unwrap();
    let req = app
        .prepare_chat(&agent.conversation_id, "重新讨论".into())
        .unwrap();
    assert!(!req
        .messages
        .iter()
        .filter(|m| m.role == "system")
        .any(|m| m.content.contains("PC_HANDOFF_UNVERIFIED")));
    let ordinary = app
        .create_conversation(None, "普通对话".into(), c.id, "deepseek-chat".into())
        .unwrap();
    let req = app.prepare_chat(&ordinary.id, "你好".into()).unwrap();
    assert!(!req
        .messages
        .iter()
        .any(|m| m.content.contains("PC_HANDOFF_UNVERIFIED")));
}

#[test]
fn callers_cannot_spoof_record_source_or_verified_state() {
    for extra in [
        serde_json::json!({"source":"tool"}),
        serde_json::json!({"verified":true}),
        serde_json::json!({"recorded_revision":1}),
        serde_json::json!({"task_snapshot":{}}),
    ] {
        let mut value =
            serde_json::json!({"task_id":"task","kind":"progress","body":"note","references":[]});
        value
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        assert!(serde_json::from_value::<NewTaskRecord>(value).is_err());
    }
}

#[test]
fn failed_storage_write_leaves_no_partial_record_and_can_retry_without_dispatch() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("write-failure.sqlite");
    let mut app = DesktopService::open(&path).unwrap();
    let t = task(&mut app, "任务");
    let before = serde_json::to_value(app.snapshot().unwrap()).unwrap();
    let revision = app.snapshot().unwrap().revision;
    let disk = rusqlite::Connection::open(&path).unwrap();
    disk.execute_batch("CREATE TRIGGER fail_update BEFORE UPDATE ON desktop_state BEGIN SELECT RAISE(ABORT, 'write failed'); END;").unwrap();
    assert_eq!(
        app.append_task_record(
            revision,
            note(&t.id, TaskRecordKind::Blocker, "保留待重试内容")
        )
        .unwrap_err()
        .code,
        "STORAGE_ERROR"
    );
    assert_eq!(
        serde_json::to_value(app.snapshot().unwrap()).unwrap(),
        before
    );
    disk.execute_batch("DROP TRIGGER fail_update;").unwrap();
    app.append_task_record(
        revision,
        note(&t.id, TaskRecordKind::Blocker, "保留待重试内容"),
    )
    .unwrap();
    let s = app.snapshot().unwrap();
    assert_eq!(s.task_records.len(), 1);
    assert_eq!(s.task_records[0].body, "保留待重试内容");
    assert!(s.turns.is_empty());
}

#[test]
fn duplicate_agents_cannot_be_assigned_to_one_task() {
    let mut app = DesktopService::memory().unwrap();
    let project = app.create_project("PC".into(), "".into(), vec![]).unwrap();
    let connection = app.add_connection(NewConnection {
        name: "API".into(),
        base_url: "https://api.deepseek.com".into(),
        models: vec!["deepseek-chat".into()],
    }, "vault".into()).unwrap();
    let agent = app.add_agent(NewAgent {
        project_id: project.id.clone(),
        name: "Worker".into(),
        role: AgentRole::Worker,
        connection_id: connection.id,
        model_id: "deepseek-chat".into(),
    }).unwrap();
    let duplicate = vec![agent.id.clone(), agent.id.clone()];
    let before = app.snapshot().unwrap().revision;
    let result = app.create_task(NewTask {
        project_id: project.id.clone(),
        title: "任务".into(),
        goal: "".into(),
        scope: vec![],
        acceptance: "".into(),
        agent_ids: duplicate.clone(),
    });
    assert_eq!(result.unwrap_err().code, "INVALID_BINDING");
    assert_eq!(app.snapshot().unwrap().revision, before);
    let task = app.create_task(NewTask {
        project_id: project.id.clone(),
        title: "任务".into(),
        goal: "".into(),
        scope: vec![],
        acceptance: "".into(),
        agent_ids: vec![agent.id],
    }).unwrap();
    let revision = app.snapshot().unwrap().revision;
    let result = app.update_task(revision, &task.id, NewTask {
        project_id: project.id,
        title: "新名称".into(),
        goal: "".into(),
        scope: vec![],
        acceptance: "".into(),
        agent_ids: duplicate,
    });
    assert_eq!(result.unwrap_err().code, "INVALID_BINDING");
    let snapshot = app.snapshot().unwrap();
    assert_eq!(snapshot.revision, revision);
    assert_eq!(snapshot.tasks[0].title, "任务");
}

#[test]
fn task_lifecycle_enforces_order_and_revisions() {
    let mut app = DesktopService::memory().unwrap();
    let task = task(&mut app, "Lifecycle");
    let start = app.snapshot().unwrap().revision;
    assert_eq!(app.transition_task(start, &task.id, TaskStatus::Completed).unwrap_err().code,
        "INVALID_TASK_TRANSITION");
    assert_eq!(app.snapshot().unwrap().revision, start);
    app.transition_task(start, &task.id, TaskStatus::Ready).unwrap();
    assert_eq!(app.transition_task(start, &task.id, TaskStatus::InProgress).unwrap_err().code,
        "REVISION_CONFLICT");
    let advance = |app: &mut DesktopService, next| {
        let revision = app.snapshot().unwrap().revision;
        app.transition_task(revision, &task.id, next).unwrap();
    };
    advance(&mut app, TaskStatus::InProgress);
    advance(&mut app, TaskStatus::Blocked);
    advance(&mut app, TaskStatus::InProgress);
    advance(&mut app, TaskStatus::InReview);
    advance(&mut app, TaskStatus::Completed);
    let done = app.snapshot().unwrap();
    assert_eq!(done.tasks[0].status, TaskStatus::Completed);
    assert_eq!(app.transition_task(done.revision, &task.id, TaskStatus::InProgress)
        .unwrap_err().code, "INVALID_TASK_TRANSITION");
    assert_eq!(app.snapshot().unwrap().revision, done.revision);
}

#[test]
fn cancelled_task_cannot_be_restarted_implicitly() {
    let mut app = DesktopService::memory().unwrap();
    let task = task(&mut app, "Cancelled");
    let revision = app.snapshot().unwrap().revision;
    app.transition_task(revision, &task.id, TaskStatus::Cancelled).unwrap();
    let current = app.snapshot().unwrap().revision;
    assert_eq!(app.transition_task(current, &task.id, TaskStatus::Ready).unwrap_err().code,
        "INVALID_TASK_TRANSITION");
    assert_eq!(app.snapshot().unwrap().tasks[0].status, TaskStatus::Cancelled);
}

#[test]
fn closed_tasks_reject_definition_edits() {
    let mut app = DesktopService::memory().unwrap();
    let task = task(&mut app, "Closed");
    let mut step = |status| {
        let rev = app.snapshot().unwrap().revision;
        app.transition_task(rev, &task.id, status).unwrap();
    };
    step(TaskStatus::Ready);
    step(TaskStatus::InProgress);
    step(TaskStatus::InReview);
    step(TaskStatus::Completed);
    let revision = app.snapshot().unwrap().revision;
    let result = app.update_task(revision, &task.id, NewTask {
        project_id: task.project_id,
        title: "Changed".into(),
        goal: "".into(),
        scope: vec![],
        acceptance: "".into(),
        agent_ids: vec![],
    });
    assert_eq!(result.unwrap_err().code, "TASK_CLOSED");
    assert_eq!(app.snapshot().unwrap().revision, revision);
}

#[test]
fn review_status_rejects_task_edits_without_changing_revision() {
    let mut app = DesktopService::memory().unwrap();
    let t = task(&mut app, "Review");
    for next in [TaskStatus::Ready, TaskStatus::InProgress, TaskStatus::InReview] {
        let revision = app.snapshot().unwrap().revision;
        app.transition_task(revision, &t.id, next).unwrap();
    }
    let revision = app.snapshot().unwrap().revision;
    let error = app.update_task(revision, &t.id, NewTask {
        project_id: t.project_id,
        title: "Edited".into(),
        goal: "".into(),
        scope: vec![],
        acceptance: "".into(),
        agent_ids: vec![],
    }).unwrap_err();
    assert_eq!(error.code, "TASK_CLOSED");
    assert_eq!(app.snapshot().unwrap().revision, revision);
}

#[test]
fn review_acceptance_and_rework_are_explicit_actions() {
    let mut app = DesktopService::memory().unwrap();
    let t = task(&mut app, "Review decision");
    let initial = app.snapshot().unwrap().revision;
    assert_eq!(app.accept_task_review(initial, &t.id).unwrap_err().code,
        "INVALID_TASK_TRANSITION");
    for status in [TaskStatus::Ready, TaskStatus::InProgress, TaskStatus::InReview] {
        let revision = app.snapshot().unwrap().revision;
        app.transition_task(revision, &t.id, status).unwrap();
    }
    let revision = app.snapshot().unwrap().revision;
    app.request_task_rework(revision, &t.id).unwrap();
    assert_eq!(app.snapshot().unwrap().tasks[0].status, TaskStatus::InProgress);
    let revision = app.snapshot().unwrap().revision;
    app.transition_task(revision, &t.id, TaskStatus::InReview).unwrap();
    let revision = app.snapshot().unwrap().revision;
    app.accept_task_review(revision, &t.id).unwrap();
    assert_eq!(app.snapshot().unwrap().tasks[0].status, TaskStatus::Completed);
}

#[test]
fn manual_review_note_does_not_complete_task() {
    let mut app = DesktopService::memory().unwrap();
    let t = task(&mut app, "Review note");
    let revision = app.snapshot().unwrap().revision;
    let note = app.append_review_note(revision, NewReviewNote {
        task_id: t.id.clone(),
        body: "需要复核真实构建".into(),
        references: vec!["commit:unverified".into()],
    }).unwrap();
    assert_eq!(note.source, TaskRecordSource::User);
    assert_eq!(note.kind, TaskRecordKind::Handoff);
    let snapshot = app.snapshot().unwrap();
    assert_eq!(snapshot.tasks[0].status, TaskStatus::Draft);
    assert_eq!(snapshot.task_records.len(), 1);
}

#[test]
fn closed_task_rejects_progress_but_allows_manual_handoff() {
    let mut app = DesktopService::memory().unwrap();
    let t = task(&mut app, "Archive");
    let revision = app.snapshot().unwrap().revision;
    app.transition_task(revision, &t.id, TaskStatus::Cancelled).unwrap();
    let revision = app.snapshot().unwrap().revision;
    let result = app.append_task_record(revision, NewTaskRecord {
        task_id: t.id.clone(),
        kind: TaskRecordKind::Progress,
        body: "continued work".into(),
        references: vec![],
    });
    assert_eq!(result.unwrap_err().code, "TASK_CLOSED");
    assert_eq!(app.snapshot().unwrap().revision, revision);
    let note = app.append_task_record(revision, NewTaskRecord {
        task_id: t.id,
        kind: TaskRecordKind::Handoff,
        body: "archived".into(),
        references: vec![],
    }).unwrap();
    assert_eq!(note.kind, TaskRecordKind::Handoff);
}

#[test]
fn active_and_reviewing_tasks_can_be_cancelled() {
    for reviewing in [false, true] {
        let mut app = DesktopService::memory().unwrap();
        let t = task(&mut app, "Cancel");
        for next in [TaskStatus::Ready, TaskStatus::InProgress] {
            let rev = app.snapshot().unwrap().revision;
            app.transition_task(rev, &t.id, next).unwrap();
        }
        if reviewing {
            let rev = app.snapshot().unwrap().revision;
            app.transition_task(rev, &t.id, TaskStatus::InReview).unwrap();
        }
        let rev = app.snapshot().unwrap().revision;
        app.transition_task(rev, &t.id, TaskStatus::Cancelled).unwrap();
        assert_eq!(app.snapshot().unwrap().tasks[0].status, TaskStatus::Cancelled);
    }
}

#[test]
fn agent_report_requires_assignment_and_preserves_task_state() {
    let mut app = DesktopService::memory().unwrap();
    let t = task(&mut app, "Agent report");
    let c = app.add_connection(NewConnection {
        name: "API".into(),
        base_url: "https://api.deepseek.com".into(),
        models: vec!["deepseek-chat".into()],
    }, "vault".into()).unwrap();
    let agent = app.add_agent(NewAgent {
        project_id: t.project_id.clone(),
        name: "Worker".into(),
        role: AgentRole::Worker,
        connection_id: c.id,
        model_id: "deepseek-chat".into(),
    }).unwrap();
    let rev = app.snapshot().unwrap().revision;
    let input = NewAgentWorkReport {
        task_id: t.id.clone(),
        agent_id: agent.id.clone(),
        body: "unverified progress".into(),
        references: vec![],
    };
    assert_eq!(app.append_agent_work_report(rev, input).unwrap_err().code,
        "INVALID_BINDING");
    assert_eq!(app.snapshot().unwrap().revision, rev);
    app.update_task(rev, &t.id, NewTask {
        project_id: t.project_id.clone(),
        title: t.title.clone(),
        goal: t.goal.clone(),
        scope: t.scope.clone(),
        acceptance: t.acceptance.clone(),
        agent_ids: vec![agent.id.clone()],
    }).unwrap();
    let rev = app.snapshot().unwrap().revision;
    let report = app.append_agent_work_report(rev, NewAgentWorkReport {
        task_id: t.id.clone(),
        agent_id: agent.id.clone(),
        body: "unverified progress".into(),
        references: vec!["commit:unverified".into()],
    }).unwrap();
    assert_eq!(report.recorded_revision, rev + 1);
    let snapshot = app.snapshot().unwrap();
    assert_eq!(snapshot.agent_work_reports.len(), 1);
    assert_eq!(snapshot.tasks[0].status, TaskStatus::Draft);
    assert_eq!(app.member_brief(&agent.id).unwrap().agent_work_reports.len(), 1);
}

#[test]
fn agent_report_survives_restart_without_becoming_a_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("agent-report.sqlite");
    let mut app = DesktopService::open(&path).unwrap();
    let t = task(&mut app, "Persist");
    let c = app.add_connection(NewConnection {
        name: "API".into(),
        base_url: "https://api.deepseek.com".into(),
        models: vec!["deepseek-chat".into()],
    }, "vault".into()).unwrap();
    let agent = app.add_agent(NewAgent {
        project_id: t.project_id.clone(),
        name: "Worker".into(),
        role: AgentRole::Worker,
        connection_id: c.id,
        model_id: "deepseek-chat".into(),
    }).unwrap();
    let rev = app.snapshot().unwrap().revision;
    app.update_task(rev, &t.id, NewTask {
        project_id: t.project_id.clone(),
        title: t.title.clone(),
        goal: t.goal.clone(),
        scope: t.scope.clone(),
        acceptance: t.acceptance.clone(),
        agent_ids: vec![agent.id.clone()],
    }).unwrap();
    let rev = app.snapshot().unwrap().revision;
    let report = app.append_agent_work_report(rev, NewAgentWorkReport {
        task_id: t.id.clone(),
        agent_id: agent.id.clone(),
        body: "self reported".into(),
        references: vec![],
    }).unwrap();
    drop(app);
    let app = DesktopService::open(&path).unwrap();
    let snapshot = app.snapshot().unwrap();
    assert_eq!(snapshot.agent_work_reports[0].id, report.id);
    assert_eq!(snapshot.agent_work_reports[0].task_snapshot.title, "Persist");
    assert_eq!(snapshot.tasks[0].status, TaskStatus::Draft);
    assert_eq!(app.member_brief(&agent.id).unwrap().agent_work_reports.len(), 1);
}

#[test]
fn member_brief_does_not_expose_unrelated_execution_receipts() {
    let mut app = DesktopService::memory().unwrap();
    let a = task(&mut app, "Visible");
    let b = task(&mut app, "Hidden");
    let c = app.add_connection(NewConnection {
        name: "API".into(),
        base_url: "https://api.deepseek.com".into(),
        models: vec!["deepseek-chat".into()],
    }, "vault".into()).unwrap();
    let agent = app.add_agent(NewAgent {
        project_id: a.project_id.clone(),
        name: "Worker".into(),
        role: AgentRole::Worker,
        connection_id: c.id,
        model_id: "deepseek-chat".into(),
    }).unwrap();
    let rev = app.snapshot().unwrap().revision;
    app.update_task(rev, &a.id, NewTask {
        project_id: a.project_id.clone(),
        title: a.title.clone(),
        goal: a.goal.clone(),
        scope: a.scope.clone(),
        acceptance: a.acceptance.clone(),
        agent_ids: vec![agent.id.clone()],
    }).unwrap();
    assert!(app.member_brief(&agent.id).unwrap().execution_receipts.is_empty());
    assert!(app.member_brief(&agent.id).unwrap().tasks.iter().all(|t| t.id != b.id));
}

#[test]
fn schema_four_upgrades_to_five_without_losing_existing_records() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("schema-four.sqlite");
    let mut app = DesktopService::open(&path).unwrap();
    let t = task(&mut app, "Upgrade");
    let rev = app.snapshot().unwrap().revision;
    app.append_task_record(rev, note(&t.id, TaskRecordKind::Handoff, "kept"))
        .unwrap();
    drop(app);
    let db = rusqlite::Connection::open(&path).unwrap();
    db.pragma_update(None, "user_version", 4).unwrap();
    drop(db);
    let app = DesktopService::open(&path).unwrap();
    assert_eq!(app.snapshot().unwrap().task_records[0].body, "kept");
    let db = rusqlite::Connection::open(&path).unwrap();
    let version: u32 = db.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(version, 5);
}
