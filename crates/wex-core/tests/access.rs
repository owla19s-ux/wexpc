use wex_core::access::{
    decide_local_access, Access, Decision, Denial, DirectoryGrant, LocalTarget,
};

fn target(id: &str, personal: bool, system_access: bool) -> LocalTarget<'_> {
    LocalTarget {
        directory_id: id,
        personal,
        system_access,
    }
}
fn grant(id: &str, access: Access, personal: bool) -> DirectoryGrant<'_> {
    DirectoryGrant {
        directory_id: id,
        access,
        explicit_personal: personal,
    }
}
#[test]
fn system_access_does_not_authorize_ai() {
    assert_eq!(
        decide_local_access(target("docs", false, true), Access::Read, &[]),
        Decision::Denied(Denial::NoGrant)
    );
}
#[test]
fn system_denial_overrides_ai_grant() {
    let grants = [grant("docs", Access::ReadWrite, true)];
    assert_eq!(
        decide_local_access(target("docs", false, false), Access::Read, &grants),
        Decision::Denied(Denial::SystemUnavailable)
    );
}
#[test]
fn read_and_write_are_independent() {
    for (allowed, requested, expected) in [
        (Access::Read, Access::Read, true),
        (Access::Read, Access::Write, false),
        (Access::Write, Access::Read, false),
        (Access::Write, Access::Write, true),
        (Access::ReadWrite, Access::Read, true),
        (Access::ReadWrite, Access::Write, true),
        (Access::Read, Access::ReadWrite, false),
        (Access::Write, Access::ReadWrite, false),
        (Access::ReadWrite, Access::ReadWrite, true),
    ] {
        assert_eq!(
            decide_local_access(
                target("docs", false, true),
                requested,
                &[grant("docs", allowed, false)]
            ) == Decision::Allowed,
            expected
        );
    }
}
#[test]
fn directory_ids_are_exact_not_prefix_or_name_rules() {
    let grants = [
        grant("root", Access::ReadWrite, true),
        grant("other", Access::Read, false),
    ];
    for id in ["root/private", "root2", "ROOT", "private"] {
        assert_eq!(
            decide_local_access(target(id, false, true), Access::Read, &grants),
            Decision::Denied(Denial::NoGrant)
        );
    }
    assert_eq!(
        decide_local_access(target("other", false, true), Access::Read, &grants),
        Decision::Allowed
    );
}
#[test]
fn personal_directory_needs_explicit_personal_grant() {
    assert_eq!(
        decide_local_access(
            target("personal", true, true),
            Access::Read,
            &[grant("personal", Access::ReadWrite, false)]
        ),
        Decision::Denied(Denial::PersonalNotAuthorized)
    );
    assert_eq!(
        decide_local_access(
            target("personal", true, true),
            Access::Read,
            &[grant("personal", Access::Read, true)]
        ),
        Decision::Allowed
    );
    assert_eq!(
        decide_local_access(
            target("personal", true, true),
            Access::Write,
            &[grant("personal", Access::Read, true)]
        ),
        Decision::Denied(Denial::OperationNotAuthorized)
    );
}
#[test]
fn separate_grants_cannot_combine_personal_consent_and_write() {
    let grants = [
        grant("personal", Access::Read, true),
        grant("personal", Access::Write, false),
    ];
    assert_eq!(
        decide_local_access(target("personal", true, true), Access::Write, &grants),
        Decision::Denied(Denial::OperationNotAuthorized)
    );
}
#[test]
fn revocation_is_observed_on_next_decision() {
    let grants = [grant("docs", Access::Read, false)];
    assert_eq!(
        decide_local_access(target("docs", false, true), Access::Read, &grants),
        Decision::Allowed
    );
    assert_eq!(
        decide_local_access(target("docs", false, true), Access::Read, &[]),
        Decision::Denied(Denial::NoGrant)
    );
}
#[test]
fn empty_identity_fails_closed_even_if_a_bad_grant_exists() {
    assert_eq!(
        decide_local_access(
            target("", false, true),
            Access::Read,
            &[grant("", Access::ReadWrite, true)]
        ),
        Decision::Denied(Denial::InvalidTarget)
    );
}
#[test]
fn grant_order_does_not_change_decision() {
    let mut grants = [
        grant("personal", Access::Write, false),
        grant("personal", Access::Read, true),
        grant("other", Access::ReadWrite, true),
    ];
    let before = decide_local_access(target("personal", true, true), Access::Write, &grants);
    grants.reverse();
    assert_eq!(
        before,
        decide_local_access(target("personal", true, true), Access::Write, &grants)
    );
}

#[test]
fn whitespace_identity_is_invalid() {
    assert_eq!(
        decide_local_access(
            target(" \t\n", false, true),
            Access::Read,
            &[grant(" \t\n", Access::ReadWrite, true)]
        ),
        Decision::Denied(Denial::InvalidTarget)
    );
}

#[test]
fn separate_read_and_write_grants_do_not_form_combined_authority() {
    let grants = [
        grant("docs", Access::Read, false),
        grant("docs", Access::Write, false),
    ];
    assert_eq!(
        decide_local_access(target("docs", false, true), Access::ReadWrite, &grants),
        Decision::Denied(Denial::OperationNotAuthorized)
    );
}
