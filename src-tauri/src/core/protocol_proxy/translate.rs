//! Bidirectional translation between the Responses API and Chat Completions.
//!
//! The two protocols disagree at a structural level: Chat Completions is
//! **message-centred** (`messages[]` with a flat role/content pair), while Responses
//! is **item-centred** (`output[]` / `input[]` typed items such as `message`,
//! `function_call`, `function_call_output`, `reasoning`). Text, ordinary function
//! calling, and reasoning content have explicit mappings. Unsupported item types are
//! deliberately dropped rather than guessed at, because an unknown field sent to a
//! strict upstream produces a hard 400 while an ignored field only degrades fidelity.
//!
//! Conventions:
//! - The request is rebuilt from an explicit **whitelist**, never by copying keys.
//!   Responses-only fields (`previous_response_id`, `store`, `include`, `text`,
//!   `truncation`, `prompt_cache_key`, …) have no Chat Completions equivalent and
//!   would be rejected by upstreams that validate their schema.
//! - The response is assembled into the documented Responses object shape so the
//!   Codex client sees exactly the field set it expects.

use serde_json::{json, Map, Value};

use super::{new_response_id, CHAT_WIRE_API};

/// How an upstream accepts reasoning/thinking controls.
///
/// Injecting a strength level into an upstream that only supports an on/off toggle
/// can get the request rejected, so the level is withheld for those hosts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReasoningStyle {
    /// Accepts a `reasoning_effort` strength level.
    Effort,
    /// Thinking is on by default; no level parameter may be injected.
    Toggle,
    /// Unknown upstream: inject nothing.
    None,
}

/// Classifies an upstream's reasoning capability from its base URL.
///
/// The match is a keyword heuristic on the host, mirroring the categorisation used by
/// comparable tools. It is intentionally conservative: a host that is not recognised
/// gets [`ReasoningStyle::None`], which leaves upstream defaults untouched instead of
/// risking a rejected request.
#[must_use]
pub fn reasoning_style_for(base_url: &str) -> ReasoningStyle {
    let lowered = base_url.trim().to_ascii_lowercase();
    const EFFORT_HOSTS: &[&str] = &["deepseek", "openrouter", "stepfun"];
    const TOGGLE_HOSTS: &[&str] = &[
        "moonshot",
        "kimi",
        "bigmodel",
        "zhipu",
        "glm",
        "dashscope",
        "aliyuncs",
        "bailian",
        "qwen",
        "minimax",
        "mimo",
        "xiaomi",
        "siliconflow",
        "volces",
        "volcengine",
        "doubao",
        "modelscope",
        "longcat",
    ];

    if EFFORT_HOSTS.iter().any(|host| lowered.contains(host)) {
        return ReasoningStyle::Effort;
    }
    if TOGGLE_HOSTS.iter().any(|host| lowered.contains(host)) {
        return ReasoningStyle::Toggle;
    }
    ReasoningStyle::None
}

