use crate::adapters::common;
use crate::models::{ParseState, TokenUsage};
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::Value;

/// ZCode keeps the durable task snapshot separate from the model I/O trail.
/// The snapshot is useful for identity, timing, prompts, tools, and task state;
/// the model I/O record carries the observed token usage. Both formats are
/// intentionally parsed defensively because older ZCode snapshots may omit
/// fields that newer ones include.
pub fn parse_record(state: &mut ParseState, record: &Value) {
    if record.get("type").and_then(Value::as_str) == Some("model_io") {
        parse_model_io(state, record);
    } else if record.get("meta").is_some() {
        parse_snapshot(state, record);
    } else {
        parse_session_event(state, record);
    }
}

pub fn parse_snapshot(state: &mut ParseState, snapshot: &Value) {
    let meta = snapshot.get("meta").unwrap_or(snapshot);
    let Some(meta_object) = meta.as_object() else {
        common::mark_unknown(state);
        return;
    };

    common::set_source_session(
        state,
        meta_object
            .get("taskId")
            .or_else(|| meta_object.get("sessionId"))
            .and_then(Value::as_str),
    );
    state.source_session_observed = true;
    common::set_project(
        state,
        meta_object.get("workspacePath").and_then(Value::as_str),
    );
    common::set_model(state, meta_object.get("model").and_then(Value::as_str));
    common::consider_title(state, meta_object.get("title").and_then(Value::as_str));

    if let Some(timestamp) = timestamp_from_value(meta_object.get("createdAt")) {
        common::observe_timestamp(state, Some(&timestamp), false);
    }

    if let Some(messages) = snapshot.get("messages").and_then(Value::as_array) {
        for message in messages {
            parse_message(state, message);
        }
    }

    if let Some(timestamp) = timestamp_from_value(meta_object.get("updatedAt")) {
        common::observe_timestamp(state, Some(&timestamp), false);
    }

    let ended_at = state.ended_at.clone();
    match meta_object.get("status").and_then(Value::as_str) {
        Some("complete") | Some("completed") => {
            if state.behavior.task_completions == 0 {
                common::record_task_complete(state, None, ended_at.as_deref());
            }
        }
        Some("failed") | Some("error") => common::record_error(state, ended_at.as_deref()),
        Some("cancelled") | Some("canceled") => {
            common::record_task_abort(state, ended_at.as_deref());
        }
        _ => {}
    }

    if let Some(events) = snapshot.get("events").and_then(Value::as_array) {
        for event in events {
            parse_session_event(state, event);
        }
    }
}

fn parse_message(state: &mut ParseState, message: &Value) {
    let Some(object) = message.as_object() else {
        common::mark_unknown(state);
        return;
    };
    let timestamp = timestamp_from_value(object.get("timestamp"));
    let role = object.get("role").and_then(Value::as_str);
    common::observe_timestamp(state, timestamp.as_deref(), role == Some("user"));

    if let Some(model) = object.get("model").and_then(Value::as_str) {
        common::set_model(state, Some(model));
    }
    let text = object.get("content").and_then(text_content);
    match role {
        Some("user") => {
            common::consider_title(state, text.as_deref());
            if state.behavior.task_starts == 0 {
                common::record_task_start(state, timestamp.as_deref());
            }
        }
        Some("assistant") => common::consider_result(state, text.as_deref()),
        _ => common::mark_unknown(state),
    }

    if object.get("interrupted").and_then(Value::as_bool) == Some(true) {
        common::record_task_abort(state, timestamp.as_deref());
    }

    if let Some(tools) = object.get("tools").and_then(Value::as_array) {
        for (index, tool) in tools.iter().enumerate() {
            let Some(tool_object) = tool.as_object() else {
                continue;
            };
            let source_event_id = common::normalized_source_record_id(tool).or_else(|| {
                timestamp.as_deref().map(|timestamp| {
                    common::derived_source_event_id(timestamp, "zcode-message-tool", index)
                })
            });
            if source_event_id.as_ref().is_some_and(|source_event_id| {
                !state
                    .seen_tool_ids
                    .insert(format!("history-tool:{source_event_id}"))
            }) {
                continue;
            }
            let name = tool_object
                .get("toolName")
                .or_else(|| tool_object.get("title"))
                .or_else(|| tool_object.get("kind"))
                .and_then(Value::as_str)
                .unwrap_or("tool");
            common::record_tool_with_source(
                state,
                name,
                tool_object.get("input"),
                timestamp.as_deref(),
                source_event_id.as_deref(),
            );
            if let Some(status) = tool_object.get("status").and_then(Value::as_str) {
                common::record_tool_result(
                    state,
                    status == "completed",
                    None,
                    timestamp.as_deref(),
                );
            }
        }
    }
}

