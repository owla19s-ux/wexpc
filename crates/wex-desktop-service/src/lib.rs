//! Authoritative desktop data and basic conversation service. No file/command
//! tools are exposed until their permission and execution adapters are wired.
mod api;
mod model;
mod service;

pub use api::{chat_endpoint, execute_chat};
pub use model::*;
pub use service::DesktopService;
