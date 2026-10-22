#![cfg(unix)]
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use wex_core::operation::{
    Action, ExecutionChecks, OperationError, OperationIntent, OperationState, Outcome,
};
use wex_workspace_store::operations::{
    OperationChange, OperationStore, OperationStoreError as Error,
};
use wex_workspace_store::{OrganizationError, OrganizationStore};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "wex-operations-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
    fn bytes(&self) -> Vec<u8> {
        fs::read(self.0.join("operations.json")).unwrap()
    }
    fn write(&self, bytes: &[u8]) {
        let path = self.0.join("operations.json");
        fs::write(&path, bytes).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn intent(id: &str) -> OperationIntent {
    OperationIntent {
        id: id.to_owned(),
        actor_id: "worker".to_owned(),
        conversation_id: "chat".to_owned(),
        context_id: None,
        connection_id: "github".to_owned(),
        capability_id: "replace".to_owned(),
        resource_id: "file".to_owned(),
        action: Action::Write,
        expected_version: Some("blob".to_owned()),
        intent_digest: "a".repeat(64),
    }
}
fn checks() -> ExecutionChecks {
    ExecutionChecks {
        authority_revision: 7,
        connection_enabled: true,
        authenticated: true,
        platform_available: true,
        resource_allowed: true,
    }
}
fn ready(root: &Root) -> OperationStore {
    let store = OperationStore::open(&root.0).unwrap();
    assert_eq!(store.load().unwrap().revision, 1);
    store
        .apply(1, OperationChange::Propose(intent("op")))
        .unwrap();
    store
        .apply(
            2,
            OperationChange::Approve {
                id: "op".to_owned(),
                exact_intent: intent("op"),
                authority_revision: 7,
            },
        )
        .unwrap();
    store
}
fn finish(outcome: Outcome) -> OperationChange {
    OperationChange::Finish {
        id: "op".to_owned(),
        attempt_id: "attempt".to_owned(),
        outcome,
    }
}
fn success() -> Outcome {
    Outcome::Succeeded {
        evidence_ref: "receipt".to_owned(),
    }
}

#[test]
fn dispatch_is_returned_only_after_the_inflight_snapshot_is_saved() {
    let root = Root::new();
    let store = ready(&root);
    let committed = store.begin(3, "op", "attempt", checks()).unwrap();
    assert_eq!(committed.revision, 4);
    assert_eq!(committed.dispatch.intent, intent("op"));
    let raw: serde_json::Value = serde_json::from_slice(&root.bytes()).unwrap();
    assert_eq!(raw["records"][0]["state"]["status"], "in_flight");
    assert_eq!(raw["records"][0]["state"]["attempt_id"], "attempt");
    assert_eq!(
        store.load().unwrap().ledger.records[0].state,
        OperationState::InFlight {
            attempt_id: "attempt".to_owned(),
        }
    );
    assert_eq!(
        store.begin(4, "op", "again", checks()).unwrap_err(),
        Error::Domain(OperationError::InvalidState)
    );
}

#[test]
fn cold_reopen_invalidates_approval_and_fences_the_previous_service() {
    let root = Root::new();
    let old = ready(&root);
    let current = OperationStore::open(&root.0).unwrap();
    let view = current.load().unwrap();
    assert_eq!(view.generation, 2);
    assert_eq!(view.revision, 4);
    assert_eq!(
        view.ledger.records[0].state,
        OperationState::PendingApproval
    );
    let before = root.bytes();
    assert_eq!(old.load(), Err(Error::StaleSession));
    assert_eq!(
        old.begin(3, "op", "attempt", checks()).unwrap_err(),
        Error::StaleSession
    );
    assert_eq!(
        old.apply(
            3,
            OperationChange::CancelUnstarted {
                id: "op".to_owned()
            }
        ),
        Err(Error::StaleSession)
    );
    assert_eq!(root.bytes(), before);
    current
        .apply(
            4,
            OperationChange::Approve {
                id: "op".to_owned(),
                exact_intent: intent("op"),
                authority_revision: 7,
            },
        )
        .unwrap();
    current.begin(5, "op", "attempt", checks()).unwrap();
}

#[test]
fn restart_keeps_unknown_attempt_and_persists_a_matching_late_result() {
    let root = Root::new();
    let store = ready(&root);
    store.begin(3, "op", "attempt", checks()).unwrap();
    drop(store);
    let reopened = OperationStore::open(&root.0).unwrap();
    assert_eq!(
        reopened.load().unwrap().ledger.records[0].state,
        OperationState::OutcomeUnknown {
            attempt_id: "attempt".to_owned(),
        }
    );
    assert_eq!(
        reopened.begin(5, "op", "retry", checks()).unwrap_err(),
        Error::Domain(OperationError::InvalidState)
    );
    reopened.apply(5, finish(success())).unwrap();
    drop(reopened);
    let final_store = OperationStore::open(&root.0).unwrap();
    let final_view = final_store.load().unwrap();
    assert_eq!(final_view.revision, 7);
    assert_eq!(
        final_view.ledger.records[0].state,
        OperationState::Succeeded {
            attempt_id: "attempt".to_owned(),
            evidence_ref: "receipt".to_owned(),
        }
    );
}

#[test]
fn stale_revision_and_domain_rejection_preserve_bytes() {
    let root = Root::new();
    let store = ready(&root);
    let before = root.bytes();
    assert_eq!(
        store.begin(2, "op", "attempt", checks()).unwrap_err(),
        Error::Storage(OrganizationError::Conflict)
    );
    let mut denied = checks();
    denied.resource_allowed = false;
    assert_eq!(
        store.begin(3, "op", "attempt", denied).unwrap_err(),
        Error::Domain(OperationError::ResourceDenied)
    );
    let mut changed = intent("op");
    changed.resource_id = "different".to_owned();
    assert_eq!(
        store.apply(
            3,
            OperationChange::Approve {
                id: "op".to_owned(),
                exact_intent: changed,
                authority_revision: 7,
            }
        ),
        Err(Error::Domain(OperationError::IntentMismatch))
    );
    assert_eq!(root.bytes(), before);
    assert!(!root.0.join("operations.lock").exists());
}

#[test]
fn identical_receipts_do_not_write_again_and_contradictions_are_rejected() {
    let root = Root::new();
    let store = ready(&root);
    store.begin(3, "op", "attempt", checks()).unwrap();
    store.apply(4, finish(success())).unwrap();
    let before = root.bytes();
    assert_eq!(store.apply(5, finish(success())).unwrap().revision, 5);
    assert_eq!(root.bytes(), before);
    assert_eq!(
        store.apply(5, finish(Outcome::Unknown)),
        Err(Error::Domain(OperationError::ReceiptConflict))
    );
    assert_eq!(
        store.apply(
            5,
            OperationChange::Finish {
                id: "op".to_owned(),
                attempt_id: "wrong".to_owned(),
                outcome: success(),
            }
        ),
        Err(Error::Domain(OperationError::AttemptMismatch))
    );
    assert_eq!(root.bytes(), before);
}

#[test]
fn cancellation_and_confirmed_failure_remain_terminal_after_reopening() {
    let root = Root::new();
    let store = ready(&root);
    store
        .apply(3, OperationChange::Propose(intent("cancel")))
        .unwrap();
    store
        .apply(
            4,
            OperationChange::CancelUnstarted {
                id: "cancel".to_owned(),
            },
        )
        .unwrap();
    store.begin(5, "op", "attempt", checks()).unwrap();
    store
        .apply(
            6,
            finish(Outcome::Failed {
                evidence_ref: "failure".to_owned(),
            }),
        )
        .unwrap();
    let before = store.load().unwrap().ledger;
    let reopened = OperationStore::open(&root.0).unwrap();
    assert_eq!(reopened.load().unwrap().ledger, before);
}

#[test]
fn corrupt_future_duplicate_and_invalid_snapshots_are_never_replaced() {
    let root = Root::new();
    let store = ready(&root);
    let original = root.bytes();
    let raw: serde_json::Value = serde_json::from_slice(&original).unwrap();
    let mut duplicate = raw.clone();
    let record = duplicate["records"][0].clone();
    duplicate["records"].as_array_mut().unwrap().push(record);
    let mut unknown = raw.clone();
    unknown["records"][0]["intent"]["api_key"] = serde_json::json!("unexpected");
    let mut invalid = raw;
    invalid["records"][0]["state"] = serde_json::json!({"status":"in_flight","attempt_id":""});
    let mut bad_documents = vec![
        b"not json".to_vec(),
        br#"{"schema_version":1,"schema_version":1,"revision":1,"generation":1,"records":[]}"#
            .to_vec(),
        serde_json::to_vec(&duplicate).unwrap(),
        serde_json::to_vec(&unknown).unwrap(),
        serde_json::to_vec(&invalid).unwrap(),
    ];
    for bad in bad_documents.drain(..) {
        root.write(&bad);
        assert_eq!(store.load(), Err(Error::InvalidSnapshot));
        assert!(matches!(
            OperationStore::open(&root.0),
            Err(Error::InvalidSnapshot)
        ));
        assert_eq!(root.bytes(), bad);
    }
    let future = br#"{"schema_version":4294967296,"new_layout":true}"#;
    root.write(future);
    assert!(matches!(
        OperationStore::open(&root.0),
        Err(Error::UnsupportedVersion)
    ));
    assert_eq!(root.bytes(), future);
}

#[test]
fn lock_and_temporary_files_preserve_evidence_and_return_no_dispatch() {
    let root = Root::new();
    let store = ready(&root);
    let before = root.bytes();
    fs::write(root.0.join("operations.lock"), b"evidence").unwrap();
    assert_eq!(
        store.begin(3, "op", "attempt", checks()).unwrap_err(),
        Error::Storage(OrganizationError::Busy)
    );
    assert_eq!(
        fs::read(root.0.join("operations.lock")).unwrap(),
        b"evidence"
    );
    fs::remove_file(root.0.join("operations.lock")).unwrap();
    fs::write(root.0.join("operations.tmp"), b"partial").unwrap();
    assert_eq!(
        store.begin(3, "op", "attempt", checks()).unwrap_err(),
        Error::Storage(OrganizationError::Unavailable)
    );
    assert!(OperationStore::open(&root.0).is_err());
    assert_eq!(fs::read(root.0.join("operations.tmp")).unwrap(), b"partial");
    assert_eq!(root.bytes(), before);
}

#[test]
fn private_modes_links_and_oversized_reads_fail_closed() {
    let root = Root::new();
    let store = ready(&root);
    let path = root.0.join("operations.json");
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
        Err(Error::Storage(OrganizationError::InvalidSnapshot))
    );
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let original = root.0.join("original");
    fs::rename(&path, &original).unwrap();
    symlink(&original, &path).unwrap();
    assert_eq!(
        store.load(),
        Err(Error::Storage(OrganizationError::InvalidSnapshot))
    );
    fs::remove_file(&path).unwrap();
    root.write(&vec![b' '; 262145]);
    assert_eq!(
        store.load(),
        Err(Error::Storage(OrganizationError::InvalidSnapshot))
    );
}

