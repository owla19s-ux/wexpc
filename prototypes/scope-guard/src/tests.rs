use super::{may_overlap, ScopeError, ScopePath};

    #[test]
    fn same_file_and_ancestor_overlap() {
        let a = ScopePath::parse("C:\\WEX\\src").unwrap();
        assert!(a.overlaps(&ScopePath::parse("c:/wex/src/main.rs").unwrap()));
        assert!(a.overlaps(&ScopePath::parse("C:\\WEX\\SRC").unwrap()));
    }

    #[test]
    fn siblings_and_other_volumes_do_not_overlap() {
        let a = ScopePath::parse("C:\\WEX\\src").unwrap();
        assert!(!a.overlaps(&ScopePath::parse("C:\\WEX\\src-old").unwrap()));
        assert!(!a.overlaps(&ScopePath::parse("D:\\WEX\\src").unwrap()));
    }

    #[test]
    fn root_scope_conflicts_with_every_path_on_drive() {
        assert!(ScopePath::parse("C:\\").unwrap().overlaps(&ScopePath::parse("c:/repo/file").unwrap()));
    }

    #[test]
    fn unsafe_or_ambiguous_paths_are_rejected() {
        for path in ["", "src/main.rs", "C:relative", "C:\\repo\\..\\secrets",
                     "C:\\repo\\.\\file", "\\\\server\\share", "C:\\file:stream",
                     "C:\\folder\\bad*name"] {
            assert!(ScopePath::parse(path).is_err(), "accepted {path}");
        }
        assert_eq!(ScopePath::parse("C:\\repo\\..").unwrap_err(), ScopeError::Traversal);
    }

    #[test]
    fn set_preflight_checks_every_pair_and_rejects_bad_entries() {
        assert_eq!(may_overlap(&["C:\\repo\\src".into()], &["C:\\repo\\tests".into()]), Ok(false));
        assert_eq!(may_overlap(&["C:\\repo".into()], &["C:\\repo\\tests".into()]), Ok(true));
        assert!(may_overlap(&["relative".into()], &["D:\\safe".into()]).is_err());
    }
#[test]
fn alternate_data_stream_suffix_is_rejected() {
    assert!(ScopePath::parse("C:/repo/file.txt:stream").is_err());
    assert!(ScopePath::parse("C:/repo/file.txt").is_ok());
}

#[test]
fn reserved_device_names_and_unicode_are_rejected() {
    for path in [
        "C:/repo/CON", "C:/repo/con.txt", "C:/repo/NUL",
        "C:/repo/COM1.log", "C:/repo/LPT9", "C:/repo/文件",
    ] {
        assert!(ScopePath::parse(path).is_err(), "accepted {path}");
    }
    assert!(ScopePath::parse("C:/repo/normal.txt").is_ok());
}

#[test]
fn reservations_block_overlap_and_release_on_drop() {
    let registry = super::ScopeRegistry::default();
    let first = registry.reserve(&["C:/project/src".into()]).unwrap();
    assert_eq!(registry.active_count(), Ok(1));
    assert!(matches!(registry.reserve(&["c:/project/src/main.rs".into()]), Err(super::ReservationError::Conflict)));
    let sibling = registry.reserve(&["C:/project/tests".into()]).unwrap();
    assert_eq!(registry.active_count(), Ok(2));
    drop(first);
    let _next = registry.reserve(&["C:/project/src/main.rs".into()]).unwrap();
    drop(sibling);
}

#[test]
fn reservations_reject_invalid_scope_without_partial_acquisition() {
    let registry = super::ScopeRegistry::default();
    assert!(matches!(registry.reserve(&["C:/project".into(), "../relative".into()]), Err(super::ReservationError::Invalid(_))));
    assert_eq!(registry.active_count(), Ok(0));
    assert_eq!(registry.reserve(&[]).err(), Some(super::ReservationError::Empty));
}

#[test]
fn registry_clones_share_active_reservations() {
    let registry = super::ScopeRegistry::default();
    let second = registry.clone();
    let held = registry.reserve(&["C:/repo".into()]).unwrap();
    assert!(matches!(second.reserve(&["C:/repo/sub".into()]), Err(super::ReservationError::Conflict)));
    drop(held);
    assert!(second.reserve(&["C:/repo/sub".into()]).is_ok());
}
