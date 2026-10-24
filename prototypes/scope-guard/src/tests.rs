#[cfg(test)]
mod tests {
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
}