#[test]
fn exhausted_counters_are_not_reset_or_wrapped() {
    let root = Root::new();
    let store = ready(&root);
    let mut raw: serde_json::Value = serde_json::from_slice(&root.bytes()).unwrap();
    raw["revision"] = serde_json::json!(u64::MAX);
    root.write(&serde_json::to_vec(&raw).unwrap());
    let before = root.bytes();
    assert_eq!(
        store
            .begin(u64::MAX, "op", "attempt", checks())
            .unwrap_err(),
        Error::Storage(OrganizationError::RevisionExhausted)
    );
    assert!(matches!(
        OperationStore::open(&root.0),
        Err(Error::Storage(OrganizationError::RevisionExhausted))
    ));
    assert_eq!(root.bytes(), before);
    raw["generation"] = serde_json::json!(u64::MAX);
    root.write(&serde_json::to_vec(&raw).unwrap());
    let before = root.bytes();
    assert!(matches!(
        OperationStore::open(&root.0),
        Err(Error::Storage(OrganizationError::RevisionExhausted))
    ));
    assert_eq!(root.bytes(), before);
}

#[test]
fn operation_snapshots_are_independent_of_catalog_and_organization() {
    let root = Root::new();
    let org = OrganizationStore::open(&root.0).unwrap();
    org.apply(
        0,
        wex_workspace_store::Change::CreateSpace {
            name: "Work".to_owned(),
        },
    )
    .unwrap();
    let catalog = wex_workspace_store::catalog::CatalogStore::open(&root.0).unwrap();
    catalog
        .apply(
            0,
            wex_workspace_store::catalog::CatalogChange::AddConnection {
                id: "ai".to_owned(),
                name: "AI".to_owned(),
                kind: wex_core::connection_session::ConnectionKind::Ai,
                credential_ref: None,
            },
        )
        .unwrap();
    let org_bytes = fs::read(root.0.join("workspace.json")).unwrap();
    let catalog_bytes = fs::read(root.0.join("catalog.json")).unwrap();
    let store = ready(&root);
    store.begin(3, "op", "attempt", checks()).unwrap();
    assert_eq!(fs::read(root.0.join("workspace.json")).unwrap(), org_bytes);
    assert_eq!(
        fs::read(root.0.join("catalog.json")).unwrap(),
        catalog_bytes
    );
}

