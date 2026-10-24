//! Authoritative desktop data and basic conversation service. No file/command
//! tools are exposed until their permission and execution adapters are wired.
mod application;
mod connectors;
mod domain;
mod infrastructure;

pub use application::DesktopService;
pub use connectors::openai_compatible::{chat_endpoint, execute_chat, execute_chat_stream};
pub use domain::*;