/// Translates a Responses request body into a Chat Completions request body.
///
/// # Arguments
///
/// * `request` - The decoded Responses request received from Codex CLI.
/// * `base_url` - Upstream base URL, used to pick a reasoning-parameter style.
/// * `force_stream` - Whether the outbound request must stream (sentinel for the
///   caller's transport decision; mirrors the inbound `stream` flag when `true`).
///
/// # Returns
///
/// A JSON object ready to be sent to `POST {base_url}/chat/completions`.
///
/// # Errors
///
/// Returns `Err` if the body is not a JSON object, if it carries no `model`, or if no
/// chat message can be derived from `instructions` + `input`.
pub fn responses_to_chat_completions(
    request: &Value,
    base_url: &str,
    force_stream: bool,
) -> Result<Value, String> {
    let source = request
        .as_object()
        .ok_or_else(|| "Responses request body must be a JSON object.".to_string())?;

    let mut out = Map::new();

    let model = source
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Responses request is missing 'model'.".to_string())?;
    out.insert("model".to_string(), Value::String(model.to_string()));

    let mut messages: Vec<Value> = Vec::new();
    if let Some(instructions) = source.get("instructions").and_then(Value::as_str) {
        let trimmed = instructions.trim();
        if !trimmed.is_empty() {
            messages.push(json!({ "role": "system", "content": trimmed }));
        }
    }
    collect_input_messages(source.get("input"), &mut messages);
    if messages.is_empty() {
        return Err("Responses request produced no chat messages.".to_string());
    }
    out.insert("messages".to_string(), Value::Array(messages));

    let stream = force_stream
        || source
            .get("stream")
            .and_then(Value::as_bool)
            .unwrap_or(false);
    if stream {
        out.insert("stream".to_string(), Value::Bool(true));
        // Ask the upstream to emit a trailing usage-only chunk so `response.completed`
        // can report real token counts. Upstreams that do not implement
        // `stream_options` are expected to ignore it; see the module docs for the
        // compatibility caveat.
        out.insert(
            "stream_options".to_string(),
            json!({ "include_usage": true }),
        );
    }

    for key in [
        "temperature",
        "top_p",
        "presence_penalty",
        "frequency_penalty",
        "seed",
        "stop",
        "user",
    ] {
        if let Some(value) = source.get(key) {
            if !value.is_null() {
                out.insert(key.to_string(), value.clone());
            }
        }
    }

    if let Some(limit) = source.get("max_output_tokens") {
        if !limit.is_null() {
            out.insert("max_tokens".to_string(), limit.clone());
        }
    }

    if let Some(tools) = source.get("tools").and_then(convert_tools) {
        out.insert("tools".to_string(), tools);
    }
    if let Some(choice) = source.get("tool_choice").and_then(convert_tool_choice) {
        out.insert("tool_choice".to_string(), choice);
    }
    if let Some(parallel) = source.get("parallel_tool_calls") {
        if !parallel.is_null() {
            out.insert("parallel_tool_calls".to_string(), parallel.clone());
        }
    }

    if let ReasoningStyle::Effort = reasoning_style_for(base_url) {
        if let Some(effort) = source
            .get("reasoning")
            .and_then(Value::as_object)
            .and_then(|reasoning| reasoning.get("effort"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            out.insert(
                "reasoning_effort".to_string(),
                Value::String(effort.to_string()),
            );
        }
    }

    Ok(Value::Object(out))
}

/// Expands `instructions`-less `input` payloads into flat chat messages.
fn collect_input_messages(input: Option<&Value>, messages: &mut Vec<Value>) {
    match input {
        None | Some(Value::Null) => {}
        Some(Value::String(text)) => {
            messages.push(json!({ "role": "user", "content": text }));
        }
        Some(Value::Array(items)) => {
            let mut pending_reasoning = String::new();
            for item in items {
                if item.get("type").and_then(Value::as_str) == Some("reasoning") {
                    if let Some(text) = responses_reasoning_text(item) {
                        pending_reasoning.push_str(&text);
                    }
                    continue;
                }

                let Some(mut message) = input_item_to_message(item) else {
                    continue;
                };

                let is_assistant = message["role"] == "assistant";
                if !pending_reasoning.is_empty() && is_assistant {
                    message["reasoning_content"] = json!(pending_reasoning);
                    pending_reasoning.clear();
                } else if !is_assistant {
                    // Reasoning only belongs to the assistant turn that produced it.
                    pending_reasoning.clear();
                }
                messages.push(message);
            }
            merge_assistant_tool_call_messages(messages);
        }
        Some(_) => {
            log::warn!("protocol gateway: unsupported `input` shape, dropping it");
        }
    }
}

/// Merges adjacent assistant text and tool-call fragments into one Chat message.
///
/// A Responses turn may be represented as `message` followed by `function_call`.
/// Those items came from one model completion, so replaying them as two assistant
/// messages would detach `reasoning_content` from the tool-call turn and can trip
/// thinking-mode upstream validation.
fn merge_assistant_tool_call_messages(messages: &mut Vec<Value>) {
    let mut i = 0;
    while i + 1 < messages.len() {
        let adjacent_assistants =
            messages[i]["role"] == "assistant" && messages[i + 1]["role"] == "assistant";
        let has_tool_calls =
            messages[i].get("tool_calls").is_some() || messages[i + 1].get("tool_calls").is_some();

        if !adjacent_assistants || !has_tool_calls {
            i += 1;
            continue;
        }

        let next = messages.remove(i + 1);
        merge_assistant_message(&mut messages[i], &next);
        // Keep scanning from the same index so parallel calls fold together too.
    }
}

/// Folds one adjacent assistant fragment into the preceding assistant message.
fn merge_assistant_message(current: &mut Value, next: &Value) {
    let current_content_is_empty = current
        .get("content")
        .map(|content| content.is_null() || content == "")
        .unwrap_or(true);
    if current_content_is_empty {
        if let Some(content) = next.get("content") {
            current["content"] = content.clone();
        }
    }

    if current.get("reasoning_content").is_none() {
        if let Some(reasoning) = next.get("reasoning_content") {
            current["reasoning_content"] = reasoning.clone();
        }
    }

    if let Some(next_calls) = next.get("tool_calls").and_then(Value::as_array) {
        let mut calls = current
            .get("tool_calls")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        calls.extend(next_calls.iter().cloned());
        current["tool_calls"] = Value::Array(calls);
    }
}
/// Extracts plain reasoning text from a Responses reasoning item.
fn responses_reasoning_text(item: &Value) -> Option<String> {
    let object = item.as_object()?;

    if let Some(text) = object
        .get("reasoning_content")
        .and_then(flatten_text)
        .filter(|text| !text.is_empty())
    {
        return Some(text);
    }

    for key in ["content", "summary"] {
        let mut buffer = String::new();
        let Some(parts) = object.get(key).and_then(Value::as_array) else {
            continue;
        };
        for part in parts {
            let part_type = part.get("type").and_then(Value::as_str).unwrap_or("");
            if !matches!(part_type, "reasoning_text" | "summary_text" | "text") {
                continue;
            }
            if let Some(text) = part.get("text").and_then(Value::as_str) {
                buffer.push_str(text);
            }
        }
        if !buffer.is_empty() {
            return Some(buffer);
        }
    }

    None
}
/// Converts one Responses input item that maps directly to a Chat message.
///
/// Reasoning items are handled by [`collect_input_messages`] because their text must
/// be attached to the following assistant message. Returns `None` for unsupported
/// items such as hosted tools that have no Chat Completions representation.
fn input_item_to_message(item: &Value) -> Option<Value> {
    if let Some(text) = item.as_str() {
        return Some(json!({ "role": "user", "content": text }));
    }

    let object = item.as_object()?;
    let item_type = object
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("message");

    match item_type {
        "message" => {
            let role = match object.get("role").and_then(Value::as_str).unwrap_or("user") {
                // Responses uses `developer` where Chat Completions uses `system`.
                "developer" => "system",
                other => other,
            };
            let content = convert_message_content(object.get("content"));
            Some(json!({ "role": role, "content": content }))
        }
        "function_call" => {
            let name = object.get("name").and_then(Value::as_str)?.trim();
            if name.is_empty() {
                return None;
            }
            let call_id = object
                .get("call_id")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("call_pending");
            let arguments = object
                .get("arguments")
                .and_then(Value::as_str)
                .unwrap_or("{}");
            Some(json!({
                "role": "assistant",
                "content": Value::Null,
                "tool_calls": [{
                    "id": call_id,
                    "type": "function",
                    "function": { "name": name, "arguments": arguments }
                }]
            }))
        }
        "function_call_output" => {
            let call_id = object.get("call_id").and_then(Value::as_str).unwrap_or("");
            let content = match object.get("output") {
                Some(Value::String(text)) => text.clone(),
                Some(other) => other.to_string(),
                None => String::new(),
            };
            Some(json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": content
            }))
        }
        other => {
            log::debug!("protocol gateway: dropping unsupported input item type '{other}'");
            None
        }
    }
}

