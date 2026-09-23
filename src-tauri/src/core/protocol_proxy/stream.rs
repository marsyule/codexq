//! Streaming translation from Chat Completions SSE chunks to Responses SSE events.
//!
//! Codex always streams, so this converter is on the critical path for every turn.
//! Two properties matter more than event volume:
//!
//! 1. **Exactly one terminal event.** `response.completed` (or `response.failed`) is
//!    emitted once and nothing may follow it. A duplicate or missing terminal event
//!    leaves the Codex client waiting or double-counting the turn.
//! 2. **No event about an item that was never announced.** `output_item.added`
//!    precedes every delta, and a `function_call` item is only announced once its
//!    name is known — a function call announced with an empty name is unusable.
//!
//! Field mapping is delegated to [`super::translate`] so the streamed and
//! non-streamed response objects cannot drift apart.

use serde_json::{json, Map, Value};

use super::translate::{self, flatten_text, item_id, map_usage, response_status};

/// Per-tool-call accumulation state.
#[derive(Debug, Default)]
struct ToolCallState {
    output_index: usize,
    call_id: String,
    name: String,
    arguments: String,
    /// Number of `arguments` bytes already forwarded as deltas.
    flushed: usize,
    /// Whether `output_item.added` has been emitted for this call.
    announced: bool,
}

/// Stateful Chat Completions SSE → Responses SSE converter.
#[derive(Debug)]
pub struct ChatSseToResponsesConverter {
    response_id: String,
    model: String,
    created_at: i64,
    sequence: u64,
    started: bool,
    finished: bool,
    message_open: bool,
    message_output_index: usize,
    text: String,
    reasoning_open: bool,
    reasoning_output_index: usize,
    reasoning: String,
    next_output_index: usize,
    tool_calls: Vec<ToolCallState>,
    finish_reason: Option<String>,
    usage: Option<Value>,
    line_buffer: Vec<u8>,
}

impl ChatSseToResponsesConverter {
    /// Creates a converter for one upstream turn.
    ///
    /// # Arguments
    ///
    /// * `response_id` - Responses id already generated for this turn.
    /// * `model` - Model name echoed back to Codex.
    /// * `created_at` - Unix seconds for the `created_at` field.
    #[must_use]
    pub fn new(response_id: String, model: String, created_at: i64) -> Self {
        Self {
            response_id,
            model,
            created_at,
            sequence: 0,
            started: false,
            finished: false,
            message_open: false,
            message_output_index: 0,
            text: String::new(),
            reasoning_open: false,
            reasoning_output_index: 0,
            reasoning: String::new(),
            next_output_index: 0,
            tool_calls: Vec::new(),
            finish_reason: None,
            usage: None,
            line_buffer: Vec::new(),
        }
    }

    /// Emits the opening lifecycle events, before any upstream byte arrives.
    ///
    /// Emitting these eagerly lets Codex observe a response id as soon as the upstream
    /// accepted the request, instead of only after the first token.
    pub fn begin(&mut self) -> Vec<String> {
        if self.started || self.finished {
            return Vec::new();
        }
        self.started = true;
        let snapshot = self.snapshot("in_progress");
        vec![
            self.emit("response.created", json!({ "response": snapshot.clone() })),
            self.emit("response.in_progress", json!({ "response": snapshot })),
        ]
    }

    /// Consumes a raw chunk of upstream SSE text and returns the events to forward.
    ///
    /// Chunks are not aligned to SSE frame boundaries, so complete lines are drained
    /// from an internal buffer and the trailing partial line is retained.
    pub fn feed(&mut self, chunk: &str) -> Vec<String> {
        self.feed_bytes(chunk.as_bytes())
    }

