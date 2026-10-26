//! Deterministic commit data for trusted Host execution; no Git writes.
use super::{
    git_commit_admission::GitCommitAdmission,
    git_commit_snapshot::GitCommitSnapshotSource,
    git_tree_plan::{GitCommitTreePlan, PlannedGitObject},
};
use crate::{Result, ServiceError};
use wex_workspace_store::operations::OperationStore;

/// Attempt-bound pre-write candidate, not execution authority or a durable
/// journal. Its exact object ID binds author/committer/time as well as content.
#[derive(Debug)]
pub struct GitCommitCandidate {
    tree_plan: GitCommitTreePlan,
    commit: PlannedGitObject,
}
impl GitCommitCandidate {
    pub fn oid(&self) -> &str {
        self.commit.oid()
    }
    pub fn tree_plan(&self) -> &GitCommitTreePlan {
        &self.tree_plan
    }
    pub fn commit(&self) -> &PlannedGitObject {
        &self.commit
    }
    pub fn objects(&self) -> impl Iterator<Item = &PlannedGitObject> {
        self.tree_plan
            .objects()
            .iter()
            .chain(std::iter::once(&self.commit))
    }
}

pub fn prepare_admitted_git_commit(
    operations: &OperationStore,
    admission: &GitCommitAdmission,
    source: &GitCommitSnapshotSource<'_>,
    trusted_git_program: &std::path::Path,
    metadata: &GitCommitMetadata<'_>,
) -> Result<GitCommitCandidate> {
    // Reject invalid Host metadata before doing any repository reads.
    identity(&metadata.author)?;
    identity(&metadata.committer)?;
    let tree_plan = super::git_tree_plan::prepare_admitted_git_tree(
        operations,
        admission,
        source,
        trusted_git_program,
    )?;
    let raw = encode_commit(
        tree_plan.tree(),
        tree_plan.parent(),
        admission.prepared().message(),
        metadata,
    )?;
    let commit = PlannedGitObject::commit(raw, tree_plan.parent().len())?;
    Ok(GitCommitCandidate { tree_plan, commit })
}

/// Accept only the exact precomputed candidate at current HEAD. An alternative
/// author/time/message with the same tree cannot substitute its object ID.
pub fn read_and_settle_candidate_git_commit(
    operations: &OperationStore,
    admission: &GitCommitAdmission,
    source: &GitCommitSnapshotSource<'_>,
    trusted_git_program: &std::path::Path,
    candidate: &GitCommitCandidate,
) -> Result<super::git_commit_admission::GitCommitReceipt> {
    super::git_commit_admission::read_and_settle_planned_git_commit(
        operations,
        admission,
        source,
        trusted_git_program,
        &candidate.tree_plan,
        candidate.oid(),
    )
}

/// Host-selected metadata, never model tool arguments. Not included in the
/// existing human-approved request digest; Host identity policy remains caller owned.
pub struct GitCommitIdentity<'a> {
    pub name: &'a str,
    pub email: &'a str,
    pub unix_seconds: u64,
    pub utc_offset_minutes: i16,
}
pub struct GitCommitMetadata<'a> {
    pub author: GitCommitIdentity<'a>,
    pub committer: GitCommitIdentity<'a>,
}

fn invalid() -> ServiceError {
    ServiceError::new("INVALID_COMMIT_METADATA", "提交身份、时间或对象头无效")
}

fn identity(value: &GitCommitIdentity<'_>) -> Result<String> {
    if value.name.is_empty()
        || value.name.len() > 128
        || value.name.trim() != value.name
        || value
            .name
            .chars()
            .any(|c| c.is_control() || matches!(c, '<' | '>'))
        || value.email.is_empty()
        || value.email.len() > 254
        || !value.email.is_ascii()
        || value
            .email
            .bytes()
            .any(|c| c.is_ascii_whitespace() || c.is_ascii_control() || matches!(c, b'<' | b'>'))
        || value.unix_seconds > 253_402_300_799
        || !(-840..=840).contains(&value.utc_offset_minutes)
    {
        return Err(invalid());
    }
    let Some((local, domain)) = value.email.split_once('@') else {
        return Err(invalid());
    };
    if local.is_empty() || domain.is_empty() || domain.contains('@') {
        return Err(invalid());
    }
    let offset = value.utc_offset_minutes.unsigned_abs();
    Ok(format!(
        "{} <{}> {} {}{:02}{:02}",
        value.name,
        value.email,
        value.unix_seconds,
        if value.utc_offset_minutes < 0 {
            "-"
        } else {
            "+"
        },
        offset / 60,
        offset % 60
    ))
}

