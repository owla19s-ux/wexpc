//! Pure SSE/OpenAI-compatible protocol decoder. No I/O, no retries, no state
//! outside this value. Feed raw body bytes; consume typed events.

use std::collections::BTreeMap;

use crate::types::*;
use crate::Cancellation;

#[derive(Default)]
struct PartialTool {
    id: String,
    name: String,
    arguments: String,
}

pub struct Decoder {
    options: RequestOptions,
    scope: Option<ContinuationScope>,
    cancellation: Option<Cancellation>,
    line: Vec<u8>,
    data: String,
    text: String,
    reasoning: String,
    tools: BTreeMap<usize, PartialTool>,
    order: Vec<usize>,
    usage: Option<Usage>,
    finish: Option<FinishReason>,
    completion: Option<Completion>,
    done_seen: bool,
    total_bytes: usize,
    terminal: bool,
    completed_sent: bool,
}

impl Decoder {
    pub fn new(options: RequestOptions) -> Self {
        Self {
            options,
            scope: None,
            cancellation: None,
            line: Vec::new(),
            data: String::new(),
            text: String::new(),
            reasoning: String::new(),
            tools: BTreeMap::new(),
            order: Vec::new(),
            usage: None,
            finish: None,
            completion: None,
            done_seen: false,
            total_bytes: 0,
            terminal: false,
            completed_sent: false,
        }
    }

    /// Continuation scope is required when the provider returns reasoning payload.
    pub fn with_continuation_scope(mut self, scope: ContinuationScope) -> Self {
        self.scope = Some(scope);
        self
    }

    pub fn with_cancellation(mut self, cancellation: Cancellation) -> Self {
        self.cancellation = Some(cancellation);
        self
    }

    /// Feed raw body bytes. Boundaries may fall anywhere, including mid-UTF-8.
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<Event>, ApiError> {
        if self.is_cancelled() {
            return Err(ApiError::new(ErrorKind::Cancelled, "request cancelled"));
        }
        self.total_bytes = self.total_bytes.saturating_add(chunk.len());
        if self.total_bytes > self.options.max_response_bytes {
            return Err(ApiError::new(
                ErrorKind::LimitExceeded,
                "response size limit exceeded",
            ));
        }
        self.line.extend_from_slice(chunk);
        let mut events = Vec::new();
        loop {
            let Some(pos) = self.line.iter().position(|byte| *byte == b'\n') else {
                if self.line.len() > self.options.max_frame_bytes {
                    return Err(ApiError::new(
                        ErrorKind::LimitExceeded,
                        "frame size limit exceeded",
                    ));
                }
                break;
            };
            let raw: Vec<u8> = self.line.drain(..=pos).collect();
            let body = &raw[..raw.len() - 1];
            let body = body.strip_suffix(b"\r").unwrap_or(body);
            self.handle_line(body, &mut events)?;
            if events.len() > self.options.max_events_per_push {
                return Err(ApiError::new(
                    ErrorKind::LimitExceeded,
                    "event batch limit exceeded",
                ));
            }
        }
        Ok(events)
    }