fn parse_model_io(state: &mut ParseState, record: &Value) {
    let (accepted, source_record_id) = common::source_record_once(state, record);
    if !accepted {
        return;
    }
    let timestamp = record_timestamp(record);
    common::set_source_session(state, record.get("sessionId").and_then(Value::as_str));
    state.source_session_observed = true;
    common::set_project(
        state,
        record
            .get("workspacePath")
            .or_else(|| record.get("cwd"))
            .and_then(Value::as_str),
    );

    let model = record
        .get("model")
        .and_then(|value| value.get("modelId").or_else(|| value.get("model")))
        .and_then(Value::as_str)
        .or_else(|| record.get("modelId").and_then(Value::as_str));
    common::set_model(state, model);

    let query_source = record.get("querySource").and_then(Value::as_str);
    let user_text = record
        .get("request")
        .and_then(|request| request.get("messages"))
        .and_then(Value::as_array)
        .and_then(|messages| {
            messages.iter().rev().find_map(|message| {
                (message.get("role").and_then(Value::as_str) == Some("user"))
                    .then(|| message.get("content").and_then(text_content))
                    .flatten()
            })
        });
    let human = user_text.is_some() && query_source != Some("session_title");
    common::observe_timestamp(state, timestamp.as_deref(), human);
    if human {
        common::consider_title(state, user_text.as_deref());
        if state.behavior.task_starts == 0 {
            common::record_task_start(state, timestamp.as_deref());
        }
    }

    if query_source == Some("compact") {
        common::record_context_compaction(state, timestamp.as_deref());
    } else if query_source == Some("subagent") {
        common::record_subagent_activity(state, "interacted", timestamp.as_deref());
    }

    if let Some(response) = record.get("response") {
        common::consider_result(
            state,
            response
                .get("text")
                .and_then(Value::as_str)
                .or_else(|| response.get("reasoningText").and_then(Value::as_str)),
        );
        if let Some(usage) = response.get("usage") {
            let usage = parse_usage(usage);
            common::record_usage(state, &usage, timestamp.as_deref(), model);
        }
        if let Some(tool_calls) = response.get("toolCalls").and_then(Value::as_array) {
            for (index, tool_call) in tool_calls.iter().enumerate() {
                let name = tool_call
                    .get("name")
                    .or_else(|| tool_call.get("toolName"))
                    .and_then(Value::as_str)
                    .unwrap_or("tool");
                let source_event_id =
                    common::normalized_source_record_id(tool_call).or_else(|| {
                        source_record_id.as_deref().map(|source_record_id| {
                            common::derived_source_event_id(
                                source_record_id,
                                "zcode-model-tool",
                                index,
                            )
                        })
                    });
                common::record_tool_with_source(
                    state,
                    name,
                    tool_call.get("input"),
                    timestamp.as_deref(),
                    source_event_id.as_deref(),
                );
            }
        }
    } else if let Some(usage) = record.get("usage") {
        let usage = parse_usage(usage);
        common::record_usage(state, &usage, timestamp.as_deref(), model);
    }

    if record.get("error").is_some() {
        common::record_error(state, timestamp.as_deref());
    }
}

fn parse_session_event(state: &mut ParseState, event: &Value) {
    let (accepted, source_record_id) = common::source_record_once(state, event);
    if !accepted {
        return;
    }
    let Some(event_type) = event
        .get("type")
        .or_else(|| event.get("event"))
        .and_then(Value::as_str)
    else {
        common::mark_unknown(state);
        return;
    };
    let timestamp = record_timestamp(event);
    common::observe_timestamp(state, timestamp.as_deref(), false);
    let payload = event.get("payload").unwrap_or(event);
    match event_type {
        "turn.started" => common::record_task_start(state, timestamp.as_deref()),
        "turn.completed" => common::record_task_complete(
            state,
            payload.get("duration").and_then(number_u64),
            timestamp.as_deref(),
        ),
        "turn.failed" => common::record_error(state, timestamp.as_deref()),
        "session.closed" => {}
        "tool.updated" => {
            let name = payload
                .get("toolName")
                .or_else(|| payload.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("tool");
            let tool_source_id =
                common::normalized_source_record_id(payload).or_else(|| source_record_id.clone());
            common::record_tool_with_source(
                state,
                name,
                payload.get("input"),
                timestamp.as_deref(),
                tool_source_id.as_deref(),
            );
        }
        _ => common::mark_unknown(state),
    }
}

fn parse_usage(value: &Value) -> TokenUsage {
    let mut usage = TokenUsage {
        input_tokens: number(value, &["inputTokens", "input_tokens", "input"]),
        output_tokens: number(value, &["outputTokens", "output_tokens", "output"]),
        cache_read_tokens: number(
            value,
            &[
                "cacheReadTokens",
                "cachedReadTokens",
                "cache_read_tokens",
                "cache_read",
            ],
        ),
        cache_write_tokens: number(
            value,
            &[
                "cacheWriteTokens",
                "cachedWriteTokens",
                "cache_write_tokens",
                "cache_write",
            ],
        ),
        cache_write_1h_tokens: 0,
        reasoning_tokens: number(value, &["reasoningTokens", "reasoning_tokens", "reasoning"]),
    };
    if usage.total() == 0 {
        usage.output_tokens = number(value, &["totalTokens", "total_tokens", "total"]);
    }
    usage
}

fn text_content(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return Some(text.to_string());
    }
    let parts = value.as_array()?;
    let text = parts
        .iter()
        .filter_map(|part| {
            let kind = part.get("type").and_then(Value::as_str).unwrap_or_default();
            (kind == "text" || kind == "reasoning" || kind == "input_text" || kind == "output_text")
                .then(|| part.get("text").and_then(Value::as_str))
                .flatten()
        })
        .collect::<Vec<_>>()
        .join(" ");
    (!text.is_empty()).then_some(text)
}