fn encode_commit(
    tree: &str,
    parent: &str,
    message: &str,
    metadata: &GitCommitMetadata<'_>,
) -> Result<Vec<u8>> {
    if !matches!(tree.len(), 40 | 64)
        || tree.len() != parent.len()
        || [tree, parent].iter().any(|oid| {
            !oid.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                || oid.bytes().all(|b| b == b'0')
        })
    {
        return Err(invalid());
    }
    super::git_commit_policy::validate_commit_request(message, &["file".into()])
        .map_err(|_| invalid())?;
    let raw = format!(
        "tree {tree}\nparent {parent}\nauthor {}\ncommitter {}\n\n{message}",
        identity(&metadata.author)?,
        identity(&metadata.committer)?
    )
    .into_bytes();
    if raw.len() > 65536 {
        return Err(invalid());
    }
    Ok(raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn metadata() -> GitCommitMetadata<'static> {
        GitCommitMetadata {
            author: GitCommitIdentity {
                name: "Author",
                email: "author@example.invalid",
                unix_seconds: 1,
                utc_offset_minutes: 480,
            },
            committer: GitCommitIdentity {
                name: "Committer",
                email: "committer@example.invalid",
                unix_seconds: 2,
                utc_offset_minutes: -330,
            },
        }
    }
    #[test]
    fn commit_encoding_preserves_exact_message_and_independent_identities() {
        let raw = encode_commit(
            &"a".repeat(40),
            &"b".repeat(40),
            "exact message",
            &metadata(),
        )
        .unwrap();
        let text = String::from_utf8(raw.clone()).unwrap();
        assert!(text.contains("author Author <author@example.invalid> 1 +0800\n"));
        assert!(text
            .contains("committer Committer <committer@example.invalid> 2 -0530\n\nexact message"));
        assert!(!text.ends_with('\n'));
        let parsed = crate::parse_commit_object(&raw).unwrap();
        assert_eq!(parsed.message, "exact message");
        assert_eq!(parsed.parents, vec!["b".repeat(40)]);
    }
    #[test]
    fn commit_metadata_rejects_header_injection_and_unbounded_dates() {
        for name in [
            "",
            " Author",
            "Author\nparent injected",
            "A<B",
            "A>B",
            "A\tB",
        ] {
            let mut meta = metadata();
            meta.author.name = name;
            assert!(encode_commit(&"a".repeat(40), &"b".repeat(40), "message", &meta).is_err());
        }
        let mut meta = metadata();
        meta.committer.unix_seconds = u64::MAX;
        assert!(encode_commit(&"a".repeat(40), &"b".repeat(40), "message", &meta).is_err());
        let mut meta = metadata();
        meta.author.utc_offset_minutes = i16::MIN;
        assert!(encode_commit(&"a".repeat(40), &"b".repeat(40), "message", &meta).is_err());
    }
    #[test]
    fn email_headers_hash_formats_and_timezone_boundaries_are_checked() {
        for email in [
            "missing-at",
            "@domain",
            "user@",
            "user@domain@extra",
            "user\n@domain",
            "u<ser@domain",
            "u sér@domain",
        ] {
            let mut meta = metadata();
            meta.committer.email = email;
            assert!(encode_commit(&"a".repeat(40), &"b".repeat(40), "message", &meta).is_err());
        }
        for offset in [-840, 0, 840] {
            let mut meta = metadata();
            meta.author.utc_offset_minutes = offset;
            let raw = encode_commit(&"a".repeat(64), &"b".repeat(64), "exact\n", &meta).unwrap();
            assert_eq!(crate::parse_commit_object(&raw).unwrap().message, "exact\n");
        }
        let mut meta = metadata();
        meta.author.utc_offset_minutes = 841;
        assert!(encode_commit(&"a".repeat(40), &"b".repeat(40), "message", &meta).is_err());
        assert!(encode_commit(&"a".repeat(40), &"b".repeat(64), "message", &metadata()).is_err());
        assert!(encode_commit(&"0".repeat(40), &"b".repeat(40), "message", &metadata()).is_err());
        assert!(encode_commit(
            &"a".repeat(40),
            &"b".repeat(40),
            &"x".repeat(4097),
            &metadata()
        )
        .is_err());
    }
}
