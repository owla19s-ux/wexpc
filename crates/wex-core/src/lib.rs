//! Platform-independent, offline runtime semantics. No JSON or FFI here.
pub mod access;
pub mod connection_session;
pub mod operation;
pub mod task;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub protocol_version: u32,
    pub request_id: String,
    pub operation: Operation,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operation {
    Ping,
    Echo(String),
    DemoFail,
    DemoDeny,
    Unknown(String),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Ok,
    Error,
    Denied,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeError {
    pub code: &'static str,
    pub message: &'static str,
    pub retryable: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub protocol_version: u32,
    pub request_id: Option<String>,
    pub status: Status,
    pub value: Option<String>,
    pub error: Option<RuntimeError>,
}

pub fn handle(request: Request) -> Reply {
    let valid_id = !request.request_id.is_empty()
        && request.request_id.len() <= 64
        && request
            .request_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    let request_id = valid_id.then_some(request.request_id);
    if request.protocol_version != 1 {
        return failure(
            request_id,
            Status::Error,
            "UNSUPPORTED_VERSION",
            "Unsupported protocol version",
        );
    }
    if !valid_id {
        return failure(None, Status::Error, "INVALID_REQUEST", "Invalid request ID");
    }
    let value = match request.operation {
        Operation::Ping => "pong".to_owned(),
        Operation::Echo(text) if text.len() <= 4096 => text,
        Operation::Echo(_) => {
            return failure(
                request_id,
                Status::Error,
                "INVALID_REQUEST",
                "Echo exceeds UTF-8 byte limit",
            );
        }
        Operation::DemoFail => {
            return failure(
                request_id,
                Status::Error,
                "DEMO_FAILURE",
                "Deterministic demo failure",
            );
        }
        Operation::DemoDeny => {
            return failure(
                request_id,
                Status::Denied,
                "DEMO_DENIED",
                "Deterministic demo denial",
            );
        }
        Operation::Unknown(_) => {
            return failure(
                request_id,
                Status::Error,
                "UNKNOWN_OPERATION",
                "Unknown operation",
            );
        }
    };
    Reply {
        protocol_version: 1,
        request_id,
        status: Status::Ok,
        value: Some(value),
        error: None,
    }
}

fn failure(
    request_id: Option<String>,
    status: Status,
    code: &'static str,
    message: &'static str,
) -> Reply {
    Reply {
        protocol_version: 1,
        request_id,
        status,
        value: None,
        error: Some(RuntimeError {
            code,
            message,
            retryable: false,
        }),
    }
}
