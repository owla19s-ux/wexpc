//! Strict, bounded JSON boundary. Business behavior remains in wex-core.
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::{json, Map, Value};
use std::fmt;
use wex_core::{handle, Operation, Request, Status};

struct StrictValue(Value);
struct StrictVisitor;

impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(StrictVisitor)
    }
}
impl<'de> Visitor<'de> for StrictVisitor {
    type Value = StrictValue;
    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("JSON without duplicate object keys")
    }
    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Bool(value)))
    }
    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
        Ok(StrictValue(value.into()))
    }
    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
        Ok(StrictValue(value.into()))
    }
    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
        serde_json::Number::from_f64(value)
            .map(|number| StrictValue(Value::Number(number)))
            .ok_or_else(|| E::custom("non-finite number"))
    }
    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::String(value.to_owned())))
    }
    fn visit_string<E: de::Error>(self, value: String) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::String(value)))
    }
    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Null))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let mut values = Vec::new();
        while let Some(StrictValue(value)) = seq.next_element()? {
            values.push(value);
        }
        Ok(StrictValue(Value::Array(values)))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut values = Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if values.contains_key(&key) {
                return Err(de::Error::custom("duplicate JSON key"));
            }
            let StrictValue(value) = map.next_value()?;
            values.insert(key, value);
        }
        Ok(StrictValue(Value::Object(values)))
    }
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
fn invalid(id: Option<&str>, code: &str) -> Value {
    json!({
        "protocol_version":1, "request_id":id, "status":"error", "data":null,
        "error":{"code":code,"message":"Invalid runtime request","retryable":false}
    })
}
fn response(input: &[u8]) -> Value {
    if input.is_empty() || input.len() > 16384 {
        return invalid(None, "INVALID_REQUEST");
    }
    let Ok(StrictValue(Value::Object(fields))) = serde_json::from_slice(input) else {
        return invalid(None, "INVALID_REQUEST");
    };
    let id = fields
        .get("request_id")
        .and_then(Value::as_str)
        .filter(|id| valid_id(id));
    let Some(version) = fields.get("protocol_version") else {
        return invalid(id, "INVALID_REQUEST");
    };
    if !(version.is_i64() || version.is_u64()) {
        return invalid(id, "INVALID_REQUEST");
    }
    if version.as_u64() != Some(1) {
        return invalid(id, "UNSUPPORTED_VERSION");
    }
    let Some(id) = id else {
        return invalid(None, "INVALID_REQUEST");
    };
    let Some(operation) = fields.get("operation").and_then(Value::as_str) else {
        return invalid(Some(id), "INVALID_REQUEST");
    };
    if !["ping", "echo", "demo_fail", "demo_deny"].contains(&operation) {
        return invalid(Some(id), "UNKNOWN_OPERATION");
    }
    if fields.len() != 4
        || fields.keys().any(|key| {
            !["protocol_version", "request_id", "operation", "payload"].contains(&key.as_str())
        })
    {
        return invalid(Some(id), "INVALID_REQUEST");
    }
    let Some(payload) = fields.get("payload").and_then(Value::as_object) else {
        return invalid(Some(id), "INVALID_REQUEST");
    };
    let operation = match operation {
        "echo" => {
            let Some(text) = payload.get("text").and_then(Value::as_str) else {
                return invalid(Some(id), "INVALID_REQUEST");
            };
            if payload.len() != 1 {
                return invalid(Some(id), "INVALID_REQUEST");
            }
            Operation::Echo(text.to_owned())
        }
        other => {
            if !payload.is_empty() {
                return invalid(Some(id), "INVALID_REQUEST");
            }
            match other {
                "ping" => Operation::Ping,
                "demo_fail" => Operation::DemoFail,
                _ => Operation::DemoDeny,
            }
        }
    };
    let reply = handle(Request {
        protocol_version: 1,
        request_id: id.into(),
        operation,
    });
    let status = match reply.status {
        Status::Ok => "ok",
        Status::Error => "error",
        Status::Denied => "denied",
    };
    let data = reply.value.map(|value| json!({"value":value}));
    let error = reply.error.map(|error| {
        json!({
            "code":error.code,"message":error.message,"retryable":error.retryable
        })
    });
    json!({
        "protocol_version":reply.protocol_version,"request_id":reply.request_id,
        "status":status,"data":data,"error":error
    })
}

/// Convert at most 16 KiB of UTF-8 JSON into a runtime reply.
/// No I/O, platform access, or native pointers are involved.
pub fn process_json(input: &[u8]) -> Vec<u8> {
    // Value contains only JSON-representable primitives; serialization cannot
    // fail for a finite Value tree produced by this bounded request handler.
    serde_json::to_vec(&response(input)).expect("serialize JSON runtime reply")
}
