use wex_core::connection_session::{
    AiSelection, Catalog, CatalogError, ConnectionKind, ConnectionStatus, NewConversation,
};

fn ai(catalog: &mut Catalog, id: &str) {
    catalog
        .add_connection(
            id,
            "same display name",
            ConnectionKind::Ai,
            Some("vault_ref"),
        )
        .unwrap();
}

fn pick(connection: &str, model: &str) -> AiSelection {
    AiSelection {
        connection_id: connection.to_owned(),
        model_id: model.to_owned(),
    }
}

#[test]
fn changing_default_initializes_only_future_conversations() {
    let mut c = Catalog::default();
    ai(&mut c, "a");
    ai(&mut c, "b");
    c.set_default(Some(pick("a", "model-a"))).unwrap();
    c.create_conversation("first", "First", NewConversation::Default)
        .unwrap();
    c.set_default(Some(pick("b", "model-b"))).unwrap();
    c.create_conversation("second", "Second", NewConversation::Default)
        .unwrap();
    assert_eq!(c.resolve_conversation("first"), Ok(pick("a", "model-a")));
    assert_eq!(c.resolve_conversation("second"), Ok(pick("b", "model-b")));
}

#[test]
fn one_connection_can_serve_independent_models_and_conversations() {
    let mut c = Catalog::default();
    ai(&mut c, "shared");
    for (id, model) in [("worker", "model-a"), ("reviewer", "model-b")] {
        c.create_conversation(id, id, NewConversation::Explicit(pick("shared", model)))
            .unwrap();
    }
    assert_eq!(
        c.resolve_conversation("worker"),
        Ok(pick("shared", "model-a"))
    );
    assert_eq!(
        c.resolve_conversation("reviewer"),
        Ok(pick("shared", "model-b"))
    );
    assert_eq!(c.view().conversations.len(), 2);
}

#[test]
fn unconfigured_conversation_needs_no_connection_context_or_task() {
    let mut c = Catalog::default();
    c.create_conversation("plain", "Plain", NewConversation::Unconfigured)
        .unwrap();
    c.create_conversation("no-default", "No default", NewConversation::Default)
        .unwrap();
    assert_eq!(
        c.resolve_conversation("plain"),
        Err(CatalogError::Unconfigured)
    );
    assert_eq!(
        c.resolve_conversation("no-default"),
        Err(CatalogError::Unconfigured)
    );
    assert_eq!(c.view().conversations.len(), 2);
}

#[test]
fn disabling_never_rebinds_old_conversations_to_current_default() {
    let mut c = Catalog::default();
    ai(&mut c, "a");
    ai(&mut c, "b");
    c.create_conversation("old", "Old", NewConversation::Explicit(pick("a", "one")))
        .unwrap();
    c.set_default(Some(pick("b", "two"))).unwrap();
    c.set_enabled("a", false).unwrap();
    assert_eq!(
        c.resolve_conversation("old"),
        Err(CatalogError::Unavailable)
    );
    assert_eq!(c.view().conversations[0].selection, Some(pick("a", "one")));
    c.set_enabled("a", true).unwrap();
    assert_eq!(c.resolve_conversation("old"), Ok(pick("a", "one")));
}

#[test]
fn retired_identity_and_history_references_cannot_be_reused() {
    let mut c = Catalog::default();
    ai(&mut c, "a");
    c.set_default(Some(pick("a", "one"))).unwrap();
    c.create_conversation("old", "Old", NewConversation::Default)
        .unwrap();
    c.retire_connection("a").unwrap();
    let before = c.view();
    assert_eq!(before.connections[0].status, ConnectionStatus::Retired);
    assert_eq!(before.conversations[0].selection, Some(pick("a", "one")));
    assert_eq!(
        c.resolve_conversation("old"),
        Err(CatalogError::Unavailable)
    );
    assert_eq!(c.set_enabled("a", true), Err(CatalogError::Unavailable));
    assert_eq!(
        c.add_connection("a", "replacement", ConnectionKind::Ai, None),
        Err(CatalogError::AlreadyExists)
    );
    assert_eq!(
        c.create_conversation("new", "New", NewConversation::Default),
        Err(CatalogError::Unavailable)
    );
    assert_eq!(c.view(), before);
    c.set_default(None).unwrap();
    c.create_conversation("new", "New", NewConversation::Default)
        .unwrap();
}