#[test]
fn capacity_rejection_returns_no_dispatch_and_keeps_the_approved_snapshot() {
    let root = Root::new();
    let store = ready(&root);
    let mut raw: serde_json::Value = serde_json::from_slice(&root.bytes()).unwrap();
    let mut index = 0;
    loop {
        let mut record = raw["records"][0].clone();
        record["intent"]["id"] = serde_json::json!(format!("filler_{index}"));
        raw["records"].as_array_mut().unwrap().push(record);
        if serde_json::to_vec(&raw).unwrap().len() > 262144 {
            raw["records"].as_array_mut().unwrap().pop();
            break;
        }
        index += 1;
    }
    let last = raw["records"].as_array().unwrap().len() - 1;
    for (field, limit) in [
        ("expected_version", 256),
        ("actor_id", 64),
        ("conversation_id", 64),
        ("capability_id", 64),
        ("resource_id", 64),
        ("connection_id", 64),
    ] {
        while serde_json::to_vec(&raw).unwrap().len() < 262144
            && raw["records"][last]["intent"][field]
                .as_str()
                .unwrap()
                .len()
                < limit
        {
            let text = format!(
                "{}a",
                raw["records"][last]["intent"][field].as_str().unwrap()
            );
            raw["records"][last]["intent"][field] = serde_json::json!(text);
        }
    }
    let before = serde_json::to_vec(&raw).unwrap();
    assert_eq!(before.len(), 262144);
    root.write(&before);
    assert!(store.load().is_ok());
    assert_eq!(
        store.begin(3, "op", &"t".repeat(64), checks()).unwrap_err(),
        Error::Storage(OrganizationError::LimitExceeded)
    );
    assert_eq!(root.bytes(), before);
    assert!(!root.0.join("operations.lock").exists());
    assert!(!root.0.join("operations.tmp").exists());
    assert!(matches!(
        store.load().unwrap().ledger.records[0].state,
        OperationState::Approved { .. }
    ));
}