/// Converts Responses message content into Chat Completions content.
fn convert_message_content(content: Option<&Value>) -> Value {
    match content {
        None | Some(Value::Null) => Value::String(String::new()),
        Some(Value::String(text)) => Value::String(text.clone()),
        Some(Value::Array(parts)) => {
            let mut converted: Vec<Value> = Vec::new();
            for part in parts {
                let Some(object) = part.as_object() else {
                    continue;
                };
                match object.get("type").and_then(Value::as_str).unwrap_or("") {
                    "input_text" | "output_text" | "text" => {
                        if let Some(text) = object.get("text").and_then(Value::as_str) {
                            converted.push(json!({ "type": "text", "text": text }));
                        }
                    }
                    "input_image" | "image_url" => {
                        let url = object
                            .get("image_url")
                            .and_then(|value| {
                                value.as_str().map(str::to_string).or_else(|| {
                                    value.get("url").and_then(Value::as_str).map(str::to_string)
                                })
                            })
                            .unwrap_or_default();
                        if !url.is_empty() {
                            converted.push(json!({
                                "type": "image_url",
                                "image_url": { "url": url }
                            }));
                        }
                    }
                    _ => {}
                }
            }
            if converted.is_empty() {
                Value::String(String::new())
            } else {
                Value::Array(converted)
            }
        }
        Some(other) => Value::String(other.to_string()),
    }
}