    /// Consumes raw network bytes, retaining incomplete UTF-8 characters until a full line arrives.
    pub fn feed_bytes(&mut self, chunk: &[u8]) -> Vec<String> {
        let mut frames = Vec::new();
        if self.finished {
            return frames;
        }
        self.line_buffer.extend_from_slice(chunk);

        while let Some(newline) = self.line_buffer.iter().position(|byte| *byte == b'\n') {
            let line = String::from_utf8_lossy(&self.line_buffer[..=newline]).into_owned();
            self.line_buffer.drain(..=newline);
            let line = line.trim_end_matches(['\n', '\r']).trim();
            if line.is_empty() || line.starts_with(':') {
                continue;
            }
            let Some(payload) = line.strip_prefix("data:") else {
                // `event:` / `id:` / `retry:` lines carry no information we need.
                continue;
            };
            let payload = payload.trim();
            if payload == "[DONE]" {
                frames.extend(self.finish());
                return frames;
            }
            let Ok(value) = serde_json::from_str::<Value>(payload) else {
                log::debug!("protocol gateway: skipping unparseable upstream SSE payload");
                continue;
            };
            frames.extend(self.consume(&value));
        }
        frames
    }

    /// Emits the closing events: item completions followed by the terminal event.
    ///
    /// Safe to call more than once — only the first call produces output.
    pub fn finish(&mut self) -> Vec<String> {
        if self.finished {
            return Vec::new();
        }
        self.finished = true;

        let finish_reason = self.finish_reason.clone().unwrap_or_else(|| {
            if self.tool_calls.is_empty() {
                "stop".to_string()
            } else {
                "tool_calls".to_string()
            }
        });
        let status = response_status(&finish_reason);
        let mut frames = Vec::new();

        if self.reasoning_open {
            let reasoning_item_id = item_id("rs", &self.response_id, self.reasoning_output_index);
            let output_index = self.reasoning_output_index;
            let text = self.reasoning.clone();
            frames.push(self.emit(
                "response.reasoning_summary_text.done",
                json!({
                    "item_id": reasoning_item_id.clone(),
                    "output_index": output_index,
                    "summary_index": 0,
                    "text": text,
                }),
            ));
            frames.push(self.emit(
                "response.reasoning_summary_part.done",
                json!({
                    "item_id": reasoning_item_id.clone(),
                    "output_index": output_index,
                    "summary_index": 0,
                    "part": { "type": "summary_text", "text": text },
                }),
            ));
            frames.push(self.emit(
                "response.output_item.done",
                json!({
                    "output_index": output_index,
                    "item": translate::reasoning_item(
                        &self.response_id,
                        &text,
                        output_index,
                    ),
                }),
            ));
        }

        if self.message_open {
            let message_item_id = item_id("msg", &self.response_id, self.message_output_index);
            let text = self.text.clone();
            let output_index = self.message_output_index;
            frames.push(self.emit(
                "response.output_text.done",
                json!({
                    "item_id": message_item_id.clone(),
                    "output_index": output_index,
                    "content_index": 0,
                    "text": text,
                }),
            ));
            frames.push(self.emit(
                "response.content_part.done",
                json!({
                    "item_id": message_item_id,
                    "output_index": output_index,
                    "content_index": 0,
                    "part": { "type": "output_text", "text": text, "annotations": [] },
                }),
            ));
            frames.push(self.emit(
                "response.output_item.done",
                json!({
                    "output_index": output_index,
                    "item": translate::message_item(&self.response_id, &text, output_index),
                }),
            ));
        }

        // Collect first: emitting requires `&mut self`, so the tool-call slice cannot be
        // borrowed across the calls.
        let announced: Vec<(usize, String, String, String, String)> = self
            .tool_calls
            .iter()
            .filter(|tool_call| tool_call.announced)
            .map(|tool_call| {
                (
                    tool_call.output_index,
                    item_id("fc", &tool_call.call_id, tool_call.output_index),
                    tool_call.call_id.clone(),
                    tool_call.name.clone(),
                    tool_call.arguments.clone(),
                )
            })
            .collect();

        for (output_index, tool_item_id, call_id, name, arguments) in announced {
            frames.push(self.emit(
                "response.function_call_arguments.done",
                json!({
                    "item_id": tool_item_id.clone(),
                    "output_index": output_index,
                    "arguments": arguments,
                }),
            ));
            frames.push(self.emit(
                "response.output_item.done",
                json!({
                    "output_index": output_index,
                    "item": {
                        "type": "function_call",
                        "id": tool_item_id,
                        "call_id": call_id,
                        "name": name,
                        "arguments": arguments,
                        "status": "completed",
                    },
                }),
            ));
        }

        if status == "failed" {
            let snapshot = self.snapshot_with("failed", &finish_reason, false);
            frames.push(self.emit("response.failed", json!({ "response": snapshot })));
        } else {
            let snapshot = self.snapshot_with(status, &finish_reason, true);
            frames.push(self.emit("response.completed", json!({ "response": snapshot })));
        }

        frames
    }

