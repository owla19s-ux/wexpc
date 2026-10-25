//! Authoritative desktop data and basic conversation service. No file/command
//! tools are exposed until their permission and execution adapters are wired.
mod application;
mod connectors;
mod domain;
mod infrastructure;

pub use application::DesktopService;
pub use connectors::openai_compatible::{
    chat_endpoint, execute_chat, execute_chat_stream, execute_chat_stream_with_progress,
};
pub use domain::*;
pub use connectors::tool_proposals::ToolProposal;
pub use connectors::tool_round::{committed_tool_messages, drive_tool_rounds, execute_model_step, ModelStep};
pub use connectors::local_dispatch::read_approved_file;
pub use connectors::github_dispatch::{create_approved_github_file, read_approved_github_file, update_approved_github_file};
pub use connectors::git_dispatch::{approved_git_diff, approved_git_status};
pub use connectors::intent_binding::digest_arguments;
pub use connectors::terminal_dispatch::{execute_approved_terminal, execute_approved_terminal_report, TerminalReport, TerminalTask};
