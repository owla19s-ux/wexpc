//! API-PC-01 protocol types (pure, no I/O).
//!
//! Executable proposal for the pure protocol core: no transport, no reqwest,
//! no global or cross-session state. Secrets are redacted by construction.

use std::fmt;
use std::time::Duration;

pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
pub const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(60);
pub const DEFAULT_TOTAL_TIMEOUT: Duration = Duration::from_secs(300);
pub const DEFAULT_MAX_FRAME_BYTES: usize = 1024 * 1024;
pub const DEFAULT_MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
pub const DEFAULT_MAX_EVENTS_PER_PUSH: usize = 1024;
pub const DEFAULT_MAX_CONTINUATION_BYTES: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// A complete tool call. Fragments never leave the decoder as calls.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// Raw JSON object text; validated as an object before completion.
    pub arguments_json: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub role: Role,
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub tool_call_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters_json: String,
}

/// Secret value owned by the host; never serialized, logged or shown in Debug.
#[derive(Clone, PartialEq, Eq)]
pub struct Credential(String);

impl Credential {
    pub fn new(value: String) -> Self {
        Self(value)
    }
    /// For the trusted authentication adapter only.
    pub fn expose(&self) -> &str {
        &self.0
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Credential([REDACTED])")
    }
}

/// Identity of the request that produced a continuation payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContinuationScope {
    pub connection_id: String,
    pub conversation_id: String,
    pub generation_id: String,
    pub model: String,
    /// Exact ordered set of complete tool call ids from the originating message.
    pub tool_call_ids: Vec<String>,
}

/// Opaque provider payload carried between rounds. Never logged or displayed.
#[derive(Clone, PartialEq, Eq)]
pub struct Continuation {
    origin: ContinuationScope,
    payload: String,
}

impl Continuation {
    pub fn new_bounded(
        origin: ContinuationScope,
        payload: String,
        limit: usize,
    ) -> Result<Self, ApiError> {
        if payload.len() > limit {
            return Err(ApiError::new(
                ErrorKind::LimitExceeded,
                "continuation payload exceeds limit",
            ));
        }
        Ok(Self { origin, payload })
    }