    /// Emits a terminal `response.failed` event for a transport-level failure.
    ///
    /// Used when the upstream connection drops mid-stream, so the Codex client sees a
    /// clean terminal event instead of a silently truncated response.
    pub fn fail(&mut self, message: &str) -> Vec<String> {
        if self.finished {
            return Vec::new();
        }
        self.finished = true;
        let mut snapshot = self.snapshot_with("failed", "error", false);
        if let Some(object) = snapshot.as_object_mut() {
            object.insert(
                "error".to_string(),
                json!({ "code": "upstream_stream_error", "message": message }),
            );
        }
        vec![self.emit("response.failed", json!({ "response": snapshot }))]
    }

    /// Applies one decoded upstream chunk to the converter state.
    fn consume(&mut self, chunk: &Value) -> Vec<String> {
        let mut frames = Vec::new();

        if self.model.is_empty() {
            if let Some(model) = chunk.get("model").and_then(Value::as_str) {
                self.model = model.to_string();
            }
        }
        if let Some(usage) = chunk.get("usage") {
            if !usage.is_null() {
                self.usage = Some(usage.clone());
            }
        }

        let Some(choice) = chunk
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
        else {
            return frames;
        };

        if let Some(delta) = choice.get("delta") {
            let content = delta
                .get("content")
                .and_then(flatten_text)
                .unwrap_or_default();
            if !content.is_empty() {
                frames.extend(self.open_message());
                self.text.push_str(&content);
                let message_item_id = item_id("msg", &self.response_id, self.message_output_index);
                let output_index = self.message_output_index;
                frames.push(self.emit(
                    "response.output_text.delta",
                    json!({
                        "item_id": message_item_id,
                        "output_index": output_index,
                        "content_index": 0,
                        "delta": content,
                    }),
                ));
            }

            let reasoning = delta
                .get("reasoning_content")
                .or_else(|| delta.get("reasoning"))
                .and_then(flatten_text)
                .unwrap_or_default();
            if !reasoning.is_empty() {
                frames.extend(self.open_reasoning());
                self.reasoning.push_str(&reasoning);
                let reasoning_item_id =
                    item_id("rs", &self.response_id, self.reasoning_output_index);
                let output_index = self.reasoning_output_index;
                frames.push(self.emit(
                    "response.reasoning_summary_text.delta",
                    json!({
                        "item_id": reasoning_item_id,
                        "output_index": output_index,
                        "summary_index": 0,
                        "delta": reasoning,
                    }),
                ));
            }

            if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
                for tool_call in tool_calls {
                    frames.extend(self.consume_tool_call_delta(tool_call));
                }
            }
        }

