//! API-PC-01 pure protocol core.
//!
//! No I/O, no transport, no retries, no global or cross-session state.
//! The transport layer feeds raw bytes and owns connection, timeouts, proxy,
//! redirect refusal, cancellation wake-up and backpressure.

pub mod decoder;
pub mod encode;
pub mod errors;
pub mod types;

pub use decoder::Decoder;
pub use encode::{encode_request, EncodeOptions};
pub use errors::{classify_provider_code, error_for_status, provider_error_from_body};
pub use types::*;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Shared, copyable cancellation flag. Cancelling does not itself unblock I/O;
/// the transport adapter must observe it and drop or abort the connection.
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

/// True when the text parses as a JSON object.
pub fn is_json_object(raw: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(raw)
        .map(|value| value.is_object())
        .unwrap_or(false)
}
