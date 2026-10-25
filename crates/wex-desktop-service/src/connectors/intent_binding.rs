//! Canonical, length-delimited SHA-256 binding for reviewed tool arguments.
/// This does not grant authority. Core's consumed Dispatch and trusted approval
/// are additionally required, with InFlight durably stored before side effects.
use crate::{Result, ServiceError};
use ring::digest::{digest, SHA256};
use wex_core::operation::Dispatch;

/// Callers must include *all* effectful arguments, excluding credentials.
/// The capability and resource ID are always part of the digest.
/// Content version for optimistic file replacement. Unlike the intent digest,
/// this hashes only the exact file bytes returned by an authorized read.
pub fn file_content_sha256(bytes: &[u8]) -> String {
    let hash = digest(&SHA256, bytes);
    let mut encoded = String::with_capacity(64);
    for byte in hash.as_ref() {
        use std::fmt::Write;
        write!(&mut encoded, "{byte:02x}").expect("write to String");
    }
    encoded
}

pub fn digest_arguments(capability: &str, resource_id: &str, args: &[&[u8]]) -> String {
    let mut bytes = b"WEXPC_TOOL_INTENT_V1\0".to_vec();
    for part in std::iter::once(capability.as_bytes())
        .chain(std::iter::once(resource_id.as_bytes()))
        .chain(args.iter().copied())
    {
        bytes.extend_from_slice(&(part.len() as u64).to_be_bytes());
        bytes.extend_from_slice(part);
    }
    let digest = digest(&SHA256, &bytes);
    let mut encoded = String::with_capacity(64);
    for byte in digest.as_ref() {
        use std::fmt::Write;
        write!(&mut encoded, "{byte:02x}").expect("write to String");
    }
    encoded
}

pub fn verify_arguments(dispatch: &Dispatch, args: &[&[u8]]) -> Result<()> {
    let expected = digest_arguments(
        &dispatch.intent.capability_id, &dispatch.intent.resource_id, args,
    );
    if expected != dispatch.intent.intent_digest {
        return Err(ServiceError::new(
            "INTENT_MISMATCH", "操作参数与批准时的摘要不一致",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn content_version_is_stable_and_distinct_from_tool_intent() {
        assert_eq!(file_content_sha256(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(file_content_sha256(b"hello"),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824");
        assert_ne!(file_content_sha256(b"hello"),
            digest_arguments("local_file_read", "saved", &[b"hello"]));
    }
    #[test]
    fn length_prefix_prevents_ambiguous_tuple_collision() {
        assert_ne!(
            digest_arguments("write", "repo", &[b"ab", b"c"]),
            digest_arguments("write", "repo", &[b"a", b"bc"]),
        );
    }
    #[test]
    fn capability_and_resource_identity_are_bound() {
        assert_ne!(
            digest_arguments("read", "repo_A", &[b"file"]),
            digest_arguments("read", "repo_B", &[b"file"]),
        );
        assert_ne!(
            digest_arguments("read", "repo_A", &[b"file"]),
            digest_arguments("write", "repo_A", &[b"file"]),
        );
    }
}
