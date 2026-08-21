use crate::adapters::common;
use crate::models::{ParseState, TokenUsage};
use crate::privacy::stable_hash;
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::Value;
use std::fs;
use std::path::Path;

const MAX_METADATA_BYTES: u64 = 256 * 1024;

/// Grok Build persists an ACP session as a summary plus append-only update
/// envelopes. This adapter intentionally keeps only privacy-trimmed metadata,
/// lifecycle facts, tool names, and token counters.
pub fn restore_session_context(updates_path: &Path, state: &mut ParseState) {
    let Some(session_root) = updates_path.parent() else {
        return;
    };
    let summary_path = session_root.join("summary.json");
    let Some(summary) = read_json(&summary_path) else {
        return;
    };
    common::set_source_session(
        state,
        summary
            .get("info")
            .and_then(|info| info.get("id"))
            .and_then(Value::as_str),
    );
    state.source_session_observed = true;
    common::set_project(
        state,
        summary
            .get("info")
            .and_then(|info| info.get("cwd"))
            .and_then(Value::as_str),
    );
    common::set_model(
        state,
        summary.get("current_model_id").and_then(Value::as_str),
    );
    common::set_observed_title(
        state,
        summary.get("session_summary").and_then(Value::as_str),
    );
    if let Some(timestamp) = timestamp_from_value(summary.get("created_at")) {
        common::observe_timestamp(state, Some(&timestamp), false);
    }

    // Chat history is read only to recover user-facing, sanitized title/result
    // evidence when an ACP stream was compacted before it was indexed.
    let history_path = session_root.join("chat_history.jsonl");
    if let Ok(content) = fs::read_to_string(history_path) {
        for line in content.lines().take(4096) {
            let Ok(record) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            parse_chat_record(state, &record);
        }
    }
}

pub fn parse_record(state: &mut ParseState, envelope: &Value) {
    let (accepted, _) = common::source_record_once(state, envelope);
    if !accepted {
        return;
    }
    let params = envelope.get("params").unwrap_or(&Value::Null);
    common::set_source_session(state, params.get("sessionId").and_then(Value::as_str));
    let update = params.get("update").unwrap_or(&Value::Null);
    let timestamp = timestamp_from_value(envelope.get("timestamp").or_else(|| {
        params
            .get("_meta")
            .and_then(|meta| meta.get("agentTimestampMs"))
    }));
    let source_event_id = params
        .get("_meta")
        .and_then(|meta| meta.get("eventId"))
        .and_then(Value::as_str)
        .map(stable_hash);
    let update_kind = update
        .get("sessionUpdate")
        .and_then(Value::as_str)
        .unwrap_or_default();

    match update_kind {
        "user_message_chunk" => {
            common::observe_timestamp(state, timestamp.as_deref(), false);
            common::set_observed_title(state, content_text(update).as_deref());
        }
        "agent_message_chunk" => {
            common::observe_timestamp(state, timestamp.as_deref(), false);
            common::append_result(state, content_text(update).as_deref());
        }
        "agent_thought_chunk" => {
            common::observe_timestamp(state, timestamp.as_deref(), false);
            common::record_event_with_source(
                state,
                "phase",
                "think",
                "thinking",
                Some(true),
                timestamp.as_deref(),
                source_event_id.as_deref(),
            );
        }
        "tool_call" => parse_tool_call(state, update, timestamp.as_deref(), source_event_id),
        "tool_call_update" => parse_tool_call_update(state, update, timestamp.as_deref()),
        "usage_update" => parse_usage_update(state, update, timestamp.as_deref()),
        "hook_execution" => parse_hook_execution(state, update, timestamp.as_deref()),
        "turn_start" => common::record_task_start(state, timestamp.as_deref()),
        "turn_end" => parse_turn_end(state, update, timestamp.as_deref()),
        "session_end" => parse_session_end(state, update, timestamp.as_deref()),
        "session_start"
        | "session_info_update"
        | "current_mode_update"
        | "config_option_updates"
        | "available_commands_update"
        | "signal" => {}
        "error" => common::record_error(state, timestamp.as_deref()),
        _ => common::mark_unknown(state),
    }
}