        if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
            self.finish_reason = Some(reason.to_string());
        }

        frames
    }

    /// Ensures the assistant text item is announced exactly once.
    fn open_message(&mut self) -> Vec<String> {
        if self.message_open {
            return Vec::new();
        }
        self.message_open = true;
        self.message_output_index = self.next_output_index;
        self.next_output_index += 1;

        let message_item_id = item_id("msg", &self.response_id, self.message_output_index);
        let output_index = self.message_output_index;
        vec![
            self.emit(
                "response.output_item.added",
                json!({
                    "output_index": output_index,
                    "item": {
                        "type": "message",
                        "id": message_item_id.clone(),
                        "status": "in_progress",
                        "role": "assistant",
                        "content": [],
                    },
                }),
            ),
            self.emit(
                "response.content_part.added",
                json!({
                    "item_id": message_item_id,
                    "output_index": output_index,
                    "content_index": 0,
                    "part": { "type": "output_text", "text": "", "annotations": [] },
                }),
            ),
        ]
    }

    /// Ensures the reasoning item is announced exactly once.
    fn open_reasoning(&mut self) -> Vec<String> {
        if self.reasoning_open {
            return Vec::new();
        }
        self.reasoning_open = true;
        self.reasoning_output_index = self.next_output_index;
        self.next_output_index += 1;

        let reasoning_item_id = item_id("rs", &self.response_id, self.reasoning_output_index);
        let output_index = self.reasoning_output_index;
        vec![
            self.emit(
                "response.output_item.added",
                json!({
                    "output_index": output_index,
                    "item": {
                        "type": "reasoning",
                        "id": reasoning_item_id.clone(),
                        "summary": [],
                        "content": [],
                        "encrypted_content": null,
                    },
                }),
            ),
            self.emit(
                "response.reasoning_summary_part.added",
                json!({
                    "item_id": reasoning_item_id,
                    "output_index": output_index,
                    "summary_index": 0,
                    "part": { "type": "summary_text", "text": "" },
                }),
            ),
        ]
    }
    /// Applies an incremental tool-call delta for one choice.
    fn consume_tool_call_delta(&mut self, delta: &Value) -> Vec<String> {
        let Some(object) = delta.as_object() else {
            return Vec::new();
        };
        let position = object
            .get("index")
            .and_then(Value::as_u64)
            .map(|index| index as usize)
            .unwrap_or(self.tool_calls.len());

        while self.tool_calls.len() <= position {
            let output_index = self.next_output_index;
            self.next_output_index += 1;
            self.tool_calls.push(ToolCallState {
                output_index,
                ..ToolCallState::default()
            });
        }

        if let Some(call_id) = object
            .get("id")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
        {
            self.tool_calls[position].call_id = call_id.to_string();
        }

        let function = object.get("function");
        if let Some(name) = function
            .and_then(|function| function.get("name"))
            .and_then(Value::as_str)
        {
            self.tool_calls[position].name.push_str(name);
        }
        if let Some(arguments) = function
            .and_then(|function| function.get("arguments"))
            .and_then(Value::as_str)
        {
            self.tool_calls[position].arguments.push_str(arguments);
        }

        // A function call is only announced once its name is known: an item published
        // with an empty name cannot be repaired by later deltas.
        let should_announce =
            !self.tool_calls[position].announced && !self.tool_calls[position].name.is_empty();
        if should_announce {
            self.tool_calls[position].announced = true;
            if self.tool_calls[position].call_id.is_empty() {
                self.tool_calls[position].call_id = format!("call_{position}");
            }
        }

        let mut frames = Vec::new();
        if should_announce {
            let call = &self.tool_calls[position];
            let tool_item_id = item_id("fc", &call.call_id, call.output_index);
            let output_index = call.output_index;
            let call_id = call.call_id.clone();
            let name = call.name.clone();
            frames.push(self.emit(
                "response.output_item.added",
                json!({
                    "output_index": output_index,
                    "item": {
                        "type": "function_call",
                        "id": tool_item_id,
                        "call_id": call_id,
                        "name": name,
                        "arguments": "",
                        "status": "in_progress",
                    },
                }),
            ));
        }

        let call = &mut self.tool_calls[position];
        let pending = if call.announced && call.arguments.len() > call.flushed {
            let fragment = call.arguments[call.flushed..].to_string();
            call.flushed = call.arguments.len();
            Some((
                call.output_index,
                item_id("fc", &call.call_id, call.output_index),
                fragment,
            ))
        } else {
            None
        };
        if let Some((output_index, tool_item_id, fragment)) = pending {
            frames.push(self.emit(
                "response.function_call_arguments.delta",
                json!({
                    "item_id": tool_item_id,
                    "output_index": output_index,
                    "delta": fragment,
                }),
            ));
        }

        frames
    }

    /// Builds the current response snapshot, inferring status from collected state.
    fn snapshot(&self, status: &str) -> Value {
        let finish_reason = self.finish_reason.clone().unwrap_or_default();
        self.snapshot_with(status, &finish_reason, false)
    }

    /// Builds a response snapshot with explicit status, finish reason and completion flag.
    fn snapshot_with(&self, status: &str, finish_reason: &str, completed: bool) -> Value {
        let mut output: Vec<(usize, Value)> = Vec::new();
        if self.reasoning_open && completed {
            output.push((
                self.reasoning_output_index,
                translate::reasoning_item(
                    &self.response_id,
                    &self.reasoning,
                    self.reasoning_output_index,
                ),
            ));
        }
        if self.message_open && completed {
            output.push((
                self.message_output_index,
                translate::message_item(&self.response_id, &self.text, self.message_output_index),
            ));
        }
        for tool_call in &self.tool_calls {
            if !tool_call.announced {
                continue;
            }
            output.push((
                tool_call.output_index,
                json!({
                    "type": "function_call",
                    "id": item_id("fc", &tool_call.call_id, tool_call.output_index),
                    "call_id": tool_call.call_id,
                    "name": tool_call.name,
                    "arguments": if completed { tool_call.arguments.clone() } else { String::new() },
                    "status": if completed { "completed" } else { "in_progress" },
                }),
            ));
        }
        output.sort_by_key(|(index, _)| *index);

        let mut response = translate::assemble_response(
            &self.response_id,
            &self.model,
            self.created_at,
            status,
            finish_reason,
            output.into_iter().map(|(_, item)| item).collect(),
            self.usage.as_ref(),
        );
        if !completed {
            if let Some(object) = response.as_object_mut() {
                object.insert("usage".to_string(), Value::Null);
            }
        }
        response
    }

    /// Serializes one Responses SSE frame.
    fn emit(&mut self, kind: &str, fields: Value) -> String {
        let mut payload: Map<String, Value> = fields.as_object().cloned().unwrap_or_default();
        payload.insert("type".to_string(), Value::String(kind.to_string()));
        payload.insert("sequence_number".to_string(), Value::from(self.sequence));
        self.sequence += 1;
        let body = serde_json::to_string(&Value::Object(payload)).unwrap_or_default();
        format!("event: {kind}\ndata: {body}\n\n")
    }
}

