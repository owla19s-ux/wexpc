#![cfg(unix)]

use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use wex_workspace_store::{Change, OrganizationError, OrganizationStore, OrganizationView};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Sandbox(PathBuf);

impl Sandbox {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "wex-workspace-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&directory).unwrap();
        Self(directory)
    }

    fn root(&self) -> PathBuf {
        self.0.join("workspace")
    }

    fn store(&self) -> OrganizationStore {
        OrganizationStore::open(self.root()).unwrap()
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn create_space(store: &OrganizationStore, name: &str) -> OrganizationView {
    let before = store.load().unwrap();
    store
        .apply(before.revision, Change::CreateSpace { name: name.into() })
        .unwrap()
}

fn create_group(
    store: &OrganizationStore,
    revision: u64,
    space_id: &str,
    name: &str,
) -> OrganizationView {
    store
        .apply(
            revision,
            Change::CreateGroup {
                space_id: space_id.into(),
                name: name.into(),
            },
        )
        .unwrap()
}

#[test]
fn space_group_context_reloads_with_stable_identity_and_move() {
    let sandbox = Sandbox::new();
    let store = sandbox.store();
    assert_eq!(store.load().unwrap(), OrganizationView::default());
    let one = create_space(&store, "工作空间");
    let space_id = one.spaces[0].id.clone();
    assert_eq!(space_id, "0000000000000001");
    let two = create_group(&store, one.revision, &space_id, "项目");
    let group_a = two.groups[0].id.clone();
    let three = create_group(&store, two.revision, &space_id, "资料");
    let group_b = three.groups[1].id.clone();
    let four = store
        .apply(
            three.revision,
            Change::CreateContext {
                space_id: space_id.clone(),
                group_id: Some(group_a.clone()),
                name: "设计与施工".into(),
            },
        )
        .unwrap();
    let context_id = four.contexts[0].id.clone();
    let five = store
        .apply(
            four.revision,
            Change::MoveContext {
                id: context_id.clone(),
                space_id: space_id.clone(),
                group_id: Some(group_b.clone()),
            },
        )
        .unwrap();
    assert_eq!(five.contexts[0].id, context_id);
    assert_eq!(five.contexts[0].group_id, Some(group_b));
    assert_eq!(five.revision, 5);
    drop(store);
    let reopened = sandbox.store();
    assert_eq!(reopened.load().unwrap(), five);
    assert_eq!(
        fs::metadata(sandbox.root()).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(sandbox.root().join("workspace.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn renames_and_sibling_duplicates_are_scoped_to_the_right_parent() {
    let sandbox = Sandbox::new();
    let store = sandbox.store();
    let one = create_space(&store, "甲");
    let two = create_space(&store, "乙");
    let a = one.spaces[0].id.clone();
    let b = two.spaces[1].id.clone();
    let first = create_group(&store, two.revision, &a, "文档");
    let second = create_group(&store, first.revision, &b, "文档");
    let duplicate = store.apply(
        second.revision,
        Change::CreateGroup {
            space_id: a.clone(),
            name: "文档".into(),
        },
    );
    assert_eq!(duplicate, Err(OrganizationError::DuplicateName));
    assert_eq!(store.load().unwrap(), second);
    let group_a = first.groups[0].id.clone();
    let group_b = second.groups[1].id.clone();
    let third = store
        .apply(
            second.revision,
            Change::RenameSpace {
                id: a,
                name: "新甲".into(),
            },
        )
        .unwrap();
    assert_eq!(
        store.apply(
            third.revision,
            Change::RenameSpace {
                id: b,
                name: "新甲".into(),
            }
        ),
        Err(OrganizationError::DuplicateName)
    );
    let fourth = store
        .apply(
            third.revision,
            Change::CreateContext {
                space_id: first.groups[0].space_id.clone(),
                group_id: Some(group_a.clone()),
                name: "工作".into(),
            },
        )
        .unwrap();
    let fifth = store
        .apply(
            fourth.revision,
            Change::CreateContext {
                space_id: second.groups[1].space_id.clone(),
                group_id: Some(group_b.clone()),
                name: "工作".into(),
            },
        )
        .unwrap();
    assert_eq!(
        store.apply(
            fifth.revision,
            Change::MoveContext {
                id: fourth.contexts[0].id.clone(),
                space_id: second.groups[1].space_id.clone(),
                group_id: Some(group_b),
            }
        ),
        Err(OrganizationError::DuplicateName)
    );
    assert_eq!(store.load().unwrap(), fifth);
}

#[test]
fn deleting_empty_group_or_space_never_cascades_to_context() {
    let sandbox = Sandbox::new();
    let store = sandbox.store();
    let one = create_space(&store, "我的空间");
    let space_id = one.spaces[0].id.clone();
    let two = create_group(&store, one.revision, &space_id, "分组");
    let group_id = two.groups[0].id.clone();
    assert_eq!(
        store.apply(
            two.revision,
            Change::DeleteEmptySpace {
                id: space_id.clone()
            }
        ),
        Err(OrganizationError::NotEmpty)
    );
    let three = store
        .apply(
            two.revision,
            Change::CreateContext {
                space_id: space_id.clone(),
                group_id: Some(group_id.clone()),
                name: "上下文".into(),
            },
        )
        .unwrap();
    assert_eq!(
        store.apply(
            three.revision,
            Change::DeleteEmptyGroup {
                id: group_id.clone()
            }
        ),
        Err(OrganizationError::NotEmpty)
    );
    assert_eq!(store.load().unwrap().contexts.len(), 1);
    assert_eq!(
        store.apply(
            three.revision,
            Change::DeleteEmptyGroup {
                id: "0000000000000123".into()
            }
        ),
        Err(OrganizationError::NotFound)
    );
}

#[test]
fn two_store_handles_use_revision_cas_and_do_not_overwrite_each_other() {
    let sandbox = Sandbox::new();
    let first = sandbox.store();
    let second = sandbox.store();
    let initial = first.load().unwrap();
    assert_eq!(initial.revision, 0);
    create_space(&first, "新的空间");
    assert_eq!(
        second.apply(
            initial.revision,
            Change::CreateSpace {
                name: "过时".into()
            }
        ),
        Err(OrganizationError::Conflict)
    );
    let snapshot = second.load().unwrap();
    assert_eq!(snapshot.spaces.len(), 1);
    assert_eq!(snapshot.spaces[0].name, "新的空间");
}

#[test]
fn invalid_names_id_and_missing_parents_do_not_touch_snapshot() {
    let sandbox = Sandbox::new();
    let store = sandbox.store();
    let zero = store.load().unwrap();
    for name in ["", " 首尾空格", "换\n行", "x\u{0000}y"] {
        assert_eq!(
            store.apply(0, Change::CreateSpace { name: name.into() }),
            Err(OrganizationError::InvalidInput)
        );
    }
    assert_eq!(
        store.apply(
            0,
            Change::CreateSpace {
                name: "好".repeat(65)
            }
        ),
        Err(OrganizationError::InvalidInput)
    );
    assert_eq!(
        store.apply(
            0,
            Change::CreateGroup {
                space_id: "00000000000000ab".into(),
                name: "孤儿".into(),
            }
        ),
        Err(OrganizationError::NotFound)
    );
    assert_eq!(
        store.apply(
            0,
            Change::RenameContext {
                id: "../context".into(),
                name: "无效".into(),
            }
        ),
        Err(OrganizationError::InvalidInput)
    );
    assert_eq!(store.load().unwrap(), zero);
    assert!(!sandbox.root().join("workspace.json").exists());
}

#[test]
fn malformed_revision_schema_or_ghost_parent_fail_closed_on_reopen() {
    let sandbox = Sandbox::new();
    let store = sandbox.store();
    let one = create_space(&store, "初始空间");
    let path = sandbox.root().join("workspace.json");
    let original = fs::read(&path).unwrap();
    let cases: Vec<Vec<u8>> = vec![
        b"{".to_vec(),
        original.repeat(10_000),
        serde_json::to_vec(&serde_json::json!({
            "schema_version":3, "revision":1, "next_id":2,
            "spaces":[{"id":"0000000000000001","name":"初始空间"}],
            "groups":[], "contexts":[]
        }))
        .unwrap(),
        serde_json::to_vec(&serde_json::json!({
            "schema_version":1, "revision":1, "next_id":2,
            "spaces":[{"id":"0000000000000001","name":"初始空间"}],
            "groups":[{"id":"0000000000000002","space_id":"0000000000000099","name":"孤儿"}],
            "contexts":[]
        }))
        .unwrap(),
        serde_json::to_vec(&serde_json::json!({
            "schema_version":1, "revision":1, "next_id":1,
            "spaces":[{"id":"0000000000000001","name":"初始空间"}],
            "groups":[], "contexts":[]
        }))
        .unwrap(),
    ];
    for corrupted in cases {
        fs::write(&path, corrupted).unwrap();
        assert_eq!(store.load(), Err(OrganizationError::InvalidSnapshot));
        assert_eq!(
            store.apply(
                1,
                Change::RenameSpace {
                    id: one.spaces[0].id.clone(),
                    name: "尝试修复".into()
                }
            ),
            Err(OrganizationError::InvalidSnapshot)
        );
    }
    fs::write(path, original).unwrap();
    assert_eq!(store.load().unwrap(), one);
}

#[test]
fn orphan_tmp_lock_symlink_and_world_readable_root_reject() {
    let sandbox = Sandbox::new();
    let store = sandbox.store();
    let root = sandbox.root();
    fs::write(root.join("workspace.lock"), b"").unwrap();
    assert_eq!(store.load(), Err(OrganizationError::Busy));
    fs::remove_file(root.join("workspace.lock")).unwrap();
    fs::write(root.join("workspace.tmp"), b"orphan").unwrap();
    assert_eq!(store.load(), Err(OrganizationError::Unavailable));
    fs::remove_file(root.join("workspace.tmp")).unwrap();
    let mut permissions = fs::metadata(&root).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&root, permissions).unwrap();
    assert_eq!(store.load(), Err(OrganizationError::Unavailable));
    let alias = sandbox.0.join("alias");
    symlink(&root, &alias).unwrap();
    assert!(matches!(
        OrganizationStore::open(alias),
        Err(OrganizationError::Unavailable)
    ));
}

#[test]
fn revision_overflow_and_quantity_limit_never_wrap_or_reuse_id() {
    let sandbox = Sandbox::new();
    let store = sandbox.store();
    let initial = create_space(&store, "基础");
    let path = sandbox.root().join("workspace.json");
    let mut document: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    document["revision"] = serde_json::json!(u64::MAX);
    fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
    let old = store.load().unwrap();
    assert_eq!(old.revision, u64::MAX);
    assert_eq!(
        store.apply(
            u64::MAX,
            Change::RenameSpace {
                id: initial.spaces[0].id.clone(),
                name: "新标题".into(),
            }
        ),
        Err(OrganizationError::RevisionExhausted)
    );
    assert_eq!(store.load().unwrap(), old);
    document["revision"] = serde_json::json!(1);
    fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
    let mut snapshot = store.load().unwrap();
    for n in 2..=64 {
        snapshot = store
            .apply(
                snapshot.revision,
                Change::CreateSpace {
                    name: format!("空间-{n}"),
                },
            )
            .unwrap();
    }
    assert_eq!(snapshot.spaces.len(), 64);
    assert_eq!(
        store.apply(
            snapshot.revision,
            Change::CreateSpace {
                name: "超出容量".into(),
            }
        ),
        Err(OrganizationError::LimitExceeded)
    );
    assert_eq!(store.load().unwrap(), snapshot);
}

#[test]
fn ungrouped_context_may_move_across_containers_without_changing_identity() {
    let sandbox = Sandbox::new();
    let store = sandbox.store();
    let personal = create_space(&store, "个人");
    let personal_id = personal.spaces[0].id.clone();
    let ctx = store
        .apply(
            personal.revision,
            Change::CreateContext {
                space_id: personal_id.clone(),
                group_id: None,
                name: "私人草稿".into(),
            },
        )
        .unwrap();
    let stable_id = ctx.contexts[0].id.clone();
    assert_eq!(store.contexts_in_space(&personal_id).unwrap().len(), 1);
    let work = create_space(&store, "工作");
    let work_id = work.spaces[1].id.clone();
    let moved = store
        .apply(
            work.revision,
            Change::MoveContext {
                id: stable_id.clone(),
                space_id: work_id.clone(),
                group_id: None,
            },
        )
        .unwrap();
    assert_eq!(moved.contexts[0].id, stable_id);
    assert_eq!(moved.contexts[0].space_id, work_id);
    assert_eq!(moved.contexts[0].group_id, None);
    assert!(store.contexts_in_space(&personal_id).unwrap().is_empty());
    assert_eq!(store.contexts_in_space(&work_id).unwrap().len(), 1);
    assert_eq!(sandbox.store().load().unwrap(), moved);

    let group = create_group(&store, moved.revision, &work_id, "开发");
    let group_id = group.groups[0].id.clone();
    let grouped = store
        .apply(
            group.revision,
            Change::MoveContext {
                id: stable_id.clone(),
                space_id: work_id.clone(),
                group_id: Some(group_id.clone()),
            },
        )
        .unwrap();
    assert_eq!(grouped.contexts[0].group_id, Some(group_id.clone()));
    let ungrouped = store
        .apply(
            grouped.revision,
            Change::MoveContext {
                id: stable_id.clone(),
                space_id: work_id.clone(),
                group_id: None,
            },
        )
        .unwrap();
    let without_group = store
        .apply(
            ungrouped.revision,
            Change::DeleteEmptyGroup { id: group_id },
        )
        .unwrap();
    assert_eq!(without_group.contexts[0].id, stable_id);
    assert!(without_group.groups.is_empty());
    assert_eq!(
        store.apply(
            without_group.revision,
            Change::DeleteEmptySpace { id: work_id }
        ),
        Err(OrganizationError::NotEmpty)
    );
}

#[test]
fn cross_container_group_references_fail_without_mutating_snapshot() {
    let sandbox = Sandbox::new();
    let store = sandbox.store();
    let a = create_space(&store, "A");
    let b = create_space(&store, "B");
    let a_id = a.spaces[0].id.clone();
    let b_id = b.spaces[1].id.clone();
    let group = create_group(&store, b.revision, &a_id, "A组");
    let g_id = group.groups[0].id.clone();
    assert_eq!(
        store.apply(
            group.revision,
            Change::CreateContext {
                space_id: b_id.clone(),
                group_id: Some(g_id.clone()),
                name: "越界".into()
            }
        ),
        Err(OrganizationError::InvalidInput)
    );
    let context = store
        .apply(
            group.revision,
            Change::CreateContext {
                space_id: b_id.clone(),
                group_id: None,
                name: "未分组".into(),
            },
        )
        .unwrap();
    let before = context.clone();
    assert_eq!(
        store.apply(
            before.revision,
            Change::MoveContext {
                id: before.contexts[0].id.clone(),
                space_id: b_id.clone(),
                group_id: Some(g_id),
            }
        ),
        Err(OrganizationError::InvalidInput)
    );
    assert_eq!(store.load().unwrap(), before);
    assert_eq!(
        store.contexts_in_space("../unsafe"),
        Err(OrganizationError::InvalidInput)
    );
}

#[test]
fn public_is_only_an_ordinary_local_name_and_creates_no_shares_or_grants() {
    let sandbox = Sandbox::new();
    let store = sandbox.store();
    let public = create_space(&store, "公共");
    let id = public.spaces[0].id.clone();
    let result = store
        .apply(
            public.revision,
            Change::CreateContext {
                space_id: id,
                group_id: None,
                name: "待讨论".into(),
            },
        )
        .unwrap();
    assert_eq!(result.spaces[0].name, "公共");
    assert_eq!(result.contexts[0].group_id, None);
    let raw = fs::read_to_string(sandbox.root().join("workspace.json")).unwrap();
    assert!(!raw.contains("share"));
    assert!(!raw.contains("grant"));
    assert!(!raw.contains("credential"));
    assert!(!raw.contains("permission"));
    assert!(!raw.contains("principal"));
}

fn write_legacy(sandbox: &Sandbox, contents: &[u8]) {
    // The store must already have created the private root.
    let path = sandbox.root().join("workspace.json");
    fs::write(&path, contents).unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o600);
    fs::set_permissions(&path, permissions).unwrap();
}

fn legacy_fixture() -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "schema_version": 1,
        "revision": 7,
        "next_id": 4,
        "spaces": [{"id":"0000000000000001","name":"旧空间"}],
        "groups": [{"id":"0000000000000002","space_id":"0000000000000001","name":"旧组"}],
        "contexts": [{"id":"0000000000000003","group_id":"0000000000000002","name":"旧内容"}]
    }))
    .unwrap()
}

