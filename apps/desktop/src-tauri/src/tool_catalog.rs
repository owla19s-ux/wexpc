//! Model-visible capability catalog assembled from live connector state.
//! Discovery does not grant execution permission. Dispatch is still guarded
//! by the existing Host/Core approval, intent binding and receipt flow.
use crate::tool_approvals::ToolPolicyAuthority;
use tauri::AppHandle;
use wex_api::ToolDefinition;

pub(crate) fn available_tools(
    app: &AppHandle,
    chat: &wex_desktop_service::PreparedChat,
    policies: &impl ToolPolicyAuthority,
) -> Vec<ToolDefinition> {
    let tool = ToolDefinition {
    name: "read_file".into(),
    description: "Read a single UTF-8 file only after the user selects that exact file in a native approval dialog.".into(),
    parameters_json: r#"{"type":"object","properties":{"file_name":{"type":"string"}},"required":["file_name"],"additionalProperties":false}"#.into(),
};
    let readable = policies
        .policy("local_file_read")
        .is_ok_and(|s| s.policy != wex_desktop_service::PolicyChoice::Block);
    let mut tools = if readable { vec![tool] } else { Vec::new() };
    if readable {
        if let Ok(status) = crate::local_access::open(app, &chat.conversation_id)
            .and_then(|store| crate::local_access::status(&store))
        {
            let directories: Vec<_> = status
                .directories
                .into_iter()
                .filter(|d| {
                    d.available
                        && matches!(
                            d.access,
                            crate::local_access::Scope::Read
                                | crate::local_access::Scope::ReadWrite
                        )
                })
                .collect();
            if !directories.is_empty() {
                let ids: Vec<_> = directories.iter().map(|d| d.id.clone()).collect();
                tools.push(ToolDefinition{name:"read_authorized_file".into(),description:format!("Read a direct UTF-8 file from a directory explicitly authorized to this conversation. Execution follows the current local read policy. Authorized directories: {}",serde_json::to_string(&directories).unwrap_or_default()),parameters_json:serde_json::json!({"type":"object","properties":{"directory_id":{"type":"string","enum":ids},"file_name":{"type":"string"}},"required":["directory_id","file_name"],"additionalProperties":false}).to_string()});
            }
        }
    }
    if policies
        .policy("local_file_create")
        .is_ok_and(|setting| setting.policy != wex_desktop_service::PolicyChoice::Block)
    {
        if let Ok(status) = crate::local_access::open(app, &chat.conversation_id)
            .and_then(|store| crate::local_access::status(&store))
        {
            let directories: Vec<_> = status
                .directories
                .into_iter()
                .filter(|d| {
                    d.available
                        && matches!(
                            d.access,
                            crate::local_access::Scope::Write
                                | crate::local_access::Scope::ReadWrite
                        )
                })
                .collect();
            if !directories.is_empty() {
                let ids: Vec<_> = directories.iter().map(|d| d.id.clone()).collect();
                tools.push(ToolDefinition {
                    name: "create_authorized_file".into(),
                    description: format!("Create a NEW direct UTF-8 text file in an explicitly write-authorized directory. Does not overwrite. Authorized directories: {}", serde_json::to_string(&directories).unwrap_or_default()),
                    parameters_json: serde_json::json!({"type":"object","properties":{
                        "directory_id":{"type":"string","enum":ids},
                        "file_name":{"type":"string"},
                        "content":{"type":"string"}},
                        "required":["directory_id","file_name","content"],
                        "additionalProperties":false}).to_string(),
                });
            }
        }
    }
    for (name, description) in [
        (
            "git_status",
            "Read Git status of a repository selected by the user.",
        ),
        (
            "git_diff",
            "Read the current unstaged Git diff without executing external diff helpers.",
        ),
        (
            "cargo_test",
            "Run cargo test --workspace --locked only after explicit per-run approval.",
        ),
        (
            "cargo_fmt_check",
            "Run cargo fmt --all -- --check after explicit per-run approval.",
        ),
    ] {
        tools.push(ToolDefinition {
            name: name.into(),
            description: description.into(),
            parameters_json:
                r#"{"type":"object","properties":{},"additionalProperties":false}"#.into(),
        });
    }
    tools.push(ToolDefinition {
        name:"run_command".into(),
        description:"Run a program with exact arguments in a user-selected working directory after explicit native approval. Returns actual exit status and bounded stdout/stderr. No implicit shell; timeout or interrupted output requires checking side effects before retry.".into(),
        parameters_json:serde_json::json!({"type":"object","properties":{
            "program":{"type":"string"},"arguments":{"type":"array","items":{"type":"string"}},
            "timeout_ms":{"type":"integer","minimum":1}},
            "required":["program","arguments","timeout_ms"],"additionalProperties":false}).to_string(),
    });
    if crate::github_config::status(app).is_ok_and(|status| status.enabled) {
        let available = |capability: &str| {
            policies.policy(capability).is_ok_and(|setting| {
                wex_core::execution_policy::decide(setting.policy.into())
                    != wex_core::execution_policy::PolicyDecision::Blocked
            })
        };
        if available("github_file_read") {
            tools.push(ToolDefinition {
        name: "github_read_file".into(),
        description: "Read an existing GitHub UTF-8 file and its current blob SHA under the current connection, conversation execution policy and Core authorization.".into(),
        parameters_json: r#"{"type":"object","properties":{"owner":{"type":"string"},"repo":{"type":"string"},"path":{"type":"string"}},"required":["owner","repo","path"],"additionalProperties":false}"#.into(),
    });
        }
        if available("github_file_create") {
            tools.push(ToolDefinition {
        name: "github_create_file".into(),
        description: "Create a new GitHub UTF-8 text file only when the path does not exist, under current connection and conversation execution policy.".into(),
        parameters_json: r#"{"type":"object","properties":{"owner":{"type":"string"},"repo":{"type":"string"},"path":{"type":"string"},"content":{"type":"string"}},"required":["owner","repo","path","content"],"additionalProperties":false}"#.into(),
    });
        }
        if available("github_file_write") {
            tools.push(ToolDefinition {
        name: "github_write_file".into(),
        description: "Update an existing GitHub text file using its exact current blob SHA; the current conversation policy decides automatic execution, user approval or blocking.".into(),
        parameters_json: r#"{"type":"object","properties":{"owner":{"type":"string"},"repo":{"type":"string"},"path":{"type":"string"},"sha":{"type":"string"},"content":{"type":"string"}},"required":["owner","repo","path","sha","content"],"additionalProperties":false}"#.into(),
    });
        }
    }
    tools
}
