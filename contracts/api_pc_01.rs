//! Executable API-PC-01 type proposal; not an adopted transport implementation.
//! Standalone std-only source until workspace and executor decisions are resolved.
use std::{
    fmt,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

#[derive(Clone, Default, Debug)]
pub struct Cancellation(Arc<AtomicBool>);
impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

pub struct Credential(String);
impl Credential {
    pub fn new(value: String) -> Self {
        Self(value)
    }
    /// For the trusted authentication adapter only; never log or serialize.
    pub fn expose(&self) -> &str {
        &self.0
    }
}
impl fmt::Debug for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Credential([REDACTED])")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContinuationScope {
    pub connection_id: String,
    pub conversation_id: String,
    /// Originating generation, not the ID of the next request.
    pub generation_id: String,
    pub model: String,
    /// Exact, ordered complete set from the originating assistant message.
    pub tool_call_ids: Vec<String>,
}
pub struct Continuation {
    pub origin: ContinuationScope,
    pub payload: String,
}
impl Continuation {
    /// Pure comparison only. Application owns storage, validity and revocation.
    pub fn matches_origin(&self, expected: &ContinuationScope) -> bool {
        self.origin == *expected
    }
}
impl fmt::Debug for Continuation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Continuation")
            .field("origin", &self.origin)
            .field("payload", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}
#[derive(Clone, Debug)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// Raw JSON object text; executor must parse/validate before completion.
    pub arguments_json: String,
}
#[derive(Clone, Debug)]
pub struct Message {
    pub role: Role,
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub tool_call_id: Option<String>,
}
#[derive(Clone, Debug)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters_json: String,
}
pub struct ApiRequest {
    pub request_id: String,
    pub connection_id: String,
    pub conversation_id: String,
    pub model: String,
    pub endpoint: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDefinition>,
    pub parameters_json: String,
    /// Explicit caller-provided data, never a module-global session cache.
    pub continuation: Option<Continuation>,
}
#[derive(Clone, Debug)]
pub enum Proxy {
    System,
    Disabled,
    Explicit(String),
}
#[derive(Clone, Debug)]
pub struct RequestOptions {
    pub connect_timeout: Duration,
    pub idle_timeout: Duration,
    pub total_timeout: Duration,
    pub proxy: Proxy,
    pub max_frame_bytes: usize,
    pub max_response_bytes: usize,
    pub event_capacity: usize,
}
#[derive(Clone, Debug)]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}
pub struct Completion {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    pub usage: Option<Usage>,
    pub continuation: Option<Continuation>,
}
pub enum Event {
    TextDelta(String),
    Usage(Usage),
    Completed(Completion),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    InvalidRequest,
    CredentialUnavailable,
    Authentication,
    PermissionDenied,
    RateLimited,
    ProviderUnavailable,
    Network,
    ConnectTimeout,
    IdleTimeout,
    TotalTimeout,
    Cancelled,
    Protocol,
    LimitExceeded,
    UnsupportedParameter,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApiError {
    pub kind: ErrorKind,
    pub http_status: Option<u16>,
}
impl ApiError {
    pub fn code(&self) -> &'static str {
        match self.kind {
            ErrorKind::InvalidRequest => "INVALID_REQUEST",
            ErrorKind::CredentialUnavailable => "CREDENTIAL_UNAVAILABLE",
            ErrorKind::Authentication => "AUTHENTICATION",
            ErrorKind::PermissionDenied => "PERMISSION_DENIED",
            ErrorKind::RateLimited => "RATE_LIMITED",
            ErrorKind::ProviderUnavailable => "PROVIDER_UNAVAILABLE",
            ErrorKind::Network => "NETWORK",
            ErrorKind::ConnectTimeout => "CONNECT_TIMEOUT",
            ErrorKind::IdleTimeout => "IDLE_TIMEOUT",
            ErrorKind::TotalTimeout => "TOTAL_TIMEOUT",
            ErrorKind::Cancelled => "CANCELLED",
            ErrorKind::Protocol => "PROTOCOL",
            ErrorKind::LimitExceeded => "LIMIT_EXCEEDED",
            ErrorKind::UnsupportedParameter => "UNSUPPORTED_PARAMETER",
        }
    }
}