fn parse_hook_execution(state: &mut ParseState, update: &Value, timestamp: Option<&str>) {
    match update
        .get("event_name")
        .or_else(|| update.get("eventName"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "session_start" | "turn_start" => common::record_task_start(state, timestamp),
        "session_end" | "turn_end" => {
            if state.behavior.task_completions == 0 {
                common::record_task_complete(state, None, timestamp);
            }
        }
        "error" | "session_error" => common::record_error(state, timestamp),
        _ => {}
    }
}

fn parse_chat_record(state: &mut ParseState, record: &Value) {
    let record_type = record
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let content = record.get("content").or_else(|| record.get("message"));
    match record_type {
        "user" | "human" => common::consider_title(
            state,
            content.and_then(common::text_from_message).as_deref(),
        ),
        "assistant" | "agent" => common::consider_result(
            state,
            content.and_then(common::text_from_message).as_deref(),
        ),
        _ => {}
    }
}

fn parse_tool_call(
    state: &mut ParseState,
    update: &Value,
    timestamp: Option<&str>,
    source_event_id: Option<String>,
) {
    let name = update
        .get("title")
        .or_else(|| update.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("tool");
    common::record_tool_with_source(state, name, None, timestamp, source_event_id.as_deref());
    if let Some(status) = update.get("status").and_then(Value::as_str) {
        parse_tool_status(state, status, timestamp);
    }
}

fn parse_tool_call_update(state: &mut ParseState, update: &Value, timestamp: Option<&str>) {
    if let Some(status) = update.get("status").and_then(Value::as_str) {
        parse_tool_status(state, status, timestamp);
    }
}

fn parse_tool_status(state: &mut ParseState, status: &str, timestamp: Option<&str>) {
    match status.to_ascii_lowercase().as_str() {
        "completed" | "complete" | "success" => {
            common::record_tool_result(state, true, None, timestamp)
        }
        "failed" | "error" | "cancelled" | "canceled" => {
            common::record_tool_result(state, false, None, timestamp)
        }
        _ => {}
    }
}

fn parse_turn_end(state: &mut ParseState, update: &Value, timestamp: Option<&str>) {
    let status = update
        .get("stopReason")
        .or_else(|| update.get("status"))
        .or_else(|| update.get("reason"))
        .and_then(Value::as_str)
        .unwrap_or("completed")
        .to_ascii_lowercase();
    if matches!(status.as_str(), "error" | "failed" | "blocked") {
        common::record_error(state, timestamp);
        common::record_task_abort(state, timestamp);
    } else if matches!(status.as_str(), "cancelled" | "canceled" | "aborted") {
        common::record_task_abort(state, timestamp);
    } else {
        common::record_task_complete(state, None, timestamp);
    }
}

fn parse_session_end(state: &mut ParseState, update: &Value, timestamp: Option<&str>) {
    let status = update
        .get("status")
        .or_else(|| update.get("reason"))
        .and_then(Value::as_str)
        .unwrap_or("completed");
    if matches!(status.to_ascii_lowercase().as_str(), "error" | "failed") {
        common::record_error(state, timestamp);
    } else if state.behavior.task_completions == 0 {
        common::record_task_complete(state, None, timestamp);
    }
}

fn parse_usage_update(state: &mut ParseState, update: &Value, timestamp: Option<&str>) {
    let (current, model) = usage_from_update(update);
    if current.total() == 0 {
        return;
    }
    let delta = current.saturating_delta(&state.previous_grok_total);
    state.previous_grok_total = current;
    if delta.total() == 0 {
        return;
    }
    let model = model
        .or_else(|| {
            update
                .get("modelId")
                .or_else(|| update.get("model"))
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
        .or_else(|| state.current_model.clone());
    common::record_usage(state, &delta, timestamp, model.as_deref());
}

fn usage_from_update(update: &Value) -> (TokenUsage, Option<String>) {
    let usage = update
        .get("usage")
        .or_else(|| update.get("modelUsage"))
        .or_else(|| update.get("model_usage"));
    let Some(usage) = usage else {
        return (usage_from_value(update), None);
    };
    let direct = usage_from_value(usage);
    if direct.total() > 0 {
        return (
            direct,
            update
                .get("modelId")
                .or_else(|| update.get("model"))
                .and_then(Value::as_str)
                .map(ToString::to_string),
        );
    }

    let Some(entries) = usage.as_object() else {
        return (TokenUsage::default(), None);
    };
    let mut total = TokenUsage::default();
    let mut model = None;
    for (model_id, value) in entries {
        let Some(value) = value.as_object() else {
            continue;
        };
        let current = usage_from_value(&Value::Object(value.clone()));
        if current.total() == 0 {
            continue;
        }
        total.add_assign(&current);
        if model.is_none() {
            model = Some(model_id.clone());
        } else {
            model = None;
        }
    }
    (total, model)
}

fn usage_from_value(value: &Value) -> TokenUsage {
    TokenUsage {
        input_tokens: number(value, &["input_tokens", "inputTokens", "input"]),
        output_tokens: number(value, &["output_tokens", "outputTokens", "output"]),
        cache_read_tokens: number(
            value,
            &[
                "cache_read_input_tokens",
                "cacheReadInputTokens",
                "cachedReadTokens",
                "cacheReadTokens",
            ],
        ),
        cache_write_tokens: number(
            value,
            &[
                "cache_creation_input_tokens",
                "cacheCreationInputTokens",
                "cachedWriteTokens",
                "cacheWriteTokens",
            ],
        ),
        cache_write_1h_tokens: 0,
        reasoning_tokens: number(
            value,
            &["reasoning_tokens", "reasoningTokens", "thoughtTokens"],
        ),
    }
}

fn number(value: &Value, keys: &[&str]) -> u64 {
    keys.iter()
        .find_map(|key| {
            value.get(*key).and_then(|value| {
                value
                    .as_u64()
                    .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
            })
        })
        .unwrap_or(0)
}

fn content_text(update: &Value) -> Option<String> {
    for key in ["content", "message", "text"] {
        let Some(value) = update.get(key) else {
            continue;
        };
        if let Some(text) = common::text_from_message(value) {
            return Some(text);
        }
        if let Some(text) = value.get("text").and_then(Value::as_str) {
            return Some(text.to_string());
        }
    }
    None
}

fn read_json(path: &Path) -> Option<Value> {
    let metadata = fs::metadata(path).ok()?;
    (metadata.len() <= MAX_METADATA_BYTES)
        .then(|| serde_json::from_slice(&fs::read(path).ok()?).ok())
        .flatten()
}

fn timestamp_from_value(value: Option<&Value>) -> Option<String> {
    let value = value?;
    if let Some(text) = value.as_str().filter(|text| !text.is_empty()) {
        return Some(text.to_string());
    }
    let number = value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))?;
    let timestamp = if number >= 10_000_000_000 {
        DateTime::<Utc>::from_timestamp_millis(number)
    } else {
        DateTime::<Utc>::from_timestamp(number, 0)
    }?;
    Some(timestamp.to_rfc3339_opts(SecondsFormat::Millis, true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::AgentKind;

    #[test]
    fn parses_grok_acp_lifecycle_tools_and_cumulative_usage_without_leaking_payloads() {
        let mut state = ParseState::new(AgentKind::GrokBuild, "grok-session".into());
        common::set_model(&mut state, Some("grok-build-0.1"));
        for record in [
            serde_json::json!({"timestamp":1787292967,"params":{"sessionId":"grok-session","update":{"sessionUpdate":"turn_start"}}}),
            serde_json::json!({"timestamp":1787292968,"params":{"sessionId":"grok-session","update":{"sessionUpdate":"tool_call","title":"terminal","status":"completed","rawInput":"private command"}}}),
            serde_json::json!({"timestamp":1787292969,"params":{"sessionId":"grok-session","update":{"sessionUpdate":"usage_update","usage":{"input_tokens":100,"output_tokens":40,"cache_read_input_tokens":20,"reasoning_tokens":7}}}}),
            serde_json::json!({"timestamp":1787292970,"params":{"sessionId":"grok-session","update":{"sessionUpdate":"usage_update","usage":{"input_tokens":150,"output_tokens":60,"cache_read_input_tokens":35,"reasoning_tokens":10}}}}),
            serde_json::json!({"timestamp":1787292971,"params":{"sessionId":"grok-session","update":{"sessionUpdate":"turn_end","stopReason":"completed"}}}),
        ] {
            parse_record(&mut state, &record);
        }
        assert_eq!(state.usage.input_tokens, 150);
        assert_eq!(state.usage.output_tokens, 60);
        assert_eq!(state.usage.cache_read_tokens, 35);
        assert_eq!(state.usage.reasoning_tokens, 10);
        assert_eq!(state.tool_calls, 1);
        assert_eq!(state.behavior.task_completions, 1);
        assert!(!format!("{state:?}").contains("private command"));
    }

    #[test]
    fn chat_history_only_uses_sanitized_user_title() {
        let mut state = ParseState::new(AgentKind::GrokBuild, "grok-session".into());
        parse_chat_record(
            &mut state,
            &serde_json::json!({"type":"user","content":"fix the release bug"}),
        );
        assert_eq!(state.title.as_deref(), Some("fix the release bug"));
    }

    #[test]
    fn parses_model_usage_maps_and_string_token_counts() {
        let mut state = ParseState::new(AgentKind::GrokBuild, "grok-session".into());
        common::set_model(&mut state, Some("grok-4.6"));
        parse_record(
            &mut state,
            &serde_json::json!({
                "timestamp": 1787292972,
                "params": {
                    "sessionId": "grok-session",
                    "update": {
                        "sessionUpdate": "usage_update",
                        "modelUsage": {
                            "grok-4.6": {
                                "inputTokens": "120",
                                "outputTokens": 40,
                                "cacheReadTokens": 20,
                                "reasoningTokens": 8
                            }
                        }
                    }
                }
            }),
        );
        assert_eq!(state.usage.input_tokens, 120);
        assert_eq!(state.usage.output_tokens, 40);
        assert_eq!(state.usage.cache_read_tokens, 20);
        assert_eq!(state.usage.reasoning_tokens, 8);
    }
}
