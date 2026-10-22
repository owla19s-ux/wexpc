//! Pure policy over trusted, resolved local directory identities.
//! This module neither resolves paths nor performs filesystem operations.
//! Callers must supply current grants and recheck platform access at execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
    ReadWrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalTarget<'a> {
    pub directory_id: &'a str,
    pub personal: bool,
    pub system_access: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectoryGrant<'a> {
    pub directory_id: &'a str,
    pub access: Access,
    pub explicit_personal: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Denial {
    InvalidTarget,
    SystemUnavailable,
    NoGrant,
    PersonalNotAuthorized,
    OperationNotAuthorized,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allowed,
    Denied(Denial),
}

/// Evaluate one request against a current, trusted authorization snapshot.
///
/// IDs are opaque and match exactly. Space/Context links are deliberately absent.
/// An Allowed result is not an execution token or evidence of successful I/O.
pub fn decide_local_access(
    target: LocalTarget<'_>,
    requested: Access,
    grants: &[DirectoryGrant<'_>],
) -> Decision {
    if target.directory_id.trim().is_empty() {
        return Decision::Denied(Denial::InvalidTarget);
    }
    if !target.system_access {
        return Decision::Denied(Denial::SystemUnavailable);
    }
    let matching: Vec<_> = grants
        .iter()
        .filter(|grant| grant.directory_id == target.directory_id)
        .collect();
    if matching.is_empty() {
        return Decision::Denied(Denial::NoGrant);
    }
    let eligible: Vec<_> = matching
        .into_iter()
        .filter(|grant| !target.personal || grant.explicit_personal)
        .collect();
    if eligible.is_empty() {
        return Decision::Denied(Denial::PersonalNotAuthorized);
    }
    if eligible
        .iter()
        .any(|grant| grant.access == Access::ReadWrite || grant.access == requested)
    {
        Decision::Allowed
    } else {
        Decision::Denied(Denial::OperationNotAuthorized)
    }
}