    pub fn new(origin: ContinuationScope, payload: String) -> Result<Self, ApiError> {
        Self::new_bounded(origin, payload, DEFAULT_MAX_CONTINUATION_BYTES)
    }
    pub fn origin(&self) -> &ContinuationScope {
        &self.origin
    }
    /// Only the transport layer should call this to replay the payload verbatim.
    pub fn payload(&self) -> &str {
        &self.payload
    }
    /// Pure comparison. Application owns storage, validity and revocation.
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
pub enum FinishReason {
    Stop,
    ToolCalls,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct Completion {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    pub usage: Option<Usage>,
    pub continuation: Option<Continuation>,
}

impl fmt::Debug for Completion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Completion")
            .field("text_len", &self.text.len())
            .field("tool_calls", &self.tool_calls.len())
            .field("usage", &self.usage)
            .field("continuation", &self.continuation.is_some())
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
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
    /// Bounded Retry-After hint; the decoder never sleeps or retries on its own.
    pub retry_after: Option<Duration>,
    /// Stable, redacted description. Never contains provider body or credentials.
    pub detail: &'static str,
}

impl ApiError {
    pub fn new(kind: ErrorKind, detail: &'static str) -> Self {
        Self {
            kind,
            http_status: None,
            retry_after: None,
            detail,
        }
    }
    pub fn with_status(kind: ErrorKind, status: u16, detail: &'static str) -> Self {
        Self {
            kind,
            http_status: Some(status),
            retry_after: None,
            detail,
        }
    }
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Proxy {
    System,
    Disabled,
    /// Validated at construction; no userinfo, no whitespace, http(s)/socks5 only.
    Explicit(String),
}

impl Proxy {
    pub fn explicit(url: &str) -> Result<Self, ApiError> {
        validate_proxy_url(url)?;
        Ok(Proxy::Explicit(url.to_string()))
    }
}

fn validate_proxy_url(url: &str) -> Result<(), ApiError> {
    let rest = ["http://", "https://", "socks5://", "socks5h://"]
        .iter()
        .find_map(|scheme| url.strip_prefix(scheme));
    let Some(rest) = rest else {
        return Err(ApiError::new(
            ErrorKind::InvalidRequest,
            "proxy scheme not supported",
        ));
    };
    if rest.is_empty() || rest.contains('@') || rest.chars().any(|c| c.is_whitespace()) {
        return Err(ApiError::new(
            ErrorKind::InvalidRequest,
            "proxy address rejected",
        ));
    }
    let host = rest.rsplit('@').next().unwrap_or(rest);
    let host_port = host.split('/').next().unwrap_or(host);
    if host_port.is_empty() || host_port.starts_with(':') {
        return Err(ApiError::new(
            ErrorKind::InvalidRequest,
            "proxy host missing",
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestOptions {
    pub connect_timeout: Duration,
    pub idle_timeout: Duration,
    pub total_timeout: Duration,
    pub proxy: Proxy,
    pub max_frame_bytes: usize,
    pub max_response_bytes: usize,
    pub max_events_per_push: usize,
    pub max_continuation_bytes: usize,
}

impl Default for RequestOptions {
    fn default() -> Self {
        Self {
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            idle_timeout: DEFAULT_IDLE_TIMEOUT,
            total_timeout: DEFAULT_TOTAL_TIMEOUT,
            proxy: Proxy::System,
            max_frame_bytes: DEFAULT_MAX_FRAME_BYTES,
            max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
            max_events_per_push: DEFAULT_MAX_EVENTS_PER_PUSH,
            max_continuation_bytes: DEFAULT_MAX_CONTINUATION_BYTES,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiRequest {
    pub request_id: String,
    pub connection_id: String,
    pub conversation_id: String,
    pub model: String,
    pub endpoint: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDefinition>,
    /// Extra body fields as a JSON object. May not override reserved keys.
    pub parameters_json: String,
    pub continuation: Option<Continuation>,
}

pub const RESERVED_PARAMETER_KEYS: [&str; 7] = [
    "model",
    "messages",
    "tools",
    "stream",
    "authorization",
    "api_key",
    "key",
];

impl ApiRequest {
    /// Structural validation only. Application-layer authority checks stay outside.
    pub fn validate(&self) -> Result<(), ApiError> {
        for (name, value) in [
            ("request_id", &self.request_id),
            ("connection_id", &self.connection_id),
            ("conversation_id", &self.conversation_id),
            ("model", &self.model),
            ("endpoint", &self.endpoint),
        ] {
            if value.trim().is_empty() {
                return Err(ApiError::new(
                    ErrorKind::InvalidRequest,
                    reserved_detail(name),
                ));
            }
        }
        if self.messages.is_empty() {
            return Err(ApiError::new(
                ErrorKind::InvalidRequest,
                "messages must not be empty",
            ));
        }
        for message in &self.messages {
            message.validate()?;
        }
        for tool in &self.tools {
            if tool.name.trim().is_empty() {
                return Err(ApiError::new(
                    ErrorKind::InvalidRequest,
                    "tool name must not be empty",
                ));
            }
            if !is_json_object(&tool.parameters_json) {
                return Err(ApiError::new(
                    ErrorKind::InvalidRequest,
                    "tool parameters must be a JSON object",
                ));
            }
        }
        validate_parameters(&self.parameters_json)?;
        Ok(())
    }
}

fn reserved_detail(field: &'static str) -> &'static str {
    match field {
        "request_id" => "request id must not be empty",
        "connection_id" => "connection id must not be empty",
        "conversation_id" => "conversation id must not be empty",
        "model" => "model must not be empty",
        "endpoint" => "endpoint must not be empty",
        _ => "field must not be empty",
    }
}

pub fn validate_parameters(raw: &str) -> Result<(), ApiError> {
    let value: serde_json::Value = serde_json::from_str(raw)
        .map_err(|_| ApiError::new(ErrorKind::InvalidRequest, "parameters must be valid JSON"))?;
    let object = value.as_object().ok_or_else(|| {
        ApiError::new(
            ErrorKind::InvalidRequest,
            "parameters must be a JSON object",
        )
    })?;
    for key in object.keys() {
        if RESERVED_PARAMETER_KEYS
            .iter()
            .any(|reserved| reserved == key)
        {
            return Err(ApiError::new(
                ErrorKind::InvalidRequest,
                "parameters override a reserved key",
            ));
        }
    }
    Ok(())
}

pub fn is_json_object(raw: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(raw)
        .map(|value| value.is_object())
        .unwrap_or(false)
}

impl Message {
    pub fn validate(&self) -> Result<(), ApiError> {
        match self.role {
            Role::Tool => {
                if self.tool_call_id.as_deref().unwrap_or("").trim().is_empty() {
                    return Err(ApiError::new(
                        ErrorKind::InvalidRequest,
                        "tool message requires tool_call_id",
                    ));
                }
            }
            Role::Assistant => {
                for call in &self.tool_calls {
                    if call.id.trim().is_empty() || call.name.trim().is_empty() {
                        return Err(ApiError::new(
                            ErrorKind::InvalidRequest,
                            "assistant tool call requires id and name",
                        ));
                    }
                }
            }
            Role::System | Role::User => {
                if !self.tool_calls.is_empty() || self.tool_call_id.is_some() {
                    return Err(ApiError::new(
                        ErrorKind::InvalidRequest,
                        "text role carries tool fields",
                    ));
                }
            }
        }
        Ok(())
    }
}
