use wex_desktop_service::*;

#[test]
fn context_can_be_ungrouped_and_move_without_changing_identity_or_resources() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("desktop.sqlite");
    let mut app = DesktopService::open(&path).unwrap();
    let personal = app.create_space("个人".into()).unwrap();
    let work = app.create_space("工作".into()).unwrap();
    let group = app.create_group(&work.id, "WEX".into()).unwrap();
    let c = app
        .create_context(
            "设计讨论".into(),
            "保留原始目标".into(),
            vec!["D:\\资料".into()],
            &personal.id,
            None,
        )
        .unwrap();
    let project = app
        .create_project("WEXPC".into(), "开发".into(), vec![])
        .unwrap();
    app.bind_project_context(&project.id, Some(c.id.clone()))
        .unwrap();
    app.move_context(&c.id, &work.id, Some(group.id.clone()))
        .unwrap();
    app.move_context(&c.id, &work.id, None).unwrap();
    drop(app);
    let app = DesktopService::open(&path).unwrap();
    let s = app.snapshot().unwrap();
    assert_eq!(s.contexts.len(), 1);
    assert_eq!(s.contexts[0].id, c.id);
    assert_eq!(s.contexts[0].resources, vec!["D:\\资料"]);
    assert_eq!(s.contexts[0].goal, "保留原始目标");
    assert_eq!(s.placements[0].space_id, work.id);
    assert!(s.placements[0].group_id.is_none());
    assert_eq!(s.projects[0].context_id.as_deref(), Some(c.id.as_str()));
    assert!(s.agents.is_empty());
    assert!(s.turns.is_empty());
}

#[test]
fn invalid_group_move_rolls_back_and_context_is_not_required_to_be_a_project() {
    let mut app = DesktopService::memory().unwrap();
    let a = app.create_space("资料".into()).unwrap();
    let b = app.create_space("公共".into()).unwrap();
    let g = app.create_group(&b.id, "讨论".into()).unwrap();
    let c = app
        .create_context("小说构思".into(), "".into(), vec![], &a.id, None)
        .unwrap();
    let before = app.snapshot().unwrap();
    assert_eq!(
        app.move_context(&c.id, &a.id, Some(g.id)).unwrap_err().code,
        "INVALID_PLACEMENT"
    );
    let after = app.snapshot().unwrap();
    assert_eq!(before.revision, after.revision);
    assert_eq!(after.placements[0].space_id, a.id);
    assert!(after.projects.is_empty());
    assert!(app.bind_project_context("missing", Some(c.id)).is_err());
}

#[test]
fn pre_organization_snapshot_is_upgraded_without_inventing_containers_or_links() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("desktop.sqlite");
    let mut app = DesktopService::open(&path).unwrap();
    let p = app
        .create_project("旧项目".into(), "原目标".into(), vec![])
        .unwrap();
    drop(app);
    let disk = rusqlite::Connection::open(&path).unwrap();
    let raw: String = disk
        .query_row("SELECT body FROM desktop_state WHERE id=1", [], |r| {
            r.get(0)
        })
        .unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    for key in ["spaces", "groups", "contexts", "placements"] {
        value.as_object_mut().unwrap().remove(key);
    }
    value["projects"][0]
        .as_object_mut()
        .unwrap()
        .remove("context_id");
    disk.execute("UPDATE desktop_state SET body=?1", [value.to_string()])
        .unwrap();
    disk.pragma_update(None, "user_version", 1).unwrap();
    drop(disk);
    let app = DesktopService::open(&path).unwrap();
    let s = app.snapshot().unwrap();
    assert_eq!(s.projects[0].id, p.id);
    assert!(s.projects[0].context_id.is_none());
    assert!(s.spaces.is_empty());
    assert!(s.contexts.is_empty());
    let disk = rusqlite::Connection::open(&path).unwrap();
    let version: u32 = disk
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        version, 2,
        "previous binaries must refuse organization snapshots"
    );
}
