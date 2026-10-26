//! Authoritative desktop data, conversation service and controlled tool adapters.
//! Execution authorization remains owned by Core and the trusted Host.
mod application;
mod connectors;
mod domain;
mod infrastructure;
pub(crate) const MAX_TOOL_RESULT_BYTES: usize = 1024 * 1024;

pub use application::process_control::{
    ActiveProcess, ProcessCancellation, ProcessLease, ProcessSupervisor,
};
pub use application::DesktopService;
pub use connectors::git_commit_admission::{
    begin_git_commit, git_commit_resource_id, read_and_settle_git_commit,
    read_and_settle_planned_git_commit, settle_verified_git_commit, GitCommitAdmission,
    GitCommitBegin, GitCommitReceipt,
};
pub use connectors::git_commit_binding::{git_commit_intent_digest, verify_git_commit_dispatch};
pub use connectors::git_commit_candidate::{
    prepare_admitted_git_commit, read_and_settle_candidate_git_commit, GitCommitCandidate,
    GitCommitIdentity, GitCommitMetadata,
};
pub use connectors::git_commit_evidence::{
    parse_commit_object, CommitEvidenceError, CommitObject, CommitReadback, PreparedCommit,
    RepositoryVersion,
};
pub use connectors::git_commit_policy::CommitPreflightError;
pub use connectors::git_commit_reader::read_approved_git_commit_object;
pub use connectors::git_commit_snapshot::{
    observe_prepared_git_commit, prepare_git_commit_from_registry, verify_live_git_commit_dispatch,
    GitCommitSnapshotSource,
};
pub use connectors::git_dispatch::{approved_git_diff, approved_git_status};
pub use connectors::git_tree_evidence::{read_approved_git_commit_changes, GitCommitChanges};
pub use connectors::git_tree_plan::{
    prepare_admitted_git_tree, GitCommitTreePlan, PlannedGitObject,
};
pub use connectors::github_dispatch::{
    create_approved_github_file, read_approved_github_file, update_approved_github_file,
};
pub use connectors::intent_binding::{digest_arguments, file_content_sha256};
pub use connectors::local_dispatch::read_approved_file;
pub use connectors::openai_compatible::{
    chat_endpoint, execute_chat, execute_chat_stream, execute_chat_stream_with_progress,
};
pub use connectors::process_dispatch::{
    execute_approved_process_report, execute_approved_process_report_with_control, ProcessReport,
    ProcessRequest, ProcessTermination,
};
pub use connectors::terminal_dispatch::{
    execute_approved_terminal, execute_approved_terminal_process_report_with_control,
    execute_approved_terminal_report, TerminalReport, TerminalTask,
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
