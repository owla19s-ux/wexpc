use wex_core::operation::{
    Action, ExecutionChecks, OperationError as Error, OperationIntent as Intent, OperationLedger,
    OperationState as State, Outcome,
};

fn intent(id: &str) -> Intent {
    Intent {
        id: id.to_owned(),
        actor_id: "worker".to_owned(),
        conversation_id: "chat".to_owned(),
        context_id: None,
        connection_id: "github".to_owned(),
        capability_id: "replace_file".to_owned(),
        resource_id: "resource".to_owned(),
        action: Action::Write,
        expected_version: Some("blob_sha".to_owned()),
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

fn prepared() -> OperationLedger {
    let mut ledger = OperationLedger::default();
    ledger.propose(intent("op")).unwrap();
    ledger.approve("op", &intent("op"), 7).unwrap();
    ledger
}

fn started() -> OperationLedger {
    let mut ledger = prepared();
    ledger.begin("op", "attempt", checks()).unwrap();
    ledger
}

fn success() -> Outcome {
    Outcome::Succeeded {
        evidence_ref: "receipt".to_owned(),
    }
}

#[test]
fn dispatch_requires_approval_and_consumes_it_once() {
    let mut ledger = OperationLedger::default();
    ledger.propose(intent("op")).unwrap();
    assert_eq!(
        ledger.begin("op", "attempt", checks()).unwrap_err(),
        Error::InvalidState
    );
    ledger.approve("op", &intent("op"), 7).unwrap();
    let dispatch = ledger.begin("op", "attempt", checks()).unwrap();
    assert_eq!(dispatch.intent, intent("op"));
    assert_eq!(dispatch.attempt_id, "attempt");
    assert_eq!(
        ledger.begin("op", "other", checks()).unwrap_err(),
        Error::InvalidState
    );
    assert_eq!(
        ledger.approve("op", &intent("op"), 7),
        Err(Error::InvalidState)
    );
}

#[test]
fn approval_is_bound_to_every_intent_field() {
    let mut ledger = OperationLedger::default();
    ledger.propose(intent("op")).unwrap();
    let mut variants = Vec::new();
    for field in 0..11 {
        let mut changed = intent("op");
        match field {
            0 => changed.id = "other".to_owned(),
            1 => changed.actor_id = "other".to_owned(),
            2 => changed.conversation_id = "other".to_owned(),
            3 => changed.context_id = Some("context".to_owned()),
            4 => changed.connection_id = "other".to_owned(),
            5 => changed.capability_id = "other".to_owned(),
            6 => changed.resource_id = "other".to_owned(),
            7 => changed.action = Action::Read,
            8 => changed.expected_version = Some("new_blob".to_owned()),
            9 => changed.expected_version = None,
            _ => changed.intent_digest = "b".repeat(64),
        }
        variants.push(changed);
    }
    let before = ledger.view();
    for changed in variants {
        assert_eq!(
            ledger.approve("op", &changed, 7),
            Err(Error::IntentMismatch)
        );
        assert_eq!(ledger.view(), before);
    }
}

#[test]
fn each_execution_gate_is_required_and_denials_preserve_approval() {
    let mut ledger = prepared();
    let before = ledger.view();
    for gate in 0..5 {
        let mut current = checks();
        let expected = match gate {
            0 => {
                current.connection_enabled = false;
                Error::ConnectionUnavailable
            }
            1 => {
                current.authenticated = false;
                Error::AuthenticationUnavailable
            }
            2 => {
                current.platform_available = false;
                Error::PlatformUnavailable
            }
            3 => {
                current.resource_allowed = false;
                Error::ResourceDenied
            }
            _ => {
                current.authority_revision = 8;
                Error::StaleApproval
            }
        };
        assert_eq!(
            ledger.begin("op", "attempt", current).unwrap_err(),
            expected
        );
        assert_eq!(ledger.view(), before);
    }
    ledger.approve("op", &intent("op"), 8).unwrap();
    let mut current = checks();
    current.authority_revision = 8;
    ledger.begin("op", "attempt", current).unwrap();
}

#[test]
fn operation_and_attempt_identities_cannot_be_reused() {
    let mut ledger = started();
    let before = ledger.view();
    assert_eq!(ledger.propose(intent("op")), Err(Error::AlreadyExists));
    assert_eq!(ledger.view(), before);
    ledger.propose(intent("next")).unwrap();
    ledger.approve("next", &intent("next"), 7).unwrap();
    let before = ledger.view();
    assert_eq!(
        ledger.begin("next", "attempt", checks()).unwrap_err(),
        Error::AlreadyExists
    );
    assert_eq!(ledger.view(), before);
}

#[test]
fn unstarted_cancellation_does_not_claim_inflight_rollback() {
    for approved in [false, true] {
        let mut ledger = OperationLedger::default();
        ledger.propose(intent("op")).unwrap();
        if approved {
            ledger.approve("op", &intent("op"), 7).unwrap();
        }
        ledger.cancel_unstarted("op").unwrap();
        assert_eq!(ledger.view().records[0].state, State::Cancelled);
        assert_eq!(
            ledger.begin("op", "a", checks()).unwrap_err(),
            Error::InvalidState
        );
    }
    let mut ledger = started();
    let before = ledger.view();
    assert_eq!(ledger.cancel_unstarted("op"), Err(Error::InvalidState));
    assert_eq!(ledger.view(), before);
}

#[test]
fn unknown_outcome_cannot_dispatch_again_but_accepts_matching_late_evidence() {
    let mut ledger = started();
    ledger.finish("op", "attempt", Outcome::Unknown).unwrap();
    assert_eq!(
        ledger.begin("op", "another", checks()).unwrap_err(),
        Error::InvalidState
    );
    assert_eq!(
        ledger.finish("op", "wrong", success()),
        Err(Error::AttemptMismatch)
    );
    ledger.finish("op", "attempt", success()).unwrap();
    assert_eq!(
        ledger.view().records[0].state,
        State::Succeeded {
            attempt_id: "attempt".to_owned(),
            evidence_ref: "receipt".to_owned(),
        }
    );
}

#[test]
fn terminal_receipt_is_idempotent_and_cannot_be_downgraded_or_contradicted() {
    let mut ledger = started();
    ledger.finish("op", "attempt", success()).unwrap();
    let before = ledger.view();
    ledger.finish("op", "attempt", success()).unwrap();
    for outcome in [
        Outcome::Unknown,
        Outcome::Failed {
            evidence_ref: "failure".to_owned(),
        },
        Outcome::Succeeded {
            evidence_ref: "other".to_owned(),
        },
    ] {
        assert_eq!(
            ledger.finish("op", "attempt", outcome),
            Err(Error::ReceiptConflict)
        );
        assert_eq!(ledger.view(), before);
    }
}

#[test]
fn confirmed_failure_is_terminal_and_unknown_repeats_are_idempotent() {
    let mut ledger = started();
    ledger.finish("op", "attempt", Outcome::Unknown).unwrap();
    let unknown = ledger.view();
    ledger.finish("op", "attempt", Outcome::Unknown).unwrap();
    assert_eq!(ledger.view(), unknown);
    let failure = Outcome::Failed {
        evidence_ref: "failure".to_owned(),
    };
    ledger.finish("op", "attempt", failure.clone()).unwrap();
    let failed = ledger.view();
    ledger.finish("op", "attempt", failure).unwrap();
    assert_eq!(ledger.view(), failed);
    assert_eq!(
        ledger.begin("op", "again", checks()).unwrap_err(),
        Error::InvalidState
    );
}

#[test]
fn results_do_not_require_current_permissions_after_dispatch() {
    let mut ledger = started();
    // No current permission check can erase a result of an already issued attempt.
    ledger.finish("op", "attempt", success()).unwrap();
    assert!(matches!(
        ledger.view().records[0].state,
        State::Succeeded { .. }
    ));
}

#[test]
fn restore_removes_unstarted_approval_and_marks_inflight_unknown() {
    let restored = OperationLedger::from_view(prepared().view()).unwrap();
    assert_eq!(restored.view().records[0].state, State::PendingApproval);
    let mut restored = OperationLedger::from_view(started().view()).unwrap();
    assert_eq!(
        restored.view().records[0].state,
        State::OutcomeUnknown {
            attempt_id: "attempt".to_owned(),
        }
    );
    assert_eq!(
        restored.begin("op", "again", checks()).unwrap_err(),
        Error::InvalidState
    );
    restored.finish("op", "attempt", success()).unwrap();
}

#[test]
fn restore_preserves_unknown_and_terminal_results() {
    for outcome in [
        Outcome::Unknown,
        success(),
        Outcome::Failed {
            evidence_ref: "f".to_owned(),
        },
    ] {
        let mut ledger = started();
        ledger.finish("op", "attempt", outcome).unwrap();
        let view = ledger.view();
        assert_eq!(
            OperationLedger::from_view(view.clone()).unwrap().view(),
            view
        );
    }
}

#[test]
fn restore_rejects_duplicate_operations_attempts_and_bad_evidence() {
    let view = started().view();
    let mut duplicate = view.clone();
    duplicate.records.push(duplicate.records[0].clone());
    assert_eq!(
        OperationLedger::from_view(duplicate).unwrap_err(),
        Error::AlreadyExists
    );
    let mut duplicate = view.clone();
    let mut next = duplicate.records[0].clone();
    next.intent.id = "next".to_owned();
    duplicate.records.push(next);
    assert_eq!(
        OperationLedger::from_view(duplicate).unwrap_err(),
        Error::AlreadyExists
    );
    let mut bad = view;
    bad.records[0].state = State::Succeeded {
        attempt_id: "attempt".to_owned(),
        evidence_ref: "secret/value".to_owned(),
    };
    assert_eq!(
        OperationLedger::from_view(bad).unwrap_err(),
        Error::InvalidInput
    );
}

#[test]
fn invalid_inputs_and_missing_records_have_no_side_effect() {
    let mut ledger = OperationLedger::default();
    for field in 0..5 {
        let mut bad = intent("op");
        match field {
            0 => bad.actor_id = "".to_owned(),
            1 => bad.context_id = Some("../context".to_owned()),
            2 => bad.intent_digest = "A".repeat(64),
            3 => bad.expected_version = Some("version\n".to_owned()),
            _ => bad.resource_id = "资源".to_owned(),
        }
        assert_eq!(ledger.propose(bad), Err(Error::InvalidInput));
        assert!(ledger.view().records.is_empty());
    }
    assert_eq!(
        ledger.approve("missing", &intent("missing"), 7),
        Err(Error::NotFound)
    );
    assert_eq!(ledger.cancel_unstarted("missing"), Err(Error::NotFound));
    assert_eq!(
        ledger.finish("missing", "a", success()),
        Err(Error::NotFound)
    );
}

#[test]
fn invalid_attempt_or_receipt_does_not_consume_or_replace_state() {
    let mut ledger = prepared();
    let approved = ledger.view();
    assert_eq!(
        ledger.begin("op", "", checks()).unwrap_err(),
        Error::InvalidInput
    );
    assert_eq!(ledger.view(), approved);
    ledger.begin("op", "attempt", checks()).unwrap();
    let inflight = ledger.view();
    assert_eq!(
        ledger.finish(
            "op",
            "attempt",
            Outcome::Succeeded {
                evidence_ref: "".to_owned(),
            }
        ),
        Err(Error::InvalidInput)
    );
    assert_eq!(ledger.view(), inflight);
}

#[test]
fn owned_projection_edits_cannot_mutate_the_ledger() {
    let ledger = started();
    let original = ledger.view();
    let mut projection = ledger.view();
    projection.records[0].intent.actor_id = "intruder".to_owned();
    projection.records[0].state = State::PendingApproval;
    assert_eq!(ledger.view(), original);
}

#[test]
fn standalone_or_context_bound_operations_share_rules_without_role_inference() {
    let mut ledger = OperationLedger::default();
    let mut with_context = intent("context_op");
    with_context.context_id = Some("work".to_owned());
    ledger.propose(with_context.clone()).unwrap();
    ledger.propose(intent("op")).unwrap();
    assert_eq!(
        ledger.begin("context_op", "a", checks()).unwrap_err(),
        Error::InvalidState
    );
    ledger.approve("context_op", &with_context, 7).unwrap();
    ledger.begin("context_op", "a", checks()).unwrap();
}

#[test]
fn exact_byte_limits_accept_maximum_and_reject_the_next_byte() {
    let mut maximum = intent(&"i".repeat(64));
    maximum.actor_id = "a".repeat(64);
    maximum.conversation_id = "c".repeat(64);
    maximum.context_id = Some("x".repeat(64));
    maximum.connection_id = "n".repeat(64);
    maximum.capability_id = "p".repeat(64);
    maximum.resource_id = "r".repeat(64);
    maximum.expected_version = Some("é".repeat(128));
    let mut ledger = OperationLedger::default();
    ledger.propose(maximum.clone()).unwrap();
    ledger.approve(&maximum.id, &maximum, 7).unwrap();
    let before = ledger.view();
    assert_eq!(
        ledger
            .begin(&maximum.id, &"t".repeat(65), checks())
            .unwrap_err(),
        Error::InvalidInput
    );
    assert_eq!(ledger.view(), before);
    ledger
        .begin(&maximum.id, &"t".repeat(64), checks())
        .unwrap();
    let before = ledger.view();
    assert_eq!(
        ledger.finish(
            &maximum.id,
            &"t".repeat(64),
            Outcome::Succeeded {
                evidence_ref: "e".repeat(129),
            }
        ),
        Err(Error::InvalidInput)
    );
    assert_eq!(ledger.view(), before);
    ledger
        .finish(
            &maximum.id,
            &"t".repeat(64),
            Outcome::Succeeded {
                evidence_ref: "e".repeat(128),
            },
        )
        .unwrap();
    for field in 0..10 {
        let mut bad = maximum.clone();
        match field {
            0 => bad.id.push('i'),
            1 => bad.actor_id.push('a'),
            2 => bad.conversation_id.push('c'),
            3 => bad.context_id.as_mut().unwrap().push('x'),
            4 => bad.connection_id.push('n'),
            5 => bad.capability_id.push('p'),
            6 => bad.resource_id.push('r'),
            7 => bad.expected_version.as_mut().unwrap().push('a'),
            8 => bad.intent_digest = "a".repeat(63),
            _ => bad.intent_digest = "a".repeat(65),
        }
        let mut empty = OperationLedger::default();
        assert_eq!(empty.propose(bad), Err(Error::InvalidInput));
        assert!(empty.view().records.is_empty());
    }
}

#[test]
fn used_attempt_identity_survives_unknown_terminal_and_restoration() {
    for outcome in [
        Outcome::Unknown,
        success(),
        Outcome::Failed {
            evidence_ref: "failed".to_owned(),
        },
    ] {
        let mut ledger = started();
        ledger.finish("op", "attempt", outcome).unwrap();
        let view = ledger.view();
        for mut ledger in [ledger, OperationLedger::from_view(view).unwrap()] {
            ledger.propose(intent("next")).unwrap();
            ledger.approve("next", &intent("next"), 7).unwrap();
            let before = ledger.view();
            assert_eq!(
                ledger.begin("next", "attempt", checks()).unwrap_err(),
                Error::AlreadyExists
            );
            assert_eq!(ledger.view(), before);
        }
    }
}

#[test]
fn trusted_transaction_hydration_preserves_approval_and_inflight_states() {
    for ledger in [prepared(), started()] {
        let view = ledger.view();
        assert_eq!(
            OperationLedger::from_trusted_view(view.clone())
                .unwrap()
                .view(),
            view
        );
    }
}