/// Builds a complete Responses usage object for a finished non-streamed translation.
///
/// Kept here so the streaming and non-streaming paths share one usage mapper.
#[must_use]
pub fn completed_usage(usage: Option<&Value>) -> Value {
    map_usage(usage)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn converter() -> ChatSseToResponsesConverter {
        ChatSseToResponsesConverter::new("resp_test".to_string(), "m".to_string(), 1_700_000_000)
    }

    fn chunk(payload: &str) -> String {
        format!("data: {payload}\n\n")
    }

    fn event_types(frames: &[String]) -> Vec<String> {
        frames
            .iter()
            .filter_map(|frame| {
                frame
                    .lines()
                    .find_map(|line| line.strip_prefix("event: "))
                    .map(str::to_string)
            })
            .collect()
    }

    #[test]
    fn begin_emits_created_then_in_progress() {
        let mut converter = converter();
        let frames = converter.begin();
        assert_eq!(
            event_types(&frames),
            vec!["response.created", "response.in_progress"]
        );
        assert!(converter.begin().is_empty());
    }

    #[test]
    fn text_deltas_open_item_then_stream() {
        let mut converter = converter();
        let mut frames = converter.begin();
        frames.extend(converter.feed(&chunk(
            r#"{"model":"m","choices":[{"delta":{"role":"assistant","content":"Hel"}}]}"#,
        )));
        frames.extend(converter.feed(&chunk(
            r#"{"choices":[{"delta":{"content":"lo"},"finish_reason":"stop"}]}"#,
        )));
        frames.extend(converter.finish());

        assert_eq!(
            event_types(&frames),
            vec![
                "response.created",
                "response.in_progress",
                "response.output_item.added",
                "response.content_part.added",
                "response.output_text.delta",
                "response.output_text.delta",
                "response.output_text.done",
                "response.content_part.done",
                "response.output_item.done",
                "response.completed",
            ]
        );
        let terminal = frames.last().expect("terminal frame");
        assert!(terminal.contains("Hello"));
        assert!(terminal.contains("\"status\":\"completed\""));
    }

    #[test]
    fn terminal_event_is_emitted_exactly_once() {
        let mut converter = converter();
        converter.begin();
        converter.feed(&chunk(
            r#"{"choices":[{"delta":{"content":"x"},"finish_reason":"stop"}]}"#,
        ));
        let first = converter.finish();
        let second = converter.finish();
        let third = converter.feed("data: [DONE]\n\n");
        assert_eq!(
            event_types(&first)
                .iter()
                .filter(|kind| kind.ends_with("completed"))
                .count(),
            1
        );
        assert!(second.is_empty());
        assert!(third.is_empty());
    }

    #[test]
    fn done_sentinel_finishes_the_turn() {
        let mut converter = converter();
        converter.begin();
        converter.feed(&chunk(r#"{"choices":[{"delta":{"content":"hi"}}]}"#));
        let frames = converter.feed("data: [DONE]\n\n");
        assert_eq!(
            event_types(&frames),
            vec![
                "response.output_text.done",
                "response.content_part.done",
                "response.output_item.done",
                "response.completed",
            ]
        );
    }

    #[test]
    fn partial_frames_across_chunks_are_reassembled() {
        let mut converter = converter();
        converter.begin();
        let split = chunk(r#"{"choices":[{"delta":{"content":"split"}}]}"#);
        let (head, tail) = split.split_at(12);
        assert!(converter.feed(head).is_empty());
        let frames = converter.feed(tail);
        assert_eq!(
            event_types(&frames),
            vec![
                "response.output_item.added",
                "response.content_part.added",
                "response.output_text.delta"
            ]
        );
        assert!(frames.last().expect("delta").contains("split"));
    }

    #[test]
    fn utf8_split_across_network_chunks_is_preserved() {
        let mut converter = converter();
        converter.begin();
        let event = chunk(r#"{"choices":[{"delta":{"content":"你好"}}]}"#);
        let bytes = event.as_bytes();
        let split = bytes.iter().position(|byte| *byte == 0xe4).expect("UTF-8 text") + 1;
        assert!(converter.feed_bytes(&bytes[..split]).is_empty());
        let frames = converter.feed_bytes(&bytes[split..]);
        assert!(frames.iter().any(|frame| frame.contains("你好")));
        assert!(!frames.iter().any(|frame| frame.contains('�')));
    }

    #[test]
    fn tool_call_is_announced_only_after_name_arrives() {
        let mut converter = converter();
        converter.begin();

        // Delta 1 carries the id but no name: announcing here would publish a nameless call.
        let frames = converter.feed(&chunk(
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_5"}]}}]}"#,
        ));
        assert!(event_types(&frames).is_empty());

        // Delta 2 delivers the name together with the first arguments fragment.
        let frames = converter.feed(&chunk(
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"shell","arguments":"{\"a\":1}"}}]},"finish_reason":"tool_calls"}]}"#,
        ));
        assert_eq!(
            event_types(&frames),
            vec![
                "response.output_item.added",
                "response.function_call_arguments.delta"
            ]
        );
        assert!(frames[0].contains("\"name\":\"shell\""));
        assert!(frames[0].contains("\"call_id\":\"call_5\""));
        assert!(frames[1].contains("{\\\"a\\\":1}"));

        let frames = converter.finish();
        assert_eq!(
            event_types(&frames),
            vec![
                "response.function_call_arguments.done",
                "response.output_item.done",
                "response.completed",
            ]
        );
        assert!(converter.finish().is_empty());
    }

    #[test]
    fn tool_call_without_name_is_never_announced() {
        let mut converter = converter();
        converter.begin();
        converter.feed(&chunk(
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1"}]},"finish_reason":"tool_calls"}]}"#,
        ));
        let frames = converter.finish();
        assert!(!event_types(&frames).contains(&"response.output_item.done".to_string()));
        assert_eq!(event_types(&frames), vec!["response.completed"]);
    }

    #[test]
    fn text_and_tool_call_keep_distinct_output_indexes() {
        let mut converter = converter();
        converter.begin();
        converter.feed(&chunk(r#"{"choices":[{"delta":{"content":"thinking"}}]}"#));
        converter.feed(&chunk(
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_9","function":{"name":"shell","arguments":"{}"}}]},"finish_reason":"tool_calls"}]}"#,
        ));
        let frames = converter.finish();
        let message_done = frames
            .iter()
            .find(|frame| frame.contains("\"type\":\"message\""))
            .expect("message item");
        let call_done = frames
            .iter()
            .find(|frame| frame.contains("\"type\":\"function_call\""))
            .expect("function call item");
        assert!(message_done.contains("\"output_index\":0"));
        assert!(call_done.contains("\"output_index\":1"));
    }

    #[test]
    fn streamed_reasoning_content_becomes_a_completed_reasoning_item() {
        let mut converter = converter();
        let mut feed_frames = converter.begin();

        feed_frames.extend(converter.feed(&chunk(
            r#"{"choices":[{"delta":{"reasoning_content":"Need to "}}]}"#,
        )));
        let reasoning_items_added = |frames: &[String]| {
            frames
                .iter()
                .filter(|frame| {
                    frame.contains("event: response.output_item.added")
                        && frame.contains("\"type\":\"reasoning\"")
                })
                .count()
        };
        assert_eq!(
            reasoning_items_added(&feed_frames),
            1,
            "the reasoning item must be announced exactly once"
        );

        feed_frames.extend(converter.feed(&chunk(
            r#"{"choices":[{"delta":{"reasoning_content":"inspect the file.","content":"Opening the file."},"finish_reason":"tool_calls"}]}"#,
        )));
        let serialized_feeds: String = feed_frames.concat();
        assert!(
            serialized_feeds.contains("inspect the file."),
            "reasoning deltas must be forwarded as they arrive"
        );
        assert_eq!(reasoning_items_added(&feed_frames), 1);

        let finish_frames = converter.finish();
        let reasoning_done = finish_frames
            .iter()
            .find(|frame| frame.contains("\"type\":\"reasoning\""))
            .expect("reasoning output item");
        assert!(reasoning_done.contains("Need to inspect the file."));

        let terminal = finish_frames.last().expect("terminal frame");
        assert!(terminal.contains("\"type\":\"reasoning\""));
        assert!(terminal.contains("Need to inspect the file."));
        assert!(terminal.contains("\"type\":\"message\""));
    }
    #[test]
    fn usage_is_reported_on_completion() {
        let mut converter = converter();
        converter.begin();
        converter.feed(&chunk(
            r#"{"choices":[{"delta":{"content":"ok"},"finish_reason":"stop"}]}"#,
        ));
        converter.feed(&chunk(
            r#"{"choices":[],"usage":{"prompt_tokens":7,"completion_tokens":3,"total_tokens":10}}"#,
        ));
        let frames = converter.finish();
        let terminal = frames.last().expect("terminal frame");
        assert!(terminal.contains("\"input_tokens\":7"));
        assert!(terminal.contains("\"output_tokens\":3"));
    }

    #[test]
    fn failure_emits_a_single_terminal_event() {
        let mut converter = converter();
        converter.begin();
        converter.feed(&chunk(r#"{"choices":[{"delta":{"content":"par"}}]}"#));
        let frames = converter.fail("connection reset");
        assert_eq!(event_types(&frames), vec!["response.failed"]);
        assert!(converter.fail("again").is_empty());
        assert!(converter.finish().is_empty());
    }

    #[test]
    fn completed_usage_shares_the_non_streaming_mapper() {
        let usage = json!({ "prompt_tokens": 2, "completion_tokens": 1 });
        assert_eq!(completed_usage(Some(&usage))["input_tokens"], 2);
        assert_eq!(completed_usage(Some(&usage))["total_tokens"], 3);
    }
}