fn record_timestamp(record: &Value) -> Option<String> {
    for key in [
        "startedAt",
        "timestamp",
        "createdAt",
        "completedAt",
        "updatedAt",
    ] {
        if let Some(timestamp) = timestamp_from_value(record.get(key)) {
            return Some(timestamp);
        }
    }
    None
}

fn timestamp_from_value(value: Option<&Value>) -> Option<String> {
    let value = value?;
    if let Some(text) = value.as_str().filter(|text| !text.is_empty()) {
        return Some(text.to_string());
    }
    let number = value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))?;
    if number >= 10_000_000_000 {
        DateTime::<Utc>::from_timestamp_millis(number)
    } else {
        DateTime::<Utc>::from_timestamp(number, 0)
    }
    .map(|time| time.to_rfc3339_opts(SecondsFormat::Millis, true))
}

fn number(value: &Value, keys: &[&str]) -> u64 {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(number_u64))
        .unwrap_or(0)
}

fn number_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_i64().and_then(|number| u64::try_from(number).ok()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::AgentKind;

    #[test]
    fn parses_zcode_snapshot_identity_and_tools() {
        let mut state = ParseState::new(AgentKind::ZCode, "fallback".into());
        parse_snapshot(
            &mut state,
            &serde_json::json!({
                "meta": {
                    "taskId": "zcode-task-1",
                    "title": "Ship the dashboard",
                    "workspacePath": "/workspace/demo",
                    "model": "glm-5",
                    "createdAt": 1_775_000_000_000_i64,
                    "updatedAt": 1_775_000_060_000_i64,
                    "status": "complete"
                },
                "messages": [
                    {"role": "user", "content": "Ship the dashboard", "timestamp": 1_775_000_000_000_i64},
                    {"role": "assistant", "content": "Done", "timestamp": 1_775_000_060_000_i64,
                     "tools": [{"toolName": "terminal", "status": "completed", "input": {"command": "true"}}]}
                ]
            }),
        );

        assert_eq!(state.source_session_id, "zcode-task-1");
        assert_eq!(state.project_label.as_deref(), Some("demo"));
        assert_eq!(state.primary_model().as_deref(), Some("glm-5"));
        assert_eq!(state.tool_calls, 1);
        assert_eq!(state.behavior.task_completions, 1);
        assert!(
            state
                .title
                .as_deref()
                .is_some_and(|title| title.contains("Ship"))
        );
    }

    #[test]
    fn parses_zcode_model_io_usage_and_tool_call() {
        let mut state = ParseState::new(AgentKind::ZCode, "fallback".into());
        parse_record(
            &mut state,
            &serde_json::json!({
                "type": "model_io",
                "sessionId": "zcode-session-1",
                "startedAt": "2026-08-06T10:00:00.000Z",
                "model": {"modelId": "glm-5", "role": "main"},
                "request": {"messages": [{"role": "user", "content": "Review the changes"}]},
                "response": {
                    "text": "Reviewed",
                    "toolCalls": [{"name": "read_file", "input": {"path": "src/lib.rs"}}],
                    "usage": {"inputTokens": 120, "outputTokens": 80, "cacheReadTokens": 30, "reasoningTokens": 20}
                }
            }),
        );

        assert_eq!(state.source_session_id, "zcode-session-1");
        assert_eq!(state.primary_model().as_deref(), Some("glm-5"));
        assert_eq!(state.usage.input_tokens, 120);
        assert_eq!(state.usage.output_tokens, 80);
        assert_eq!(state.usage.cache_read_tokens, 30);
        assert_eq!(state.usage.reasoning_tokens, 20);
        assert_eq!(state.tool_calls, 1);
    }
}