#[test]
fn tool_identity_and_credential_reference_do_not_make_an_ai_connection() {
    let mut c = Catalog::default();
    c.add_connection(
        "github",
        "GitHub",
        ConnectionKind::Tool("github".to_owned()),
        Some("github_vault"),
    )
    .unwrap();
    let before = c.view();
    assert_eq!(
        c.set_default(Some(pick("github", "model"))),
        Err(CatalogError::NotAi)
    );
    assert_eq!(
        c.create_conversation(
            "chat",
            "Chat",
            NewConversation::Explicit(pick("github", "model"))
        ),
        Err(CatalogError::NotAi)
    );
    assert_eq!(c.view(), before);
}

#[test]
fn rejected_connection_inputs_leave_catalog_unchanged() {
    let mut c = Catalog::default();
    for id in ["", "a/b", " a", "a\n"] {
        assert_eq!(
            c.add_connection(id, "Name", ConnectionKind::Ai, None),
            Err(CatalogError::InvalidInput)
        );
    }
    for name in ["", " trailing ", "line\nbreak"] {
        assert_eq!(
            c.add_connection("valid", name, ConnectionKind::Ai, None),
            Err(CatalogError::InvalidInput)
        );
    }
    assert_eq!(
        c.add_connection(
            "valid",
            "Name",
            ConnectionKind::Ai,
            Some("raw key with spaces")
        ),
        Err(CatalogError::InvalidInput)
    );
    assert_eq!(
        c.add_connection("valid", "Name", ConnectionKind::Tool("".to_owned()), None),
        Err(CatalogError::InvalidInput)
    );
    assert!(c.view().connections.is_empty());
}

#[test]
fn invalid_or_missing_selection_does_not_replace_default_or_create_chat() {
    let mut c = Catalog::default();
    ai(&mut c, "a");
    c.set_default(Some(pick("a", "valid"))).unwrap();
    let before = c.view();
    for selection in [
        pick("a", ""),
        pick("a", " spaced "),
        pick("a", "line\nbreak"),
    ] {
        assert_eq!(
            c.set_default(Some(selection.clone())),
            Err(CatalogError::InvalidInput)
        );
        assert_eq!(
            c.create_conversation("new", "New", NewConversation::Explicit(selection)),
            Err(CatalogError::InvalidInput)
        );
    }
    assert_eq!(
        c.set_default(Some(pick("missing", "model"))),
        Err(CatalogError::NotFound)
    );
    assert_eq!(c.view(), before);
}

#[test]
fn rename_and_projection_edits_cannot_change_identity_or_binding() {
    let mut c = Catalog::default();
    ai(&mut c, "a");
    c.create_conversation("chat", "Old", NewConversation::Explicit(pick("a", "model")))
        .unwrap();
    c.rename_connection("a", "Renamed").unwrap();
    c.rename_conversation("chat", "Renamed chat").unwrap();
    let mut projected = c.view();
    projected.connections[0].id = "other".to_owned();
    projected.conversations[0].selection = None;
    assert_eq!(c.view().connections[0].id, "a");
    assert_eq!(c.view().connections[0].name, "Renamed");
    assert_eq!(c.view().conversations[0].title, "Renamed chat");
    assert_eq!(c.resolve_conversation("chat"), Ok(pick("a", "model")));
}

#[test]
fn duplicate_conversation_and_invalid_renames_are_atomic() {
    let mut c = Catalog::default();
    ai(&mut c, "a");
    c.create_conversation("chat", "Original", NewConversation::Unconfigured)
        .unwrap();
    let before = c.view();
    assert_eq!(
        c.create_conversation(
            "chat",
            "Replacement",
            NewConversation::Explicit(pick("a", "model"))
        ),
        Err(CatalogError::AlreadyExists)
    );
    assert_eq!(
        c.rename_connection("a", " "),
        Err(CatalogError::InvalidInput)
    );
    assert_eq!(
        c.rename_conversation("chat", " "),
        Err(CatalogError::InvalidInput)
    );
    assert_eq!(c.view(), before);
}

