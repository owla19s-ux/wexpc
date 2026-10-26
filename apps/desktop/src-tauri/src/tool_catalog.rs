//! Model-visible capability catalog assembled from live connector state.
//! Discovery does not grant execution permission. Dispatch is still guarded
//! by the existing Host/Core approval, intent binding and receipt flow.
use crate::tool_approvals::ToolPolicyAuthority;
use tauri::AppHandle;
use wex_api::ToolDefinition;

/// Stable capability declaration. The model-facing name is distinct from the
/// Core capability ID; exposure is not permission to execute.
#[derive(Debug, Clone, Copy)]
struct CapabilityDescriptor {
    name: &'static str,
    capability_id: &'static str,
    description: &'static str,
    parameters_json: &'static str,
}

impl CapabilityDescriptor {
    fn definition(self) -> ToolDefinition {
        ToolDefinition {
            name: self.name.into(),
            description: self.description.into(),
            parameters_json: self.parameters_json.into(),
        }
    }
}

const NO_ARGUMENTS: &str = r#"{"type":"object","properties":{},"additionalProperties":false}"#;
const BUILTIN_COMMANDS: &[CapabilityDescriptor] = &[
    CapabilityDescriptor { name:"git_status",capability_id:"git_status",description:"Read Git status of a repository selected by the user.",parameters_json:NO_ARGUMENTS },
    CapabilityDescriptor { name:"git_diff",capability_id:"git_diff",description:"Read the current unstaged Git diff without executing external diff helpers.",parameters_json:NO_ARGUMENTS },
    CapabilityDescriptor { name:"cargo_test",capability_id:"terminal_cargo_test",description:"Run cargo test --workspace --locked only after explicit per-run approval.",parameters_json:NO_ARGUMENTS },
    CapabilityDescriptor { name:"cargo_fmt_check",capability_id:"terminal_cargo_fmt_check",description:"Run cargo fmt --all -- --check after explicit per-run approval.",parameters_json:NO_ARGUMENTS },
];
const GITHUB_CAPABILITIES: &[CapabilityDescriptor] = &[
    CapabilityDescriptor {
        name:"github_read_file",capability_id:"github_file_read",
        description:"Read an existing GitHub UTF-8 file and its current blob SHA under the current connection, conversation execution policy and Core authorization.",
        parameters_json:r#"{"type":"object","properties":{"owner":{"type":"string"},"repo":{"type":"string"},"path":{"type":"string"}},"required":["owner","repo","path"],"additionalProperties":false}"#,
    },
    CapabilityDescriptor {
        name:"github_create_file",capability_id:"github_file_create",
        description:"Create a new GitHub UTF-8 text file only when the path does not exist, under current connection and conversation execution policy.",
        parameters_json:r#"{"type":"object","properties":{"owner":{"type":"string"},"repo":{"type":"string"},"path":{"type":"string"},"content":{"type":"string"}},"required":["owner","repo","path","content"],"additionalProperties":false}"#,
    },
    CapabilityDescriptor {
        name:"github_write_file",capability_id:"github_file_write",
        description:"Update an existing GitHub text file using its exact current blob SHA; the current conversation policy decides automatic execution, user approval or blocking.",
        parameters_json:r#"{"type":"object","properties":{"owner":{"type":"string"},"repo":{"type":"string"},"path":{"type":"string"},"sha":{"type":"string"},"content":{"type":"string"}},"required":["owner","repo","path","sha","content"],"additionalProperties":false}"#,
    },
];
fn generic_process_definition() -> ToolDefinition {
    ToolDefinition {
        name: "run_command".into(),
        description: "Run a program with exact arguments in a user-selected working directory after explicit native approval. Returns actual exit status and bounded stdout/stderr. No implicit shell; timeout or interrupted output requires checking side effects before retry.".into(),
        parameters_json: serde_json::json!({"type":"object","properties":{
            "program":{"type":"string"},"arguments":{"type":"array","items":{"type":"string"}},
            "timeout_ms":{"type":"integer","minimum":1}},
            "required":["program","arguments","timeout_ms"],"additionalProperties":false}).to_string(),
    }
}

/// Local file capabilities are scoped to this conversation's configured directories.
fn append_local_capabilities(app: &AppHandle, chat: &wex_desktop_service::PreparedChat, policies: &impl ToolPolicyAuthority, tools: &mut Vec<ToolDefinition>) {
    let tool = ToolDefinition {
        name: "read_file".into(),
        description: "Read a single UTF-8 file only after the user selects that exact file in a native approval dialog.".into(),
        parameters_json: r#"{"type":"object","properties":{"file_name":{"type":"string"}},"required":["file_name"],"additionalProperties":false}"#.into(),
    };
    let readable = policies
        .policy("local_file_read")
        .is_ok_and(|s| s.policy != wex_desktop_service::PolicyChoice::Block);
    if readable {
        tools.push(tool);
    }
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
}