    /// Ends the stream. A valid terminal frame is mandatory; EOF alone is not success.
    pub fn finish(mut self) -> Result<Completion, ApiError> {
        if !self.terminal {
            return Err(ApiError::new(
                ErrorKind::Protocol,
                "stream ended without terminal frame",
            ));
        }
        self.completion
            .take()
            .ok_or_else(|| ApiError::new(ErrorKind::Protocol, "completion missing"))
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    fn is_cancelled(&self) -> bool {
        self.cancellation
            .as_ref()
            .map(Cancellation::is_cancelled)
            .unwrap_or(false)
    }

    fn handle_line(&mut self, raw: &[u8], events: &mut Vec<Event>) -> Result<(), ApiError> {
        if raw.is_empty() {
            return self.dispatch(events);
        }
        if raw.starts_with(b":") {
            return Ok(());
        }
        let Some(rest) = raw.strip_prefix(b"data:") else {
            return Ok(());
        };
        let rest = rest.strip_prefix(b" ").unwrap_or(rest);
        let text = std::str::from_utf8(rest)
            .map_err(|_| ApiError::new(ErrorKind::Protocol, "invalid UTF-8 in data line"))?;
        if !self.data.is_empty() {
            self.data.push('\n');
        }
        self.data.push_str(text);
        if self.data.len() > self.options.max_frame_bytes {
            return Err(ApiError::new(
                ErrorKind::LimitExceeded,
                "frame data limit exceeded",
            ));
        }
        Ok(())
    }

    fn dispatch(&mut self, events: &mut Vec<Event>) -> Result<(), ApiError> {
        if self.data.is_empty() {
            return Ok(());
        }
        let payload = std::mem::take(&mut self.data);
        if payload.trim() == "[DONE]" {
            if self.done_seen {
                return Err(ApiError::new(
                    ErrorKind::Protocol,
                    "duplicate stream terminator",
                ));
            }
            self.done_seen = true;
            return Ok(());
        }
        if self.done_seen {
            return Err(ApiError::new(
                ErrorKind::Protocol,
                "frame after stream terminator",
            ));
        }
        let value: serde_json::Value = serde_json::from_str(&payload)
            .map_err(|_| ApiError::new(ErrorKind::Protocol, "invalid JSON frame"))?;
        let object = value
            .as_object()
            .ok_or_else(|| ApiError::new(ErrorKind::Protocol, "frame is not an object"))?;

        // Usage tail frames are permitted after the terminal frame (include_usage).
        if let Some(usage) = object.get("usage").and_then(parse_usage) {
            self.usage = Some(usage.clone());
            if let Some(completion) = self.completion.as_mut() {
                completion.usage = Some(usage.clone());
            }
            events.push(Event::Usage(usage));
        }

        let choices = object
            .get("choices")
            .and_then(|value| value.as_array())
            .ok_or_else(|| ApiError::new(ErrorKind::Protocol, "frame without choices"))?;
        let mut primary_choice_seen = false;
        for choice in choices {
            let object = choice
                .as_object()
                .ok_or_else(|| ApiError::new(ErrorKind::Protocol, "choice is not an object"))?;
            let index = object
                .get("index")
                .and_then(|value| value.as_u64())
                .unwrap_or(0);
            if index != 0 {
                continue;
            }
            if primary_choice_seen {
                return Err(ApiError::new(
                    ErrorKind::Protocol,
                    "duplicate primary choice in one frame",
                ));
            }
            primary_choice_seen = true;
            let delta = object.get("delta").filter(|delta| {
                delta
                    .as_object()
                    .map(|object| !object.is_empty())
                    .unwrap_or(false)
            });
            let finish = object
                .get("finish_reason")
                .filter(|value| !value.is_null())
                .and_then(|value| value.as_str());
            if self.terminal {
                if let Some(reason) = finish {
                    let parsed = parse_finish_reason(reason)?;
                    return Err(ApiError::new(
                        ErrorKind::Protocol,
                        if Some(parsed) == self.finish {
                            "duplicate terminal frame"
                        } else {
                            "conflicting terminal frame"
                        },
                    ));
                }
                if delta.is_some() {
                    return Err(ApiError::new(
                        ErrorKind::Protocol,
                        "frame after terminal event",
                    ));
                }
                continue;
            }
            if let Some(delta) = delta {
                self.apply_delta(delta, events)?;
            }
            if let Some(reason) = finish {
                self.apply_finish(reason)?;
            }
        }
        if self.terminal && !self.completed_sent {
            if let Some(completion) = self.completion.clone() {
                events.push(Event::Completed(completion));
                self.completed_sent = true;
            }
        }
        Ok(())
    }

    fn apply_delta(
        &mut self,
        delta: &serde_json::Value,
        events: &mut Vec<Event>,
    ) -> Result<(), ApiError> {
        let object = delta
            .as_object()
            .ok_or_else(|| ApiError::new(ErrorKind::Protocol, "delta is not an object"))?;
        if let Some(role) = object.get("role").and_then(|value| value.as_str()) {
            if role != "assistant" {
                return Err(ApiError::new(ErrorKind::Protocol, "unexpected delta role"));
            }
        }
        if let Some(content) = object.get("content").and_then(|value| value.as_str()) {
            if !content.is_empty() {
                self.text.push_str(content);
                events.push(Event::TextDelta(content.to_string()));
            }
        }
        if let Some(reasoning) = object
            .get("reasoning_content")
            .and_then(|value| value.as_str())
        {
            self.reasoning.push_str(reasoning);
            if self.reasoning.len() > self.options.max_continuation_bytes {
                return Err(ApiError::new(
                    ErrorKind::LimitExceeded,
                    "reasoning payload limit exceeded",
                ));
            }
        }
        if let Some(calls) = object.get("tool_calls").and_then(|value| value.as_array()) {
            for call in calls {
                self.apply_tool_call(call)?;
            }
        }
        Ok(())
    }

    fn apply_tool_call(&mut self, call: &serde_json::Value) -> Result<(), ApiError> {
        let object = call
            .as_object()
            .ok_or_else(|| ApiError::new(ErrorKind::Protocol, "tool call is not an object"))?;
        let index = object
            .get("index")
            .and_then(|value| value.as_u64())
            .ok_or_else(|| ApiError::new(ErrorKind::Protocol, "tool call without index"))?;
        let index = usize::try_from(index)
            .map_err(|_| ApiError::new(ErrorKind::Protocol, "tool index out of range"))?;
        if !self.tools.contains_key(&index) {
            if index != self.order.len() {
                return Err(ApiError::new(ErrorKind::Protocol, "tool call index gap"));
            }
            if self.order.len() >= 64 {
                return Err(ApiError::new(
                    ErrorKind::LimitExceeded,
                    "tool call count limit exceeded",
                ));
            }
            self.order.push(index);
            self.tools.insert(index, PartialTool::default());
        }
        let entry = self.tools.get_mut(&index).expect("inserted");
        if let Some(id) = object.get("id").and_then(|value| value.as_str()) {
            if !entry.id.is_empty() && entry.id != id {
                return Err(ApiError::new(
                    ErrorKind::Protocol,
                    "tool call id changed mid-stream",
                ));
            }
            entry.id = id.to_string();
        }
        if let Some(function) = object.get("function") {
            let function = function
                .as_object()
                .ok_or_else(|| ApiError::new(ErrorKind::Protocol, "function is not an object"))?;
            if let Some(name) = function.get("name").and_then(|value| value.as_str()) {
                if !entry.name.is_empty() && entry.name != name {
                    return Err(ApiError::new(
                        ErrorKind::Protocol,
                        "tool name changed mid-stream",
                    ));
                }
                entry.name = name.to_string();
            }
            if let Some(arguments) = function.get("arguments").and_then(|value| value.as_str()) {
                entry.arguments.push_str(arguments);
                if entry.arguments.len() > self.options.max_frame_bytes {
                    return Err(ApiError::new(
                        ErrorKind::LimitExceeded,
                        "tool arguments limit exceeded",
                    ));
                }
            }
        }
        Ok(())
    }

    fn apply_finish(&mut self, reason: &str) -> Result<(), ApiError> {
        let parsed = parse_finish_reason(reason)?;
        self.finish = Some(parsed);
        self.terminal = true;
        let completion = self.build_completion(parsed)?;
        self.completion = Some(completion);
        Ok(())
    }

    fn build_completion(&self, reason: FinishReason) -> Result<Completion, ApiError> {
        let mut tool_calls = Vec::with_capacity(self.order.len());
        if reason == FinishReason::Stop && !self.order.is_empty() {
            return Err(ApiError::new(
                ErrorKind::Protocol,
                "stop with pending tool calls",
            ));
        }
        if reason == FinishReason::ToolCalls && self.order.is_empty() {
            return Err(ApiError::new(
                ErrorKind::Protocol,
                "tool_calls without calls",
            ));
        }
        let mut seen_ids = std::collections::HashSet::new();
        for index in &self.order {
            let partial = self.tools.get(index).expect("indexed");
            if partial.id.trim().is_empty() || partial.name.trim().is_empty() {
                return Err(ApiError::new(ErrorKind::Protocol, "incomplete tool call"));
            }
            if !seen_ids.insert(partial.id.as_str()) {
                return Err(ApiError::new(ErrorKind::Protocol, "duplicate tool call id"));
            }
            if !crate::is_json_object(&partial.arguments) {
                return Err(ApiError::new(
                    ErrorKind::Protocol,
                    "tool arguments are not a JSON object",
                ));
            }
            tool_calls.push(ToolCall {
                id: partial.id.clone(),
                name: partial.name.clone(),
                arguments_json: partial.arguments.clone(),
            });
        }
        let continuation = if self.reasoning.is_empty() {
            None
        } else {
            let scope = self.scope.clone().ok_or_else(|| {
                ApiError::new(ErrorKind::Protocol, "reasoning without continuation scope")
            })?;
            Some(Continuation::new_bounded(
                scope,
                self.reasoning.clone(),
                self.options.max_continuation_bytes,
            )?)
        };
        Ok(Completion {
            text: self.text.clone(),
            tool_calls,
            usage: self.usage.clone(),
            continuation,
        })
    }
}

fn parse_finish_reason(reason: &str) -> Result<FinishReason, ApiError> {
    match reason {
        "stop" => Ok(FinishReason::Stop),
        "tool_calls" => Ok(FinishReason::ToolCalls),
        "length" | "content_filter" => Err(ApiError::new(
            ErrorKind::Protocol,
            "truncated stream is not success",
        )),
        _ => Err(ApiError::new(ErrorKind::Protocol, "unknown finish reason")),
    }
}

fn parse_usage(value: &serde_json::Value) -> Option<Usage> {
    let object = value.as_object()?;
    Some(Usage {
        input_tokens: object.get("prompt_tokens").and_then(|value| value.as_u64()),
        output_tokens: object
            .get("completion_tokens")
            .and_then(|value| value.as_u64()),
    })
}
