//! Trusted local read adapter. No model-facing registration or auto grant.
/// This is an execution primitive, not an independent authorization decision.
use crate::{Result, ServiceError};
use wex_core::access::DirectoryGrant;
use wex_core::operation::{Action, Dispatch};
use wex_local_resources::{DirectoryRegistry, FileReadError};

/// A successfully consumed Core Dispatch must be persisted as InFlight by the
/// Host before this function is called. A Space/project reference is not a grant.
pub fn read_approved_file(
    dispatch: &Dispatch,
    registry: &DirectoryRegistry,
    leaf: &str,
    system_access: bool,
    trusted_grants: &[DirectoryGrant<'_>],
) -> Result<String> {
    if dispatch.intent.action != Action::Read || dispatch.intent.capability_id != "local_file_read"
    {
        return Err(ServiceError::new(
            "DISPATCH_REJECTED",
            "操作类型与文件读取能力不匹配",
        ));
    }
    if leaf.is_empty()
        || leaf.len() > 255
        || leaf.contains('/')
        || leaf.contains('\\')
        || leaf == "."
        || leaf == ".."
    {
        return Err(ServiceError::new(
            "INVALID_INPUT",
            "仅支持授权目录内的单个文件名",
        ));
    }
    super::intent_binding::verify_arguments(dispatch, &[leaf.as_bytes()])?;
    let bytes = registry
        .read_file(
            &dispatch.intent.resource_id,
            leaf,
            system_access,
            trusted_grants,
        )
        .map_err(map_read_error)?;
    if bytes.len() > 64 * 1024 {
        return Err(ServiceError::new(
            "TOOL_OUTPUT_LIMIT",
            "文件内容超过工具回执上限",
        ));
    }
    String::from_utf8(bytes)
        .map_err(|_| ServiceError::new("TOOL_UNSUPPORTED_CONTENT", "当前工具仅支持 UTF-8 文本"))
}

fn map_read_error(error: FileReadError) -> ServiceError {
    match error {
        FileReadError::Denied(_) => ServiceError::new("PERMISSION_DENIED", "目录或操作未获授权"),
        FileReadError::TooLarge => ServiceError::new("TOOL_OUTPUT_LIMIT", "文件超过读取上限"),
        FileReadError::Changed => ServiceError::new("RESOURCE_CHANGED", "读取期间文件发生变化"),
        FileReadError::Resource(_) => {
            ServiceError::new("RESOURCE_UNAVAILABLE", "文件不可用或不符合安全边界")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wex_core::operation::OperationIntent;

    fn dispatch(capability: &str, action: Action) -> Dispatch {
        Dispatch {
            intent: OperationIntent {
                id: "op_1".into(),
                actor_id: "a_1".into(),
                conversation_id: "conv_1".into(),
                context_id: None,
                connection_id: "local_1".into(),
                capability_id: capability.into(),
                resource_id: "root_1".into(),
                action,
                expected_version: None,
                intent_digest: "a".repeat(64),
            },
            attempt_id: "attempt_1".into(),
        }
    }

    #[test]
    fn rejects_unapproved_capabilities_and_invalid_paths() {
        let registry = DirectoryRegistry::new();
        assert_eq!(
            read_approved_file(
                &dispatch("local_file_write", Action::Write),
                &registry,
                "file.txt",
                true,
                &[]
            )
            .unwrap_err()
            .code,
            "DISPATCH_REJECTED"
        );
        assert_eq!(
            read_approved_file(
                &dispatch("local_file_read", Action::Read),
                &registry,
                "../secret",
                true,
                &[]
            )
            .unwrap_err()
            .code,
            "INVALID_INPUT"
        );
    }
}
