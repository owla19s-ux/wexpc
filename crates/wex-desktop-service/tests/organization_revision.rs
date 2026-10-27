//! Revision checks must run inside the same transaction as placement/binding writes.
use wex_desktop_service::*;

#[derive(Clone, Copy)]
enum Edit {
    Move,
    Project,
    Conversation,
}

struct Fixture {
    home: Space,
    work: Space,
    first: Context,
    second: Context,
    project: Project,
    conversation: Conversation,
}

fn fixture(app: &mut DesktopService) -> Fixture {
    let home = app.create_space("Home".into()).unwrap();
    let work = app.create_space("Work".into()).unwrap();
    let first = app
        .create_context("First".into(), String::new(), vec![], &home.id, None)
        .unwrap();
    let second = app
        .create_context("Second".into(), String::new(), vec![], &work.id, None)
        .unwrap();
    let project = app
        .create_project("Project".into(), String::new(), vec![])
        .unwrap();
    let api = app
        .add_connection(
            NewConnection {
                name: "API".into(),
                base_url: "https://example.invalid".into(),
                models: vec!["model".into()],
            },
            "vault-reference".into(),
        )
        .unwrap();
    let conversation = app
        .create_conversation(None, "Chat".into(), api.id, "model".into())
        .unwrap();
    Fixture {
        home,
        work,
        first,
        second,
        project,
        conversation,
    }
}

fn apply(
    app: &mut DesktopService,
    fixture: &Fixture,
    edit: Edit,
    revision: u64,
    second: bool,
) -> Result<()> {
    let context = if second {
        &fixture.second.id
    } else {
        &fixture.first.id
    };
    match edit {
        Edit::Move => app.move_context(
            revision,
            &fixture.first.id,
            if second {
                &fixture.work.id
            } else {
                &fixture.home.id
            },
            None,
        ),
        Edit::Project => {
            app.bind_project_context(revision, &fixture.project.id, Some(context.clone()))
        }
        Edit::Conversation => {
            app.bind_conversation_context(revision, &fixture.conversation.id, Some(context.clone()))
        }
    }
}

fn snapshot(app: &DesktopService) -> serde_json::Value {
    serde_json::to_value(app.snapshot().unwrap()).unwrap()
}

#[test]
fn stale_placement_and_bindings_are_rejected_after_write_and_restart() {
    for edit in [Edit::Move, Edit::Project, Edit::Conversation] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("revision.sqlite");
        let mut app = DesktopService::open(&path).unwrap();
        let fixture = fixture(&mut app);
        let stale = app.snapshot().unwrap().revision;
        apply(&mut app, &fixture, edit, stale, true).unwrap();
        let accepted = snapshot(&app);
        assert_eq!(
            apply(&mut app, &fixture, edit, stale, false)
                .unwrap_err()
                .code,
            "REVISION_CONFLICT"
        );
        assert_eq!(snapshot(&app), accepted);
        drop(app);
        let mut app = DesktopService::open(&path).unwrap();
        assert_eq!(snapshot(&app), accepted);
        assert_eq!(
            apply(&mut app, &fixture, edit, stale, false)
                .unwrap_err()
                .code,
            "REVISION_CONFLICT"
        );
        assert_eq!(snapshot(&app), accepted);
        let current = app.snapshot().unwrap().revision;
        apply(&mut app, &fixture, edit, current, false).unwrap();
        assert_eq!(app.snapshot().unwrap().revision, current + 1);
        let after = app.snapshot().unwrap();
        assert_eq!(after.contexts[0].id, fixture.first.id);
        assert!(after.agent_duties.is_empty());
        assert!(after.dispatch_attempts.is_empty());
        assert!(after.tool_policies.is_empty());
    }
}

#[test]
fn invalid_targets_do_not_advance_revision_or_change_associations() {
    let mut app = DesktopService::memory().unwrap();
    let fixture = fixture(&mut app);
    let revision = app.snapshot().unwrap().revision;
    let before = snapshot(&app);
    for result in [
        app.move_context(revision, &fixture.first.id, "missing", None),
        app.bind_project_context(revision, &fixture.project.id, Some("missing".into())),
        app.bind_conversation_context(revision, &fixture.conversation.id, Some("missing".into())),
    ] {
        assert!(result.is_err());
        assert_eq!(snapshot(&app), before);
    }
}

#[test]
fn failed_storage_rolls_back_each_edit_and_same_revision_can_retry() {
    for edit in [Edit::Move, Edit::Project, Edit::Conversation] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("failure.sqlite");
        let mut app = DesktopService::open(&path).unwrap();
        let fixture = fixture(&mut app);
        let revision = app.snapshot().unwrap().revision;
        let before = snapshot(&app);
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch(
            "CREATE TRIGGER fail_update BEFORE UPDATE ON desktop_state
             BEGIN SELECT RAISE(ABORT, 'injected storage failure'); END;",
        )
        .unwrap();
        assert_eq!(
            apply(&mut app, &fixture, edit, revision, true)
                .unwrap_err()
                .code,
            "STORAGE_ERROR"
        );
        assert_eq!(snapshot(&app), before);
        db.execute_batch("DROP TRIGGER fail_update").unwrap();
        apply(&mut app, &fixture, edit, revision, true).unwrap();
        assert_eq!(app.snapshot().unwrap().revision, revision + 1);
    }
}

#[test]
fn clearing_bindings_requires_the_current_revision_and_preserves_sources() {
    let mut app = DesktopService::memory().unwrap();
    let fixture = fixture(&mut app);
    let revision = app.snapshot().unwrap().revision;
    app.bind_project_context(
        revision,
        &fixture.project.id,
        Some(fixture.first.id.clone()),
    )
    .unwrap();
    let revision = app.snapshot().unwrap().revision;
    app.bind_conversation_context(
        revision,
        &fixture.conversation.id,
        Some(fixture.first.id.clone()),
    )
    .unwrap();
    let revision = app.snapshot().unwrap().revision;
    let before = snapshot(&app);
    assert_eq!(
        app.bind_project_context(revision - 1, &fixture.project.id, None)
            .unwrap_err()
            .code,
        "REVISION_CONFLICT"
    );
    assert_eq!(
        app.bind_conversation_context(revision - 1, &fixture.conversation.id, None)
            .unwrap_err()
            .code,
        "REVISION_CONFLICT"
    );
    assert_eq!(snapshot(&app), before);
    app.bind_project_context(revision, &fixture.project.id, None)
        .unwrap();
    let revision = app.snapshot().unwrap().revision;
    app.bind_conversation_context(revision, &fixture.conversation.id, None)
        .unwrap();
    let after = app.snapshot().unwrap();
    assert!(after.projects[0].context_id.is_none());
    assert!(after.conversations[0].context_id.is_none());
    assert_eq!(after.contexts.len(), 2);
    assert_eq!(after.placements.len(), 2);
    assert!(after.conversations[0].project_id.is_none());
}