/// Converts Responses tool declarations into Chat Completions tool declarations.
///
/// Hosted tools (`web_search`, `file_search`, …) are skipped: Chat Completions has no
/// equivalent server-side tool, and forwarding an unknown `type` is rejected.
fn convert_tools(tools: &Value) -> Option<Value> {
    let array = tools.as_array()?;
    let mut converted: Vec<Value> = Vec::new();
    for tool in array {
        let Some(object) = tool.as_object() else {
            continue;
        };
        if object
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("function")
            != "function"
        {
            continue;
        }
        // Tolerate an already-nested declaration.
        if let Some(function) = object.get("function").and_then(Value::as_object) {
            converted
                .push(json!({ "type": "function", "function": Value::Object(function.clone()) }));
            continue;
        }
        let Some(name) = object
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let mut function = Map::new();
        function.insert("name".to_string(), Value::String(name.to_string()));
        if let Some(description) = object.get("description").and_then(Value::as_str) {
            function.insert(
                "description".to_string(),
                Value::String(description.to_string()),
            );
        }
        if let Some(parameters) = object.get("parameters") {
            function.insert("parameters".to_string(), parameters.clone());
        }
        converted.push(json!({ "type": "function", "function": Value::Object(function) }));
    }
    if converted.is_empty() {
        None
    } else {
        Some(Value::Array(converted))
    }
}

/// Converts a Responses `tool_choice` into the Chat Completions shape.
fn convert_tool_choice(choice: &Value) -> Option<Value> {
    match choice {
        Value::String(mode) => Some(Value::String(mode.clone())),
        Value::Object(object) => {
            let name = object.get("name").and_then(Value::as_str)?;
            Some(json!({ "type": "function", "function": { "name": name } }))
        }
        _ => None,
    }
}

/// Translates a non-streamed Chat Completions response into a Responses object.
///
/// # Errors
///
/// Returns `Err` if the upstream payload is not a JSON object.
pub fn chat_completion_to_response(chat: &Value, request: &Value) -> Result<Value, String> {
    let source = chat
        .as_object()
        .ok_or_else(|| "Upstream Chat Completions response is not a JSON object.".to_string())?;

    let response_id = new_response_id();
    let model = source
        .get("model")
        .and_then(Value::as_str)
        .or_else(|| request.get("model").and_then(Value::as_str))
        .unwrap_or_default()
        .to_string();
    let created_at = source
        .get("created")
        .and_then(Value::as_i64)
        .unwrap_or_else(|| chrono::Utc::now().timestamp());

    let choice = source
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first());
    let message = choice.and_then(|choice| choice.get("message"));
    let finish_reason = choice
        .and_then(|choice| choice.get("finish_reason"))
        .and_then(Value::as_str)
        .unwrap_or("stop");

    let mut output: Vec<Value> = Vec::new();
    let mut output_index = 0usize;

    if let Some(reasoning) = message
        .and_then(|message| {
            message
                .get("reasoning_content")
                .or_else(|| message.get("reasoning"))
        })
        .and_then(flatten_text)
        .filter(|text| !text.is_empty())
    {
        output.push(reasoning_item(&response_id, &reasoning, output_index));
        output_index += 1;
    }

    if let Some(text) = message
        .and_then(|message| message.get("content"))
        .and_then(flatten_text)
        .filter(|text| !text.is_empty())
    {
        output.push(message_item(&response_id, &text, output_index));
        output_index += 1;
    }

    if let Some(tool_calls) = message
        .and_then(|message| message.get("tool_calls"))
        .and_then(Value::as_array)
    {
        for tool_call in tool_calls {
            if let Some(item) = function_call_item(tool_call, output_index) {
                output.push(item);
                output_index += 1;
            }
        }
    }

    let status = response_status(finish_reason);
    Ok(assemble_response(
        &response_id,
        &model,
        created_at,
        status,
        finish_reason,
        output,
        source.get("usage"),
    ))
}

/// Maps a Chat Completions `finish_reason` onto a Responses `status`.
#[must_use]
pub fn response_status(finish_reason: &str) -> &'static str {
    match finish_reason {
        "length" | "content_filter" => "incomplete",
        "error" => "failed",
        _ => "completed",
    }
}

/// Builds a Responses `reasoning` output item carrying plain reasoning text.
#[must_use]
pub fn reasoning_item(response_id: &str, text: &str, output_index: usize) -> Value {
    json!({
        "type": "reasoning",
        "id": item_id("rs", response_id, output_index),
        "summary": [],
        "content": [{ "type": "reasoning_text", "text": text }],
        "encrypted_content": null
    })
}
/// Builds a Responses `message` output item carrying plain assistant text.
#[must_use]
pub fn message_item(response_id: &str, text: &str, output_index: usize) -> Value {
    json!({
        "type": "message",
        "id": item_id("msg", response_id, output_index),
        "status": "completed",
        "role": "assistant",
        "content": [{ "type": "output_text", "text": text, "annotations": [] }]
    })
}