#[test]
fn a_lost_snapshot_or_initialization_marker_is_not_a_fresh_store() {
    let root = Root::new();
    let store = ready(&root);
    let before = root.bytes();
    fs::remove_file(root.0.join("operations.json")).unwrap();
    assert_eq!(store.load(), Err(Error::InvalidSnapshot));
    assert!(matches!(
        OperationStore::open(&root.0),
        Err(Error::InvalidSnapshot)
    ));
    assert!(!root.0.join("operations.json").exists());
    root.write(&before);
    fs::remove_file(root.0.join("operations-initialized.json")).unwrap();
    assert!(matches!(
        OperationStore::open(&root.0),
        Err(Error::InvalidSnapshot)
    ));
    assert_eq!(root.bytes(), before);
}

#[test]
fn empty_states_must_reject_extra_or_duplicate_payload_fields() {
    let root = Root::new();
    let store = ready(&root);
    let mut raw: serde_json::Value = serde_json::from_slice(&root.bytes()).unwrap();
    raw["records"][0]["state"] = serde_json::json!("__STATE__");
    let template = serde_json::to_string(&raw).unwrap();
    for state in [
        r#"{"status":"pending_approval","attempt_id":"lost"}"#,
        r#"{"status":"cancelled","evidence_ref":"lost"}"#,
        r#"{"status":"pending_approval","unexpected":1,"unexpected":2}"#,
    ] {
        let text = template.replace("\"__STATE__\"", state);
        root.write(text.as_bytes());
        assert_eq!(store.load(), Err(Error::InvalidSnapshot));
        assert!(matches!(
            OperationStore::open(&root.0),
            Err(Error::InvalidSnapshot)
        ));
        assert_eq!(root.bytes(), text.as_bytes());
    }
}