#[test]
fn legacy_schema_requires_explicit_cas_migration_and_preserves_bytes() {
    let sandbox = Sandbox::new();
    let store = sandbox.store();
    let original = legacy_fixture();
    write_legacy(&sandbox, &original);
    let preview = store.legacy_summary().unwrap().unwrap();
    assert_eq!(
        (
            preview.revision,
            preview.spaces,
            preview.groups,
            preview.contexts
        ),
        (7, 1, 1, 1)
    );
    assert_eq!(store.load(), Err(OrganizationError::MigrationRequired));
    assert_eq!(
        store.apply(
            7,
            Change::CreateSpace {
                name: "工作".into()
            }
        ),
        Err(OrganizationError::MigrationRequired)
    );
    assert_eq!(store.migrate_v1(6), Err(OrganizationError::Conflict));
    assert_eq!(
        fs::read(sandbox.root().join("workspace.json")).unwrap(),
        original
    );
    assert!(!sandbox.root().join("workspace.v1.bak").exists());
    let upgraded = store.migrate_v1(7).unwrap();
    assert_eq!(upgraded.revision, 8);
    assert_eq!(upgraded.spaces[0].name, "旧空间");
    assert_eq!(upgraded.contexts[0].id, "0000000000000003");
    assert_eq!(upgraded.contexts[0].space_id, "0000000000000001");
    assert_eq!(
        upgraded.contexts[0].group_id.as_deref(),
        Some("0000000000000002")
    );
    assert!(!upgraded
        .spaces
        .iter()
        .any(|s| s.name == "个人" || s.name == "公共"));
    assert_eq!(
        fs::read(sandbox.root().join("workspace.v1.bak")).unwrap(),
        original
    );
    assert_eq!(
        fs::metadata(sandbox.root().join("workspace.v1.bak"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(store.legacy_summary().unwrap(), None);
    assert_eq!(store.migrate_v1(8), Err(OrganizationError::InvalidInput));
    assert_eq!(sandbox.store().load().unwrap(), upgraded);
    let later = store
        .apply(
            upgraded.revision,
            Change::MoveContext {
                id: upgraded.contexts[0].id.clone(),
                space_id: upgraded.spaces[0].id.clone(),
                group_id: None,
            },
        )
        .unwrap();
    assert_eq!(later.contexts[0].group_id, None);
}

#[test]
fn invalid_legacy_does_not_become_migratable_or_overwrite_original() {
    let sandbox = Sandbox::new();
    let store = sandbox.store();
    let mut parsed: serde_json::Value = serde_json::from_slice(&legacy_fixture()).unwrap();
    parsed["contexts"][0]["group_id"] = serde_json::json!("0000000000000099");
    let invalid = serde_json::to_vec(&parsed).unwrap();
    write_legacy(&sandbox, &invalid);
    assert_eq!(
        store.legacy_summary(),
        Err(OrganizationError::InvalidSnapshot)
    );
    assert_eq!(store.load(), Err(OrganizationError::InvalidSnapshot));
    assert_eq!(store.migrate_v1(7), Err(OrganizationError::InvalidSnapshot));
    assert_eq!(
        fs::read(sandbox.root().join("workspace.json")).unwrap(),
        invalid
    );
    assert!(!sandbox.root().join("workspace.v1.bak").exists());
}

#[test]
fn preexisting_backup_fails_closed_without_overwriting_legacy_data() {
    let sandbox = Sandbox::new();
    let store = sandbox.store();
    let original = legacy_fixture();
    write_legacy(&sandbox, &original);
    fs::write(sandbox.root().join("workspace.v1.bak"), b"previous backup").unwrap();
    assert_eq!(store.migrate_v1(7), Err(OrganizationError::Unavailable));
    assert_eq!(
        fs::read(sandbox.root().join("workspace.json")).unwrap(),
        original
    );
    assert_eq!(
        fs::read(sandbox.root().join("workspace.v1.bak")).unwrap(),
        b"previous backup"
    );
    assert!(!sandbox.root().join("workspace.lock").exists());
}
