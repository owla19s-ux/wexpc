//! Authoritative desktop data, conversation service and controlled tool adapters.
//! Execution authorization remains owned by Core and the trusted Host.
mod application;
mod connectors;
mod domain;
mod infrastructure;
pub(crate) const MAX_TOOL_RESULT_BYTES: usize = 1024 * 1024;

pub use application::DesktopService;
pub use connectors::git_dispatch::{approved_git_diff, approved_git_status};
pub use connectors::github_dispatch::{
    create_approved_github_file, read_approved_github_file, update_approved_github_file,
};
pub use connectors::intent_binding::{digest_arguments, file_content_sha256};
pub use connectors::local_dispatch::read_approved_file;
pub use connectors::openai_compatible::{
    chat_endpoint, execute_chat, execute_chat_stream, execute_chat_stream_with_progress,
};
pub use connectors::process_dispatch::{
    execute_approved_process_report, ProcessReport, ProcessRequest,
};
pub use connectors::terminal_dispatch::{
    execute_approved_terminal, execute_approved_terminal_report, TerminalReport, TerminalTask,
};
pub use connectors::tool_proposals::ToolProposal;
pub use connectors::tool_round::{
    committed_tool_messages, drive_tool_rounds, drive_tool_rounds_checkpointed,
    drive_tool_rounds_from_history, execute_model_step, ModelStep,
};
pub use domain::*;

mod tool_journal;
pub use tool_journal::{append_tool_marker, read_tool_markers, ToolReceiptMarker};

mod tool_results;
pub use tool_results::{read_tool_results, save_tool_result, SavedToolResult};

mod tool_checkpoints;
pub use tool_checkpoints::{
    load_tool_checkpoint, restore_tool_checkpoint, restore_tool_checkpoint_with_cursor,
    save_tool_checkpoint,
};

mod saved_chat_reply;
pub use saved_chat_reply::{read_chat_reply, save_chat_reply};
