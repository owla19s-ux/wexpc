#![cfg(unix)]
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use wex_core::connection_session::{
    AiSelection, Catalog, CatalogError, ConnectionKind, ConnectionStatus, NewConversation,
};
use wex_workspace_store::catalog::{CatalogChange, CatalogStore, CatalogStoreError};
use wex_workspace_store::{OrganizationError, OrganizationStore};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let name = format!(
            "wex-catalog-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        Self(std::env::temp_dir().join(name))
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn add(id: &str) -> CatalogChange {
    CatalogChange::AddConnection {
        id: id.to_owned(),
        name: "AI".to_owned(),
        kind: ConnectionKind::Ai,
        credential_ref: Some("vault_ref".to_owned()),
    }
}
fn pick() -> AiSelection {
    AiSelection {
        connection_id: "ai".to_owned(),
        model_id: "model".to_owned(),
    }
}
fn write_raw(root: &Root, text: &str) {
    let path = root.0.join("catalog.json");
    fs::write(&path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

#[test]
fn saved_default_conversations_and_tombstones_survive_reopening() {
    let root = Root::new();
    let store = CatalogStore::open(&root.0).unwrap();
    assert_eq!(store.load().unwrap().revision, 0);
    store.apply(0, add("ai")).unwrap();
    store
        .apply(1, CatalogChange::SetDefault(Some(pick())))
        .unwrap();
    store
        .apply(
            2,
            CatalogChange::CreateConversation {
                id: "chat".to_owned(),
                title: "Chat".to_owned(),
                choice: NewConversation::Default,
            },
        )
        .unwrap();
    store
        .apply(
            3,
            CatalogChange::RetireConnection {
                id: "ai".to_owned(),
            },
        )
        .unwrap();
    let before = store.load().unwrap();
    drop(store);
    let reopened = CatalogStore::open(&root.0).unwrap();
    let after = reopened.load().unwrap();
    assert_eq!(after, before);
    assert_eq!(after.revision, 4);
    assert_eq!(
        after.catalog.connections[0].status,
        ConnectionStatus::Retired
    );
    assert_eq!(after.catalog.conversations[0].selection, Some(pick()));
    assert_eq!(after.catalog.default_selection, Some(pick()));
    let restored = Catalog::from_view(after.catalog).unwrap();
    assert_eq!(
        restored.resolve_conversation("chat"),
        Err(CatalogError::Unavailable)
    );
    assert_eq!(
        reopened.apply(4, add("ai")),
        Err(CatalogStoreError::Domain(CatalogError::AlreadyExists))
    );
}

#[test]
fn competing_store_instances_must_use_latest_revision() {
    let root = Root::new();
    let one = CatalogStore::open(&root.0).unwrap();
    let two = CatalogStore::open(&root.0).unwrap();
    one.apply(0, add("first")).unwrap();
    let before = fs::read(root.0.join("catalog.json")).unwrap();
    assert_eq!(
        two.apply(0, add("second")),
        Err(CatalogStoreError::Storage(OrganizationError::Conflict))
    );
    assert_eq!(fs::read(root.0.join("catalog.json")).unwrap(), before);
    two.apply(1, add("second")).unwrap();
    assert_eq!(one.load().unwrap().catalog.connections.len(), 2);
}

#[test]
fn failed_domain_change_preserves_bytes_revision_and_lock_availability() {
    let root = Root::new();
    let store = CatalogStore::open(&root.0).unwrap();
    store.apply(0, add("ai")).unwrap();
    let before = fs::read(root.0.join("catalog.json")).unwrap();
    assert_eq!(
        store.apply(
            1,
            CatalogChange::SetDefault(Some(AiSelection {
                connection_id: "missing".to_owned(),
                model_id: "model".to_owned(),
            }))
        ),
        Err(CatalogStoreError::Domain(CatalogError::NotFound))
    );
    assert_eq!(fs::read(root.0.join("catalog.json")).unwrap(), before);
    assert!(!root.0.join("catalog.lock").exists());
    assert_eq!(store.load().unwrap().revision, 1);
}

#[test]
fn unknown_and_malformed_snapshots_never_reset_or_overwrite() {
    let root = Root::new();
    let store = CatalogStore::open(&root.0).unwrap();
    for bad in [
        "not json",
        r#"{"schema_version":1,"revision":0,"connections":[],"conversations":[],"default_selection":null,"api_key":"unexpected"}"#,
        r#"{"schema_version":1,"schema_version":1,"revision":0,"connections":[],"conversations":[],"default_selection":null}"#,
        r#"{"schema_version":1,"revision":0,"connections":[],"conversations":[{"id":"c","title":"C","selection":{"connection_id":"missing","model_id":"m"}}],"default_selection":null}"#,
    ] {
        write_raw(&root, bad);
        assert_eq!(store.load(), Err(CatalogStoreError::InvalidSnapshot));
        assert_eq!(
            store.apply(0, add("ai")),
            Err(CatalogStoreError::InvalidSnapshot)
        );
        assert_eq!(
            fs::read_to_string(root.0.join("catalog.json")).unwrap(),
            bad
        );
    }
    let future = r#"{"schema_version":2,"revision":0,"connections":[],"conversations":[],"default_selection":null}"#;
    write_raw(&root, future);
    assert_eq!(store.load(), Err(CatalogStoreError::UnsupportedVersion));
    assert_eq!(
        store.apply(0, add("ai")),
        Err(CatalogStoreError::UnsupportedVersion)
    );
    assert_eq!(
        fs::read_to_string(root.0.join("catalog.json")).unwrap(),
        future
    );
}

#[test]
fn duplicated_id_or_invalid_kind_is_rejected_during_restore() {
    let root = Root::new();
    let store = CatalogStore::open(&root.0).unwrap();
    store.apply(0, add("ai")).unwrap();
    let raw = fs::read_to_string(root.0.join("catalog.json")).unwrap();
    let mut document: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let record = document["connections"][0].clone();
    document["connections"].as_array_mut().unwrap().push(record);
    write_raw(&root, &serde_json::to_string(&document).unwrap());
    assert_eq!(store.load(), Err(CatalogStoreError::InvalidSnapshot));
    for field in ["kind", "status", "credential_ref"] {
        let mut document: serde_json::Value = serde_json::from_str(&raw).unwrap();
        document["connections"][0][field] = serde_json::json!({"unexpected": "value"});
        write_raw(&root, &serde_json::to_string(&document).unwrap());
        assert_eq!(store.load(), Err(CatalogStoreError::InvalidSnapshot));
    }
    let mut document: serde_json::Value = serde_json::from_str(&raw).unwrap();
    document["connections"][0]["api_key"] = serde_json::json!("unexpected");
    write_raw(&root, &serde_json::to_string(&document).unwrap());
    assert_eq!(store.load(), Err(CatalogStoreError::InvalidSnapshot));
}

#[test]
fn lock_and_orphan_temporary_snapshot_are_preserved() {
    let root = Root::new();
    let store = CatalogStore::open(&root.0).unwrap();
    fs::write(root.0.join("catalog.lock"), "diagnostic").unwrap();
    assert_eq!(
        store.load(),
        Err(CatalogStoreError::Storage(OrganizationError::Busy))
    );
    assert_eq!(
        fs::read_to_string(root.0.join("catalog.lock")).unwrap(),
        "diagnostic"
    );
    fs::remove_file(root.0.join("catalog.lock")).unwrap();
    fs::write(root.0.join("catalog.tmp"), "partial").unwrap();
    assert_eq!(
        store.load(),
        Err(CatalogStoreError::Storage(OrganizationError::Unavailable))
    );
    assert_eq!(
        fs::read_to_string(root.0.join("catalog.tmp")).unwrap(),
        "partial"
    );
}

#[test]
fn unsafe_root_or_snapshot_permissions_and_links_are_rejected() {
    let root = Root::new();
    let store = CatalogStore::open(&root.0).unwrap();
    store.apply(0, add("ai")).unwrap();
    let path = root.0.join("catalog.json");
    assert_eq!(
        fs::metadata(&root.0).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(
        store.load(),
        Err(CatalogStoreError::Storage(
            OrganizationError::InvalidSnapshot
        ))
    );
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let moved = root.0.join("other");
    fs::rename(&path, &moved).unwrap();
    symlink(&moved, &path).unwrap();
    assert_eq!(
        store.load(),
        Err(CatalogStoreError::Storage(
            OrganizationError::InvalidSnapshot
        ))
    );
    fs::set_permissions(&root.0, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(CatalogStore::open(&root.0).is_err());
    fs::set_permissions(&root.0, fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn catalog_and_existing_organization_files_remain_independent() {
    let root = Root::new();
    let organization = OrganizationStore::open(&root.0).unwrap();
    let catalog = CatalogStore::open(&root.0).unwrap();
    let org = organization
        .apply(
            0,
            wex_workspace_store::Change::CreateSpace {
                name: "Work".to_owned(),
            },
        )
        .unwrap();
    let bytes = fs::read(root.0.join("workspace.json")).unwrap();
    catalog.apply(0, add("ai")).unwrap();
    assert_eq!(organization.load().unwrap(), org);
    assert_eq!(fs::read(root.0.join("workspace.json")).unwrap(), bytes);
    assert_eq!(catalog.load().unwrap().revision, 1);
}

#[test]
fn oversized_or_revision_exhausted_state_does_not_get_reinitialized() {
    let root = Root::new();
    let store = CatalogStore::open(&root.0).unwrap();
    write_raw(&root, &" ".repeat(262145));
    assert_eq!(
        store.load(),
        Err(CatalogStoreError::Storage(
            OrganizationError::InvalidSnapshot
        ))
    );
    let exhausted = format!(
        r#"{{"schema_version":1,"revision":{},"connections":[],"conversations":[],"default_selection":null}}"#,
        u64::MAX
    );
    write_raw(&root, &exhausted);
    assert_eq!(store.load().unwrap().revision, u64::MAX);
    assert_eq!(
        store.apply(u64::MAX, add("ai")),
        Err(CatalogStoreError::Storage(
            OrganizationError::RevisionExhausted
        ))
    );
    assert_eq!(
        fs::read_to_string(root.0.join("catalog.json")).unwrap(),
        exhausted
    );
}

#[test]
fn successful_mutations_roundtrip_through_the_core_rules() {
    let root = Root::new();
    let store = CatalogStore::open(&root.0).unwrap();
    store.apply(0, add("ai")).unwrap();
    store
        .apply(
            1,
            CatalogChange::RenameConnection {
                id: "ai".to_owned(),
                name: "Renamed".to_owned(),
            },
        )
        .unwrap();
    store
        .apply(
            2,
            CatalogChange::CreateConversation {
                id: "c".to_owned(),
                title: "Before".to_owned(),
                choice: NewConversation::Explicit(pick()),
            },
        )
        .unwrap();
    store
        .apply(
            3,
            CatalogChange::RenameConversation {
                id: "c".to_owned(),
                title: "After".to_owned(),
            },
        )
        .unwrap();
    store
        .apply(
            4,
            CatalogChange::SetEnabled {
                id: "ai".to_owned(),
                enabled: false,
            },
        )
        .unwrap();
    let loaded = store.load().unwrap();
    assert_eq!(loaded.catalog.connections[0].name, "Renamed");
    assert_eq!(loaded.catalog.conversations[0].title, "After");
    assert_eq!(
        Catalog::from_view(loaded.catalog)
            .unwrap()
            .resolve_conversation("c"),
        Err(CatalogError::Unavailable)
    );
}

#[test]
fn write_size_limit_preserves_last_snapshot_and_releases_lock() {
    let root = Root::new();
    let store = CatalogStore::open(&root.0).unwrap();
    store.apply(0, add("ai")).unwrap();
    let raw = fs::read_to_string(root.0.join("catalog.json")).unwrap();
    let mut document: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let mut index = 0;
    loop {
        let mut record = document["connections"][0].clone();
        record["id"] = serde_json::json!(format!("id_{index}"));
        record["name"] = serde_json::json!("N".repeat(128));
        document["connections"].as_array_mut().unwrap().push(record);
        if serde_json::to_vec(&document).unwrap().len() > 262144 {
            document["connections"].as_array_mut().unwrap().pop();
            break;
        }
        index += 1;
    }
    let before = serde_json::to_string(&document).unwrap();
    write_raw(&root, &before);
    assert!(store.load().is_ok());
    assert_eq!(
        store.apply(
            1,
            CatalogChange::AddConnection {
                id: "new".to_owned(),
                name: "N".repeat(128),
                kind: ConnectionKind::Ai,
                credential_ref: Some("R".repeat(128)),
            }
        ),
        Err(CatalogStoreError::Storage(OrganizationError::LimitExceeded))
    );
    assert_eq!(
        fs::read_to_string(root.0.join("catalog.json")).unwrap(),
        before
    );
    assert!(!root.0.join("catalog.lock").exists());
    assert!(!root.0.join("catalog.tmp").exists());
    assert_eq!(store.load().unwrap().revision, 1);
}

#[test]
fn future_schema_with_new_fields_is_unsupported_and_preserved() {
    let root = Root::new();
    let store = CatalogStore::open(&root.0).unwrap();
    for future in [
        r#"{"schema_version":2,"new_field":{"shape":"unknown"}}"#,
        r#"{"schema_version":4294967296,"connections":"future layout"}"#,
    ] {
        write_raw(&root, future);
        assert_eq!(store.load(), Err(CatalogStoreError::UnsupportedVersion));
        assert_eq!(
            store.apply(0, add("ai")),
            Err(CatalogStoreError::UnsupportedVersion)
        );
        assert_eq!(
            fs::read_to_string(root.0.join("catalog.json")).unwrap(),
            future
        );
    }
}

#[test]
fn core_restore_preserves_unconfigured_and_rejects_tool_binding_or_duplicate_chat() {
    let mut catalog = Catalog::default();
    catalog
        .add_connection("ai", "AI", ConnectionKind::Ai, None)
        .unwrap();
    catalog
        .create_conversation("c", "C", NewConversation::Explicit(pick()))
        .unwrap();
    catalog
        .create_conversation("empty", "Empty", NewConversation::Unconfigured)
        .unwrap();
    catalog.set_default(Some(pick())).unwrap();
    catalog.set_enabled("ai", false).unwrap();
    let view = catalog.view();
    let restored = Catalog::from_view(view.clone()).unwrap();
    assert_eq!(restored.view(), view);
    assert_eq!(
        restored.resolve_conversation("empty"),
        Err(CatalogError::Unconfigured)
    );
    let mut duplicate = view.clone();
    duplicate
        .conversations
        .push(duplicate.conversations[0].clone());
    assert_eq!(
        Catalog::from_view(duplicate).unwrap_err(),
        CatalogError::AlreadyExists
    );
    let mut tool = view;
    tool.connections[0].kind = ConnectionKind::Tool("github".to_owned());
    assert_eq!(Catalog::from_view(tool).unwrap_err(), CatalogError::NotAi);
}