/// Builds a Responses `function_call` output item from a Chat Completions tool call.
fn function_call_item(tool_call: &Value, output_index: usize) -> Option<Value> {
    let object = tool_call.as_object()?;
    let function = object.get("function").and_then(Value::as_object);
    let name = function
        .and_then(|function| function.get("name"))
        .and_then(Value::as_str)?
        .trim();
    if name.is_empty() {
        return None;
    }
    let arguments = function
        .and_then(|function| function.get("arguments"))
        .and_then(Value::as_str)
        .unwrap_or("{}");
    let call_id = object
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("call_{output_index}"));

    Some(json!({
        "type": "function_call",
        "id": item_id("fc", &call_id, output_index),
        "call_id": call_id,
        "name": name,
        "arguments": arguments,
        "status": "completed"
    }))
}

/// Builds a complete Responses object around a set of output items.
#[must_use]
pub fn assemble_response(
    response_id: &str,
    model: &str,
    created_at: i64,
    status: &str,
    finish_reason: &str,
    output: Vec<Value>,
    usage: Option<&Value>,
) -> Value {
    let incomplete = if status == "incomplete" {
        json!({ "reason": if finish_reason == "content_filter" { "content_filter" } else { "max_output_tokens" } })
    } else {
        Value::Null
    };

    json!({
        "id": response_id,
        "object": "response",
        "created_at": created_at,
        "status": status,
        "model": model,
        "output": output,
        "parallel_tool_calls": true,
        "tool_choice": "auto",
        "tools": [],
        "usage": map_usage(usage),
        "incomplete_details": incomplete,
        "error": Value::Null,
        "metadata": {},
        "reasoning": { "effort": Value::Null, "summary": Value::Null },
        "text": { "format": { "type": "text" } },
        "truncation": "disabled"
    })
}

