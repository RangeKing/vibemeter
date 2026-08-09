use crate::adapters::common;
use crate::models::{ParseState, TokenUsage};
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::Value;

/// OpenCLAW keeps append-only session records. The fields vary by transport, so
/// this parser deliberately accepts the stable message/tool/usage envelope and
/// leaves unknown records out of the derived metrics.
pub fn parse_record(state: &mut ParseState, record: &Value) {
    let (accepted, source_record_id) = common::source_record_once(state, record);
    if !accepted {
        return;
    }
    let timestamp = timestamp(record);
    let event = record
        .get("type")
        .or_else(|| record.get("event"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let message = record.get("message").unwrap_or(record);
    let role = message
        .get("role")
        .or_else(|| record.get("role"))
        .and_then(Value::as_str);
    common::observe_timestamp(state, timestamp.as_deref(), role == Some("user"));
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
            .get("cwd")
            .or_else(|| record.get("workdir"))
            .and_then(Value::as_str),
    );

    if role == Some("user") {
        common::consider_title(
            state,
            message
                .get("content")
                .or_else(|| record.get("content"))
                .and_then(common::text_from_message)
                .as_deref(),
        );
    } else if role == Some("assistant") {
        common::consider_result(
            state,
            message
                .get("content")
                .or_else(|| record.get("content"))
                .and_then(common::text_from_message)
                .as_deref(),
        );
    }
    if let Some(model) = record
        .get("model")
        .or_else(|| message.get("model"))
        .and_then(Value::as_str)
    {
        common::set_model(state, Some(model));
    }
    if event.contains("tool") {
        if event.contains("result") || event.contains("output") {
            let failed = record.get("error").is_some()
                || record.get("isError").and_then(Value::as_bool) == Some(true);
            common::record_tool_result(
                state,
                !failed,
                record.get("duration_ms").and_then(Value::as_u64),
                timestamp.as_deref(),
            );
        } else {
            common::record_tool_with_source(
                state,
                record
                    .get("name")
                    .or_else(|| record.get("tool_name"))
                    .and_then(Value::as_str)
                    .unwrap_or("tool"),
                record.get("arguments"),
                timestamp.as_deref(),
                source_record_id.as_deref(),
            );
        }
    }
    if let Some(usage) = record.get("usage") {
        let values = TokenUsage {
            input_tokens: number(usage, "input_tokens", "input"),
            output_tokens: number(usage, "output_tokens", "output"),
            cache_read_tokens: number(usage, "cache_read_tokens", "cache_read"),
            cache_write_tokens: number(usage, "cache_write_tokens", "cache_write"),
            cache_write_1h_tokens: 0,
            reasoning_tokens: number(usage, "reasoning_tokens", "reasoning"),
        };
        common::record_usage(
            state,
            &values,
            timestamp.as_deref(),
            state.current_model.clone().as_deref(),
        );
    }
}

fn number(value: &Value, first: &str, second: &str) -> u64 {
    value
        .get(first)
        .or_else(|| value.get(second))
        .and_then(Value::as_u64)
        .unwrap_or(0)
}
fn timestamp(record: &Value) -> Option<String> {
    record
        .get("timestamp")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            record
                .get("created_at")
                .or_else(|| record.get("time"))
                .and_then(Value::as_i64)
                .and_then(|millis| {
                    DateTime::<Utc>::from_timestamp_millis(millis)
                        .map(|time| time.to_rfc3339_opts(SecondsFormat::Millis, true))
                })
        })
}