fn filled_pending_snapshot(root: &Root, target: usize) -> Vec<u8> {
    let mut raw: serde_json::Value = serde_json::from_slice(&root.bytes()).unwrap();
    let mut index = 0;
    loop {
        let mut record = raw["records"][0].clone();
        record["intent"]["id"] = serde_json::json!(format!("filler_{index}"));
        record["state"] = serde_json::json!({"status":"pending_approval"});
        raw["records"].as_array_mut().unwrap().push(record);
        if serde_json::to_vec(&raw).unwrap().len() > target {
            raw["records"].as_array_mut().unwrap().pop();
            break;
        }
        index += 1;
    }
    let last = raw["records"].as_array().unwrap().len() - 1;
    for (field, limit) in [
        ("expected_version", 256),
        ("actor_id", 64),
        ("conversation_id", 64),
        ("capability_id", 64),
        ("resource_id", 64),
        ("connection_id", 64),
    ] {
        while serde_json::to_vec(&raw).unwrap().len() < target
            && raw["records"][last]["intent"][field]
                .as_str()
                .unwrap()
                .len()
                < limit
        {
            let text = format!(
                "{}a",
                raw["records"][last]["intent"][field].as_str().unwrap()
            );
            raw["records"][last]["intent"][field] = serde_json::json!(text);
        }
    }
    let bytes = serde_json::to_vec(&raw).unwrap();
    assert_eq!(bytes.len(), target);
    bytes
}

#[test]
fn admission_reserves_space_for_cold_recovery_and_growing_counters() {
    let approved = serde_json::to_vec(&serde_json::json!({
        "status":"approved","authority_revision":7
    }))
    .unwrap()
    .len();
    let inflight = serde_json::to_vec(&serde_json::json!({
        "status":"in_flight","attempt_id":"attempt"
    }))
    .unwrap()
    .len();
    assert!(inflight > approved);
    for reserve in [0, 256] {
        let root = Root::new();
        let store = ready(&root);
        let bytes = filled_pending_snapshot(&root, 262144 - (inflight - approved) - reserve);
        root.write(&bytes);
        let result = store.begin(3, "op", "attempt", checks());
        if reserve == 0 {
            assert_eq!(
                result.unwrap_err(),
                Error::Storage(OrganizationError::LimitExceeded)
            );
            assert_eq!(root.bytes(), bytes);
        } else {
            result.unwrap();
            let recovered = OperationStore::open(&root.0).unwrap();
            let view = recovered.load().unwrap();
            let op = view
                .ledger
                .records
                .iter()
                .find(|record| record.intent.id == "op")
                .unwrap();
            assert_eq!(
                op.state,
                OperationState::OutcomeUnknown {
                    attempt_id: "attempt".to_owned(),
                }
            );
            recovered
                .apply(
                    view.revision,
                    finish(Outcome::Succeeded {
                        evidence_ref: "e".repeat(128),
                    }),
                )
                .unwrap();
            let final_store = OperationStore::open(&root.0).unwrap();
            let final_view = final_store.load().unwrap();
            let op = final_view
                .ledger
                .records
                .iter()
                .find(|record| record.intent.id == "op")
                .unwrap();
            assert!(matches!(op.state, OperationState::Succeeded { .. }));
        }
    }
}
