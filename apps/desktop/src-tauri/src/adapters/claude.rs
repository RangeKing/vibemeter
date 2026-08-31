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
    if record
        .get("isSidechain")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        && let Some(child_session_id) = record
            .get("agentId")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        && state
            .seen_delegation_children
            .insert(child_session_id.to_string())
    {
        let parent_session_id = state.source_session_id.clone();
        common::record_delegation_signal(
            state,
            child_session_id,
            Some(&parent_session_id),
            "spawn",
            "delegation.spawn.started",
            None,
            timestamp,
            "derived",
            "derived-delegation",
            record.get("uuid").and_then(Value::as_str),
        );
    }

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
    if let Some(result) = record.get("toolUseResult").and_then(Value::as_object)
        && let Some(child_session_id) = result
            .get("agentId")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
    {
        let parent_session_id = state.source_session_id.clone();
        let status = result.get("status").and_then(Value::as_str);
        let (relation_type, event_type, success) = match status {
            Some("async_launched" | "running" | "started") => {
                ("spawn", "delegation.spawn.started", None)
            }
            Some("completed" | "success" | "succeeded") => {
                ("join", "delegation.join.completed", Some(true))
            }
            Some("failed" | "error") => ("spawn", "delegation.spawn.failed", Some(false)),
            Some("waiting" | "blocked") => ("spawn", "delegation.child.waiting", None),
            _ => ("delegate", "delegation.delegate.started", None),
        };
        common::record_delegation_signal(
            state,
            child_session_id,
            Some(&parent_session_id),
            relation_type,
            event_type,
            success,
            timestamp,
            "derived",
            "derived-delegation",
            record.get("uuid").and_then(Value::as_str),
        );
        state
            .seen_delegation_children
            .insert(child_session_id.to_string());
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
    let subtype = record
        .get("subtype")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match subtype {
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
    if record
        .get("isSidechain")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        && let Some(child_session_id) = record
            .get("agentId")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
    {
        let (relation_type, event_type, success) = match subtype {
            "turn_duration" => ("join", "delegation.join.completed", Some(true)),
            "api_error" | "error" => ("spawn", "delegation.child.error", Some(false)),
            _ => return,
        };
        let parent_session_id = state.source_session_id.clone();
        common::record_delegation_signal(
            state,
            child_session_id,
            Some(&parent_session_id),
            relation_type,
            event_type,
            success,
            timestamp,
            "derived",
            "derived-delegation",
            record.get("uuid").and_then(Value::as_str),
        );
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::AgentKind;
    use serde_json::json;

    #[test]
    fn extracts_derived_sidechain_lifecycle_without_promoting_it_to_exact() {
        let mut state = ParseState::new(AgentKind::ClaudeCode, "parent-session".into());
        parse_record(
            &mut state,
            &json!({
                "type": "user",
                "timestamp": "2026-08-30T10:00:00Z",
                "sessionId": "parent-session",
                "cwd": "/Users/private/project",
                "uuid": "launch",
                "toolUseResult": {
                    "agentId": "child-agent",
                    "status": "async_launched",
                    "prompt": "PRIVATE_CHILD_PROMPT"
                },
                "message": {"role": "user", "content": [{"type": "tool_result", "content": "PRIVATE_RESULT"}]}
            }),
        );
        parse_record(
            &mut state,
            &json!({
                "type": "system",
                "subtype": "turn_duration",
                "durationMs": 1200,
                "timestamp": "2026-08-30T10:00:02Z",
                "sessionId": "parent-session",
                "agentId": "child-agent",
                "isSidechain": true,
                "uuid": "complete"
            }),
        );
        parse_record(
            &mut state,
            &json!({
                "type": "system",
                "subtype": "error",
                "timestamp": "2026-08-30T10:00:03Z",
                "sessionId": "parent-session",
                "agentId": "failed-child",
                "isSidechain": true,
                "uuid": "failure"
            }),
        );

        let delegation = state
            .events
            .iter()
            .filter(|event| event.relation_type.is_some())
            .collect::<Vec<_>>();
        assert!(delegation.iter().any(|event| {
            event.delegation_child_session_id.as_deref() == Some("child-agent")
                && event.relation_type.as_deref() == Some("spawn")
        }));
        assert!(delegation.iter().any(|event| {
            event.delegation_child_session_id.as_deref() == Some("child-agent")
                && event.relation_type.as_deref() == Some("join")
                && event.success == Some(true)
        }));
        assert!(delegation.iter().any(|event| {
            event.delegation_child_session_id.as_deref() == Some("failed-child")
                && event.event_type == "delegation.child.error"
        }));
        assert!(delegation.iter().all(|event| {
            event.evidence_level.as_deref() == Some("derived")
                && event.source_coverage.as_deref() == Some("derived-delegation")
        }));
        let serialized = serde_json::to_string(&delegation).expect("signals should serialize");
        assert!(!serialized.contains("PRIVATE_CHILD_PROMPT"));
        assert!(!serialized.contains("PRIVATE_RESULT"));
    }
}