#[test]
fn disabled_defaults_fail_explicitly_but_unconfigured_creation_still_works() {
    let mut c = Catalog::default();
    ai(&mut c, "a");
    c.set_default(Some(pick("a", "model"))).unwrap();
    c.set_enabled("a", false).unwrap();
    let before = c.view();
    assert_eq!(
        c.create_conversation("default", "Default", NewConversation::Default),
        Err(CatalogError::Unavailable)
    );
    assert_eq!(c.view(), before);
    c.create_conversation("plain", "Plain", NewConversation::Unconfigured)
        .unwrap();
}

#[test]
fn unknown_targets_never_create_records_or_choose_fallbacks() {
    let mut c = Catalog::default();
    assert_eq!(
        c.resolve_conversation("missing"),
        Err(CatalogError::NotFound)
    );
    assert_eq!(
        c.rename_connection("missing", "Name"),
        Err(CatalogError::NotFound)
    );
    assert_eq!(
        c.rename_conversation("missing", "Title"),
        Err(CatalogError::NotFound)
    );
    assert_eq!(c.set_enabled("missing", true), Err(CatalogError::NotFound));
    assert_eq!(c.retire_connection("missing"), Err(CatalogError::NotFound));
    assert!(c.view().connections.is_empty());
    assert!(c.view().conversations.is_empty());
}

#[test]
fn byte_limits_are_explicit_and_rejections_are_atomic() {
    let mut c = Catalog::default();
    let id = "a".repeat(64);
    let name = "界".repeat(42);
    let reference = "v".repeat(128);
    c.add_connection(&id, &name, ConnectionKind::Ai, Some(&reference))
        .unwrap();
    let model = "m".repeat(256);
    c.set_default(Some(pick(&id, &model))).unwrap();
    let before = c.view();
    assert_eq!(
        c.add_connection(&"b".repeat(65), "Name", ConnectionKind::Ai, None),
        Err(CatalogError::InvalidInput)
    );
    assert_eq!(
        c.add_connection("b", &"界".repeat(43), ConnectionKind::Ai, None),
        Err(CatalogError::InvalidInput)
    );
    assert_eq!(
        c.add_connection("b", "Name", ConnectionKind::Ai, Some(&"v".repeat(129))),
        Err(CatalogError::InvalidInput)
    );
    assert_eq!(
        c.set_default(Some(pick(&id, &"m".repeat(257)))),
        Err(CatalogError::InvalidInput)
    );
    assert_eq!(c.view(), before);
}

#[test]
fn setting_or_clearing_default_does_not_retroactively_configure_chats() {
    let mut c = Catalog::default();
    c.create_conversation("plain", "Plain", NewConversation::Default)
        .unwrap();
    ai(&mut c, "a");
    c.set_default(Some(pick("a", "model"))).unwrap();
    c.create_conversation("bound", "Bound", NewConversation::Default)
        .unwrap();
    c.set_default(None).unwrap();
    assert_eq!(
        c.resolve_conversation("plain"),
        Err(CatalogError::Unconfigured)
    );
    assert_eq!(c.resolve_conversation("bound"), Ok(pick("a", "model")));
}

#[test]
fn identical_display_names_and_id_prefixes_never_merge_identities() {
    let mut c = Catalog::default();
    ai(&mut c, "a");
    ai(&mut c, "A");
    ai(&mut c, "a_worker");
    c.create_conversation(
        "a",
        "Same title",
        NewConversation::Explicit(pick("A", "same")),
    )
    .unwrap();
    c.create_conversation(
        "b",
        "Same title",
        NewConversation::Explicit(pick("a_worker", "same")),
    )
    .unwrap();
    c.set_enabled("a", false).unwrap();
    assert_eq!(c.resolve_conversation("a"), Ok(pick("A", "same")));
    assert_eq!(c.resolve_conversation("b"), Ok(pick("a_worker", "same")));
    assert_eq!(c.view().connections.len(), 3);
}
