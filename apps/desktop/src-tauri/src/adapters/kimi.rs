use crate::adapters::common;
use crate::models::{ParseState, TokenUsage};
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::Value;

pub fn parse_record(state: &mut ParseState, record: &Value) {
    let (accepted, source_record_id) = common::source_record_once(state, record);
    if !accepted {
        return;
    }
    common::set_source_session(
        state,
        record
            .get("sessionId")
            .or_else(|| record.get("session_id"))
            .and_then(Value::as_str),
    );
    common::set_project(
        state,
        record
            .get("workspacePath")
            .or_else(|| record.get("cwd"))
            .or_else(|| record.get("workdir"))
            .and_then(Value::as_str),
    );
    let timestamp = record_timestamp(record);
    let record_type = record
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    common::observe_timestamp(state, timestamp.as_deref(), record_type == "turn.prompt");

    match record_type {
        "usage.record" => parse_usage(state, record, timestamp.as_deref()),
        "turn.prompt" => {
            let text = record
                .get("prompt")
                .or_else(|| record.get("message"))
                .or_else(|| record.get("content"))
                .and_then(common::text_from_message);
            common::consider_title(state, text.as_deref());
            common::record_task_start(state, timestamp.as_deref());
        }
        "context.append_message" => {
            let message = record.get("message").unwrap_or(record);
            if message.get("role").and_then(Value::as_str) == Some("user") {
                let text = message.get("content").and_then(common::text_from_message);
                common::consider_title(state, text.as_deref());
            }
        }
        "context.append_loop_event" => {
            let event = record.get("event").unwrap_or(&Value::Null);
            match event
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default()
            {
                "content.part" => {
                    let part = event.get("part").unwrap_or(&Value::Null);
                    if part.get("type").and_then(Value::as_str) == Some("text") {
                        common::append_result(state, part.get("text").and_then(Value::as_str));
                    }
                }
                "tool.call" => {
                    let event_source_id = common::normalized_source_record_id(event)
                        .or_else(|| source_record_id.clone());
                    common::record_tool_with_source(
                        state,
                        event.get("name").and_then(Value::as_str).unwrap_or("other"),
                        event.get("args"),
                        timestamp.as_deref(),
                        event_source_id.as_deref(),
                    );
                }
                "tool.result" => {
                    let result = event.get("result").unwrap_or(&Value::Null);
                    let success = result
                        .get("isError")
                        .or_else(|| result.get("is_error"))
                        .and_then(Value::as_bool)
                        != Some(true);
                    common::record_tool_result(state, success, None, timestamp.as_deref());
                }
                _ => {}
            }
        }
        "llm.request" => {}
        "turn.steer" => common::record_goal_change(state, timestamp.as_deref()),
        "turn.cancel" => common::record_task_abort(state, timestamp.as_deref()),
        "full_compaction.begin" | "context.apply_compaction" => {
            common::record_context_compaction(state, timestamp.as_deref())
        }
        "full_compaction.complete" => {}
        "metadata"
        | "config.update"
        | "tools.set_active_tools"
        | "tools.update_store"
        | "llm.tools_snapshot"
        | "permission.set_mode"
        | "permission.record_approval_result" => {}
        _ => common::mark_unknown(state),
    }
}

fn parse_usage(state: &mut ParseState, record: &Value, timestamp: Option<&str>) {
    let Some(usage) = record.get("usage") else {
        return;
    };
    let model = normalized_model(record);
    common::record_usage(
        state,
        &TokenUsage {
            input_tokens: number(usage, "inputOther"),
            output_tokens: number(usage, "output"),
            cache_read_tokens: number(usage, "inputCacheRead"),
            cache_write_tokens: number(usage, "inputCacheCreation"),
            cache_write_1h_tokens: 0,
            reasoning_tokens: 0,
        },
        timestamp,
        model.as_deref(),
    );
}

fn normalized_model(record: &Value) -> Option<String> {
    let model = record.get("model").and_then(Value::as_str)?.trim();
    if model.is_empty() {
        return None;
    }
    if model.contains('/') {
        return Some(model.to_string());
    }
    let provider = record
        .get("provider")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    Some(provider.map_or_else(
        || model.to_string(),
        |provider| format!("{provider}/{model}"),
    ))
}

fn record_timestamp(record: &Value) -> Option<String> {
    if let Some(value) = record
        .get("timestamp")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        return Some(value.to_string());
    }
    let millis = record
        .get("time")
        .or_else(|| record.get("created_at"))
        .and_then(|value| {
            value
                .as_i64()
                .or_else(|| value.as_u64().and_then(|raw| i64::try_from(raw).ok()))
        })?;
    DateTime::<Utc>::from_timestamp_millis(millis)
        .map(|value| value.to_rfc3339_opts(SecondsFormat::Millis, true))
}

fn number(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::AgentKind;

    #[test]
    fn parses_kimi_k3_usage_and_epoch_milliseconds() {
        let mut state = ParseState::new(AgentKind::KimiCode, "kimi-session".into());
        parse_record(
            &mut state,
            &serde_json::json!({
                "type": "usage.record",
                "time": 1_784_467_905_327_i64,
                "model": "kimi-code/k3",
                "usageScope": "turn",
                "usage": {
                    "inputOther": 2104,
                    "output": 849,
                    "inputCacheRead": 18944,
                    "inputCacheCreation": 32
                }
            }),
        );

        assert_eq!(state.primary_model().as_deref(), Some("kimi-code/k3"));
        assert_eq!(state.usage.input_tokens, 2104);
        assert_eq!(state.usage.output_tokens, 849);
        assert_eq!(state.usage.cache_read_tokens, 18944);
        assert_eq!(state.usage.cache_write_tokens, 32);
        assert_eq!(
            state.hourly.values().map(TokenUsage::total).sum::<u64>(),
            21929
        );
        assert!(
            state
                .started_at
                .as_deref()
                .is_some_and(|value| value.starts_with("2026-"))
        );
    }
}