/// Built-in git and process capabilities use existing per-run Host checks.
fn append_host_commands(tools: &mut Vec<ToolDefinition>) {
    for descriptor in BUILTIN_COMMANDS {
        tools.push(descriptor.definition());
    }
    tools.push(generic_process_definition());
}

/// Testable discovery gate: the global connection must be enabled before
/// any per-conversation policy is consulted. Discovery never authorizes dispatch.
fn append_github_for_status(
    enabled: bool,
    mut capability_available: impl FnMut(&str) -> bool,
    tools: &mut Vec<ToolDefinition>,
) {
    if !enabled {
        return;
    }
    for descriptor in GITHUB_CAPABILITIES {
        if capability_available(descriptor.capability_id) {
            tools.push(descriptor.definition());
        }
    }
}

/// GitHub discovery is conditional on global connection and per-conversation policy.
/// A failed status read is treated as disabled, not as permission to advertise tools.
fn append_github_capabilities(
    app: &AppHandle,
    policies: &impl ToolPolicyAuthority,
    tools: &mut Vec<ToolDefinition>,
) {
    let enabled = crate::github_config::status(app).is_ok_and(|status| status.enabled);
    append_github_for_status(
        enabled,
        |capability| {
            policies.policy(capability).is_ok_and(|setting| {
                wex_core::execution_policy::decide(setting.policy.into())
                    != wex_core::execution_policy::PolicyDecision::Blocked
            })
        },
        tools,
    );
}

pub(crate) fn available_tools(
    app: &AppHandle,
    chat: &wex_desktop_service::PreparedChat,
    policies: &impl ToolPolicyAuthority,
) -> Vec<ToolDefinition> {
    let mut tools = Vec::new();
    append_local_capabilities(app, chat, policies, &mut tools);
    append_host_commands(&mut tools);
    append_github_capabilities(app, policies, &mut tools);
    tools
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn declared_capabilities_have_distinct_names_and_valid_schemas() {
        let mut names = HashSet::new();
        for descriptor in BUILTIN_COMMANDS.iter().chain(GITHUB_CAPABILITIES.iter()) {
            assert!(!descriptor.capability_id.is_empty());
            assert!(names.insert(descriptor.name), "duplicate tool: {}", descriptor.name);
            let schema: serde_json::Value =
                serde_json::from_str(descriptor.parameters_json).expect("valid schema JSON");
            assert_eq!(schema["type"], "object");
            let definition = descriptor.definition();
            assert_eq!(definition.name, descriptor.name);
            assert_eq!(definition.parameters_json, descriptor.parameters_json);
        }
        let process = generic_process_definition();
        assert!(names.insert(process.name.as_str()));
        let schema: serde_json::Value =
            serde_json::from_str(&process.parameters_json).expect("valid process schema");
        assert_eq!(schema["type"], "object");
    }

    #[test]
    fn host_command_provider_keeps_existing_model_tool_order() {
        let mut tools = Vec::new();
        append_host_commands(&mut tools);
        let names: Vec<_> = tools.iter().map(|tool| tool.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["git_status", "git_diff", "cargo_test", "cargo_fmt_check", "run_command"]
        );
    }

    #[test]
    fn disabled_github_connection_never_checks_policies_or_advertises_tools() {
        let mut tools = Vec::new();
        let mut policy_reads = 0;
        append_github_for_status(
            false,
            |_| {
                policy_reads += 1;
                true
            },
            &mut tools,
        );
        assert!(tools.is_empty());
        assert_eq!(policy_reads, 0);
    }

    #[test]
    fn enabled_github_connection_exposes_only_allowed_capabilities_in_stable_order() {
        let mut tools = Vec::new();
        let mut checked = Vec::new();
        append_github_for_status(
            true,
            |id| {
                checked.push(id.to_owned());
                id != "github_file_create"
            },
            &mut tools,
        );
        assert_eq!(
            checked,
            ["github_file_read", "github_file_create", "github_file_write"]
        );
        assert_eq!(
            tools.iter().map(|tool| tool.name.as_str()).collect::<Vec<_>>(),
            ["github_read_file", "github_write_file"]
        );
        for tool in &tools {
            let schema: serde_json::Value =
                serde_json::from_str(&tool.parameters_json).expect("valid advertised schema");
            assert_eq!(schema["type"], "object");
            assert_eq!(schema["additionalProperties"], false);
        }
    }

    #[test]
    fn enabled_github_connection_with_all_policies_blocked_exposes_nothing() {
        let mut tools = Vec::new();
        append_github_for_status(true, |_| false, &mut tools);
        assert!(tools.is_empty());
    }

    #[test]
    fn github_capabilities_have_explicit_core_ids() {
        assert_eq!(
            GITHUB_CAPABILITIES.iter().map(|d| d.capability_id).collect::<Vec<_>>(),
            vec!["github_file_read", "github_file_create", "github_file_write"]
        );
    }
}