/// Remaps Chat Completions usage counters onto Responses counter names.
#[must_use]
pub fn map_usage(usage: Option<&Value>) -> Value {
    let source = usage.and_then(Value::as_object);
    let counter = |key: &str| -> i64 {
        source
            .and_then(|object| object.get(key))
            .and_then(Value::as_i64)
            .unwrap_or(0)
    };

    let input_tokens = if counter("prompt_tokens") != 0 {
        counter("prompt_tokens")
    } else {
        counter("input_tokens")
    };
    let output_tokens = if counter("completion_tokens") != 0 {
        counter("completion_tokens")
    } else {
        counter("output_tokens")
    };
    let total_tokens = {
        let declared = counter("total_tokens");
        if declared != 0 {
            declared
        } else {
            input_tokens + output_tokens
        }
    };
    let cached_tokens = source
        .and_then(|object| object.get("prompt_tokens_details"))
        .and_then(|details| details.get("cached_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let reasoning_tokens = source
        .and_then(|object| object.get("completion_tokens_details"))
        .and_then(|details| details.get("reasoning_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);

    json!({
        "input_tokens": input_tokens,
        "input_tokens_details": { "cached_tokens": cached_tokens },
        "output_tokens": output_tokens,
        "output_tokens_details": { "reasoning_tokens": reasoning_tokens },
        "total_tokens": total_tokens
    })
}

/// Flattens a Chat Completions `content` value into plain text.
///
/// Handles the plain string form, the array-of-parts form, and providers that emit a
/// bare object with a `text` field.
#[must_use]
pub fn flatten_text(content: &Value) -> Option<String> {
    match content {
        Value::Null => None,
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => {
            let mut buffer = String::new();
            for part in parts {
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    buffer.push_str(text);
                }
            }
            Some(buffer)
        }
        Value::Object(object) => object
            .get("text")
            .and_then(Value::as_str)
            .map(str::to_string),
        _ => None,
    }
}

/// Builds a stable item id of the form `{prefix}_{slug}_{index}`.
///
/// Codex requires distinct id namespaces per item kind: a `message` item id that
/// merely concatenates the response id is rejected by stricter clients.
#[must_use]
pub fn item_id(prefix: &str, slug: &str, index: usize) -> String {
    let core = slug
        .trim()
        .trim_start_matches("resp_")
        .trim_start_matches("chatcmpl-")
        .trim_start_matches("chatcmpl");
    if core.is_empty() {
        format!("{prefix}_{index}")
    } else {
        format!("{prefix}_{core}_{index}")
    }
}

/// Returns whether the given upstream protocol string denotes Chat Completions.
#[must_use]
pub fn is_chat_protocol(protocol: &str) -> bool {
    protocol == CHAT_WIRE_API
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_request() -> Value {
        json!({
            "model": "deepseek-v4-flash",
            "instructions": "You are a coding agent.",
            "input": [
                { "type": "message", "role": "user",
                  "content": [{ "type": "input_text", "text": "read main.rs" }] }
            ],
            "tools": [{ "type": "function", "name": "shell", "description": "run",
                        "parameters": { "type": "object" } }],
            "tool_choice": "auto",
            "max_output_tokens": 1024,
            "stream": true
        })
    }

    #[test]
    fn instructions_become_system_message() {
        let chat =
            responses_to_chat_completions(&base_request(), "https://api.deepseek.com/v1", false)
                .expect("translate");
        let messages = chat["messages"].as_array().expect("messages");
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], "You are a coding agent.");
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[1]["content"][0]["text"], "read main.rs");
    }

    #[test]
    fn responses_only_fields_are_not_forwarded() {
        let mut request = base_request();
        request["previous_response_id"] = json!("resp_old");
        request["store"] = json!(false);
        request["include"] = json!(["reasoning.encrypted_content"]);
        request["text"] = json!({ "format": { "type": "text" } });
        let chat = responses_to_chat_completions(&request, "https://api.deepseek.com/v1", false)
            .expect("translate");
        for key in [
            "previous_response_id",
            "store",
            "include",
            "text",
            "input",
            "instructions",
            "reasoning",
        ] {
            assert!(
                chat.get(key).is_none(),
                "{key} leaked into the chat request"
            );
        }
        assert_eq!(chat["max_tokens"], 1024);
        assert_eq!(chat["stream"], true);
    }

    #[test]
    fn function_call_history_becomes_tool_messages() {
        let request = json!({
            "model": "m",
            "input": [
                { "type": "function_call", "name": "shell", "arguments": "{\"cmd\":\"ls\"}",
                  "call_id": "call_1" },
                { "type": "function_call_output", "call_id": "call_1", "output": "files" }
            ]
        });
        let chat = responses_to_chat_completions(&request, "https://x.example/v1", false)
            .expect("translate");
        let messages = chat["messages"].as_array().expect("messages");
        assert_eq!(messages[0]["role"], "assistant");
        assert_eq!(messages[0]["tool_calls"][0]["id"], "call_1");
        assert_eq!(messages[0]["tool_calls"][0]["function"]["name"], "shell");
        assert_eq!(messages[1]["role"], "tool");
        assert_eq!(messages[1]["tool_call_id"], "call_1");
    }

    #[test]
    fn flat_tools_are_nested_for_chat() {
        let chat = responses_to_chat_completions(&base_request(), "https://x.example/v1", false)
            .expect("translate");
        assert_eq!(chat["tools"][0]["type"], "function");
        assert_eq!(chat["tools"][0]["function"]["name"], "shell");
        assert!(chat["tools"][0].get("name").is_none());
    }

    #[test]
    fn hosted_tools_are_dropped() {
        let mut request = base_request();
        request["tools"] = json!([
            { "type": "web_search" },
            { "type": "function", "name": "shell", "parameters": { "type": "object" } }
        ]);
        let chat = responses_to_chat_completions(&request, "https://x.example/v1", false)
            .expect("translate");
        assert_eq!(chat["tools"].as_array().expect("tools").len(), 1);
        assert_eq!(chat["tools"][0]["function"]["name"], "shell");
    }

    #[test]
    fn effort_style_injects_reasoning_effort() {
        let mut request = base_request();
        request["reasoning"] = json!({ "effort": "high" });
        let deepseek =
            responses_to_chat_completions(&request, "https://api.deepseek.com/v1", false)
                .expect("translate");
        assert_eq!(deepseek["reasoning_effort"], "high");

        let glm = responses_to_chat_completions(&request, "https://open.bigmodel.cn/api", false)
            .expect("translate");
        assert!(glm.get("reasoning_effort").is_none());

        let unknown = responses_to_chat_completions(&request, "https://example.com/v1", false)
            .expect("translate");
        assert!(unknown.get("reasoning_effort").is_none());
    }

    #[test]
    fn chat_response_maps_text_and_usage() {
        let chat = json!({
            "id": "chatcmpl-9",
            "model": "deepseek-v4-flash",
            "created": 1_700_000_000,
            "choices": [{ "index": 0, "finish_reason": "stop",
                          "message": { "role": "assistant", "content": "hello" } }],
            "usage": { "prompt_tokens": 10, "completion_tokens": 4, "total_tokens": 14,
                       "completion_tokens_details": { "reasoning_tokens": 2 } }
        });
        let response = chat_completion_to_response(&chat, &base_request()).expect("translate");
        assert_eq!(response["object"], "response");
        assert_eq!(response["status"], "completed");
        assert_eq!(response["output"][0]["type"], "message");
        assert_eq!(response["output"][0]["role"], "assistant");
        assert_eq!(response["output"][0]["content"][0]["text"], "hello");
        assert_eq!(response["output"][0]["content"][0]["type"], "output_text");
        assert!(response["output"][0]["id"]
            .as_str()
            .expect("item id")
            .starts_with("msg_"));
        assert_eq!(response["usage"]["input_tokens"], 10);
        assert_eq!(response["usage"]["output_tokens"], 4);
        assert_eq!(
            response["usage"]["output_tokens_details"]["reasoning_tokens"],
            2
        );
    }

    #[test]
    fn chat_response_preserves_reasoning_content_as_a_reasoning_item() {
        let chat = json!({
            "id": "chatcmpl-reasoning",
            "model": "deepseek-v4.1-flash",
            "choices": [{
                "index": 0,
                "finish_reason": "tool_calls",
                "message": {
                    "role": "assistant",
                    "reasoning_content": "Inspect the status field before editing.",
                    "content": "Now the read-only status line:",
                    "tool_calls": [{
                        "id": "call_reason",
                        "type": "function",
                        "function": { "name": "shell", "arguments": "{\"cmd\":\"sed -n '1,20p' file\"}" }
                    }]
                }
            }]
        });

        let response = chat_completion_to_response(&chat, &base_request()).expect("translate");
        let output = response["output"].as_array().expect("output");

        assert_eq!(output[0]["type"], "reasoning");
        assert_eq!(
            output[0]["content"][0]["text"],
            "Inspect the status field before editing."
        );
        assert_eq!(output[1]["type"], "message");
        assert_eq!(
            output[1]["content"][0]["text"],
            "Now the read-only status line:"
        );
        assert_eq!(output[2]["type"], "function_call");
        assert_eq!(output[2]["call_id"], "call_reason");
    }
    #[test]
    fn chat_response_maps_tool_calls() {
        let chat = json!({
            "id": "chatcmpl-10",
            "model": "m",
            "choices": [{ "index": 0, "finish_reason": "tool_calls",
                          "message": { "role": "assistant", "content": null,
                                       "tool_calls": [{ "id": "call_7", "type": "function",
                                                        "function": { "name": "shell",
                                                                      "arguments": "{\"cmd\":\"ls\"}" } }] } }]
        });
        let response = chat_completion_to_response(&chat, &base_request()).expect("translate");
        let item = &response["output"][0];
        assert_eq!(item["type"], "function_call");
        assert_eq!(item["call_id"], "call_7");
        assert_eq!(item["name"], "shell");
        assert_eq!(item["arguments"], "{\"cmd\":\"ls\"}");
    }

    #[test]
    fn truncated_completion_is_incomplete() {
        let chat = json!({
            "model": "m",
            "choices": [{ "index": 0, "finish_reason": "length",
                          "message": { "content": "cut" } }]
        });
        let response = chat_completion_to_response(&chat, &base_request()).expect("translate");
        assert_eq!(response["status"], "incomplete");
        assert_eq!(
            response["incomplete_details"]["reason"],
            "max_output_tokens"
        );
    }

    #[test]
    fn interleaved_reasoning_between_call_and_output_preserves_order() {
        let request = json!({
            "model": "m",
            "input": [
                { "type": "message", "role": "user", "content": "run it" },
                { "type": "reasoning", "summary": [] },
                { "type": "function_call", "name": "shell", "arguments": "{\"cmd\":\"ls\"}",
                  "call_id": "call_1" },
                { "type": "reasoning", "summary": [] },
                { "type": "function_call_output", "call_id": "call_1", "output": "files" }
            ]
        });
        let chat =
            responses_to_chat_completions(&request, "https://x/v1", false).expect("translate");
        let messages = chat["messages"].as_array().expect("messages");
        // user, assistant(tool_calls), tool — tool output must follow its call.
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[1]["tool_calls"][0]["id"], "call_1");
        assert_eq!(messages[2]["tool_call_id"], "call_1");
    }
    #[test]
    fn assistant_text_before_tool_calls_is_one_assistant_message() {
        let request = json!({
            "model": "m",
            "input": [
                { "type": "message", "role": "assistant",
                  "content": [{ "type": "output_text", "text": "Let me check." }] },
                { "type": "function_call", "name": "shell", "arguments": "{\"cmd\":\"ls\"}",
                  "call_id": "call_1" },
                { "type": "function_call_output", "call_id": "call_1", "output": "files" }
            ]
        });
        let chat =
            responses_to_chat_completions(&request, "https://x/v1", false).expect("translate");
        let messages = chat["messages"].as_array().expect("messages");

        // The visible text and tool call came from one assistant completion.
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "assistant");
        assert_eq!(messages[0]["content"][0]["text"], "Let me check.");
        assert_eq!(messages[0]["tool_calls"][0]["id"], "call_1");
        assert_eq!(messages[1]["role"], "tool");
        assert_eq!(messages[1]["tool_call_id"], "call_1");
    }
    #[test]
    fn parallel_function_calls_are_merged_into_one_assistant_message() {
        let request = json!({
            "model": "m",
            "input": [
                { "type": "message", "role": "user", "content": "read both files" },
                { "type": "function_call", "name": "shell", "arguments": "{\"cmd\":\"cat a\"}",
                  "call_id": "call_A" },
                { "type": "function_call", "name": "shell", "arguments": "{\"cmd\":\"cat b\"}",
                  "call_id": "call_B" },
                { "type": "function_call_output", "call_id": "call_A", "output": "aaa" },
                { "type": "function_call_output", "call_id": "call_B", "output": "bbb" }
            ]
        });
        let chat =
            responses_to_chat_completions(&request, "https://x/v1", false).expect("translate");
        let messages = chat["messages"].as_array().expect("messages");
        // assistant tool_calls merged, then tool outputs
        assert_eq!(messages.len(), 4);
        assert_eq!(
            messages[1]["tool_calls"].as_array().expect("calls").len(),
            2
        );
        assert_eq!(messages[2]["tool_call_id"], "call_A");
        assert_eq!(messages[3]["tool_call_id"], "call_B");
    }

    #[test]
    fn reasoning_round_trip_is_attached_to_the_assistant_tool_call() {
        let chat = json!({
            "id": "chatcmpl-round-trip",
            "model": "deepseek-v4.1-flash",
            "choices": [{
                "index": 0,
                "finish_reason": "tool_calls",
                "message": {
                    "role": "assistant",
                    "reasoning_content": "I should inspect the provider status first.",
                    "content": "Now the read-only status line:",
                    "tool_calls": [{
                        "id": "call_round_trip",
                        "type": "function",
                        "function": { "name": "shell", "arguments": "{\"cmd\":\"sed -n '1,20p' file\"}" }
                    }]
                }
            }]
        });

        let first_response =
            chat_completion_to_response(&chat, &base_request()).expect("translate first response");
        let mut input = first_response["output"].clone();
        input.as_array_mut().expect("output array").push(json!({
            "type": "function_call_output",
            "call_id": "call_round_trip",
            "output": "status = ready"
        }));

        let next_request = json!({ "model": "deepseek-v4.1-flash", "input": input });
        let next_chat = responses_to_chat_completions(&next_request, "https://x/v1", false)
            .expect("translate next request");
        let messages = next_chat["messages"].as_array().expect("messages");

        assert_eq!(
            messages.len(),
            2,
            "assistant text and tool call must remain one turn"
        );
        assert_eq!(messages[0]["role"], "assistant");
        assert_eq!(
            messages[0]["reasoning_content"],
            "I should inspect the provider status first."
        );
        assert_eq!(
            messages[0]["content"][0]["text"],
            "Now the read-only status line:"
        );
        assert_eq!(messages[0]["tool_calls"][0]["id"], "call_round_trip");
        assert_eq!(messages[1]["role"], "tool");
        assert_eq!(messages[1]["tool_call_id"], "call_round_trip");
    }
    #[test]
    fn missing_model_is_rejected() {
        assert!(
            responses_to_chat_completions(&json!({"input": "hi"}), "https://x/v1", false).is_err()
        );
    }

    #[test]
    fn empty_input_is_rejected() {
        assert!(responses_to_chat_completions(
            &json!({"model": "m", "input": []}),
            "https://x/v1",
            false
        )
        .is_err());
    }
}
