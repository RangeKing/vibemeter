use crate::adapters::common;
use crate::models::{ParseState, TokenUsage};
use serde_json::Value;

pub fn parse_record(state: &mut ParseState, record: &Value) {
    let timestamp = record.get("timestamp").and_then(Value::as_str);
    let record_type = record
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let human = record_type == "user"
        && record
            .get("message")
            .and_then(|message| message.get("role"))
            .and_then(Value::as_str)
            .is_some_and(|role| role == "user")
        && !contains_only_tool_result(record.get("message"));
    common::observe_timestamp(state, timestamp, human);
    common::set_source_session(state, record.get("sessionId").and_then(Value::as_str));
    common::set_project(state, record.get("cwd").and_then(Value::as_str));

    match record_type {
        "user" => parse_user(state, record, timestamp),
        "assistant" => parse_assistant(state, record, timestamp),
        "system" => parse_system(state, record, timestamp),
        "file-history-snapshot" | "last-prompt" => {}
        _ => common::mark_unknown(state),
    }
}

fn parse_user(state: &mut ParseState, record: &Value, timestamp: Option<&str>) {
    let Some(message) = record.get("message") else {
        return;
    };
    if let Some(text) = message.get("content").and_then(common::text_from_message) {
        common::consider_title(state, Some(&text));
    }
    if let Some(content) = message.get("content").and_then(Value::as_array) {
        for item in content {
            if item.get("type").and_then(Value::as_str) == Some("tool_result") {
                common::record_tool_result(
                    state,
                    !item
                        .get("is_error")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    None,
                    timestamp,
                );
            }
        }
    }
}

fn parse_assistant(state: &mut ParseState, record: &Value, timestamp: Option<&str>) {
    let Some(message) = record.get("message") else {
        return;
    };
    let model = message.get("model").and_then(Value::as_str);
    common::set_model(state, model);
    let assistant_text = message.get("content").and_then(common::text_from_message);
    common::consider_result(state, assistant_text.as_deref());

    if let Some(usage_value) = message.get("usage") {
        let message_id = message
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let current = TokenUsage {
            input_tokens: number(usage_value, "input_tokens"),
            output_tokens: number(usage_value, "output_tokens"),
            cache_read_tokens: number(usage_value, "cache_read_input_tokens"),
            cache_write_tokens: number(usage_value, "cache_creation_input_tokens").saturating_sub(
                usage_value
                    .get("cache_creation")
                    .and_then(|value| value.get("ephemeral_1h_input_tokens"))
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
            ),
            cache_write_1h_tokens: usage_value
                .get("cache_creation")
                .and_then(|value| value.get("ephemeral_1h_input_tokens"))
                .and_then(Value::as_u64)
                .unwrap_or(0),
            reasoning_tokens: 0,
        };
        let delta = if state.last_claude_message_id.as_deref() == Some(message_id) {
            current.saturating_delta(&state.last_claude_message_usage)
        } else {
            current.clone()
        };
        state.last_claude_message_id = Some(message_id.to_string());
        state.last_claude_message_usage = current;
        common::record_usage(state, &delta, timestamp, model);
    }

    if let Some(content) = message.get("content").and_then(Value::as_array) {
        for item in content {
            if item.get("type").and_then(Value::as_str) != Some("tool_use") {
                continue;
            }
            let tool_id = common::normalized_tool_id(item);
            if tool_id
                .as_ref()
                .is_some_and(|tool_id| !state.seen_tool_ids.insert(tool_id.clone()))
            {
                continue;
            }
            let name = item.get("name").and_then(Value::as_str).unwrap_or("other");
            common::record_tool_with_source(
                state,
                name,
                item.get("input"),
                timestamp,
                tool_id.as_deref(),
            );
        }
    }
}

fn parse_system(state: &mut ParseState, record: &Value, timestamp: Option<&str>) {
    match record
        .get("subtype")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "compact_boundary" => common::record_context_compaction(state, timestamp),
        "turn_duration" => {
            if let Some(duration_ms) = record
                .get("durationMs")
                .or_else(|| record.get("duration_ms"))
                .and_then(Value::as_u64)
            {
                common::record_task_complete(state, Some(duration_ms), timestamp);
            }
        }
        "api_error" | "error" => common::record_error(state, timestamp),
        "retry" => common::increment_retry(state),
        _ => {}
    }
}

fn contains_only_tool_result(message: Option<&Value>) -> bool {
    let Some(content) = message
        .and_then(|value| value.get("content"))
        .and_then(Value::as_array)
    else {
        return false;
    };
    !content.is_empty()
        && content
            .iter()
            .all(|item| item.get("type").and_then(Value::as_str) == Some("tool_result"))
}

fn number(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or(0)
}
