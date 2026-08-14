use crate::adapters::common;
use crate::models::{ParseState, TokenUsage};
use chrono::{SecondsFormat, Utc};
use serde_json::Value;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::Path;

const MAX_RECORD_BYTES: usize = 32 * 1024 * 1024;

pub fn parse_record(state: &mut ParseState, record: &Value) {
    if record.get("version").is_some() && record.get("id").is_some() {
        common::set_source_session(state, record.get("id").and_then(Value::as_str));
        state.source_session_observed = true;
        common::set_project(state, record.get("cwd").and_then(Value::as_str));
        let timestamp = timestamp(record.get("createdAt"));
        common::observe_timestamp(state, timestamp.as_deref(), false);
        return;
    }

    let event_type = record
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let data = record.get("data").unwrap_or(&Value::Null);
    let timestamp = timestamp(record.get("time"));
    let source_event_id = record
        .get("seq")
        .and_then(Value::as_u64)
        .map(|seq| seq.to_string());
    let human = event_type == "user/message"
        && data
            .get("source")
            .and_then(|source| source.get("kind"))
            .and_then(Value::as_str)
            == Some("user");
    common::observe_timestamp(state, timestamp.as_deref(), human);

    match event_type {
        "session/title" => {
            common::set_observed_title(state, data.get("title").and_then(Value::as_str))
        }
        "request/context" => common::set_model(state, data.get("model").and_then(Value::as_str)),
        "user/message" if human => {
            let text = data.get("content").and_then(common::text_from_message);
            common::consider_title(state, text.as_deref());
        }
        // Harness also serializes injected plugin and system context as
        // user/message records. They are known transport input, not malformed
        // history and never count as human intervention or phrase evidence.
        "user/message" => {}
        "assistant/message" => {
            let text = data
                .get("message")
                .and_then(|message| message.get("content"))
                .and_then(common::text_from_message);
            common::consider_result(state, text.as_deref());
            if let Some(usage) = data.get("usage") {
                let usage = TokenUsage {
                    input_tokens: number(usage, "inputTokens"),
                    output_tokens: number(usage, "outputTokens"),
                    cache_read_tokens: number(usage, "cacheReadTokens"),
                    cache_write_tokens: number(usage, "cacheWriteTokens"),
                    cache_write_1h_tokens: number(usage, "cacheWrite1hTokens"),
                    reasoning_tokens: number(usage, "reasoningTokens"),
                };
                let model = state.current_model.clone();
                common::record_usage(state, &usage, timestamp.as_deref(), model.as_deref());
            }
        }
        "turn/start" => common::record_task_start(state, timestamp.as_deref()),
        "turn/end" => match data
            .get("reason")
            .and_then(|reason| reason.get("kind"))
            .and_then(Value::as_str)
        {
            Some("completed") | Some("max-tokens") => {
                common::record_task_complete(state, None, timestamp.as_deref())
            }
            Some("error") | Some("blocked") => {
                common::record_error(state, timestamp.as_deref());
                common::record_task_abort(state, timestamp.as_deref());
            }
            Some("aborted") => common::record_task_abort(state, timestamp.as_deref()),
            _ => {}
        },
        "tool/call" => {
            let name = data.get("name").and_then(Value::as_str).unwrap_or("tool");
            let input = data
                .get("arguments")
                .and_then(Value::as_str)
                .map(common::parsed_object_from_string);
            common::record_tool_with_source(
                state,
                name,
                input.as_ref(),
                timestamp.as_deref(),
                source_event_id.as_deref(),
            );
        }
        "tool/result" => common::record_tool_result(
            state,
            data.get("error").is_none_or(Value::is_null),
            None,
            timestamp.as_deref(),
        ),
        "approval/asked" => common::record_event_with_source(
            state,
            "waiting",
            "needs-you",
            "approval",
            None,
            timestamp.as_deref(),
            source_event_id.as_deref(),
        ),
        "compaction/start" => common::record_context_compaction(state, timestamp.as_deref()),
        "subagent/descriptor" | "tool-workflow/agent-start" => {
            common::record_subagent_activity(state, "started", timestamp.as_deref())
        }
        "tool-workflow/agent-end" => {
            common::record_subagent_activity(state, "interacted", timestamp.as_deref())
        }
        "step/start"
        | "step/end"
        | "reasoning-chunks"
        | "text-chunks"
        | "tool-call-chunks"
        | "approval/decided"
        | "command/run"
        | "command/done"
        | "todo/write"
        | "request/header"
        | "assistant/chunk"
        | "agent/inbox/spliced"
        | "permission/preset"
        | "approval/policy"
        | "sandbox/mode"
        | "agent-preset/selected"
        | "session/title-llm-request"
        | "compaction/end"
        | "compaction/summary"
        | "compaction/prune"
        | "tool/code-dispatch"
        | "tool/code-dispatch-start"
        | "tool-workflow/run-start"
        | "tool-workflow/run-end" => {}
        _ => common::mark_unknown(state),
    }
}

pub(crate) fn read_records(path: &Path, mut visit: impl FnMut(&Value)) -> io::Result<()> {
    let decoder = zstd::stream::read::Decoder::new(File::open(path)?)?;
    let mut reader = BufReader::with_capacity(256 * 1024, decoder);
    let mut buffer = Vec::with_capacity(64 * 1024);
    loop {
        buffer.clear();
        match reader.read_until(b'\n', &mut buffer) {
            Ok(0) => break,
            Ok(_) if buffer.len() > MAX_RECORD_BYTES => continue,
            Ok(_) => {
                if let Ok(record) = serde_json::from_slice::<Value>(&buffer) {
                    visit(&record);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn timestamp(value: Option<&Value>) -> Option<String> {
    let value = value?;
    let milliseconds = value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))?;
    chrono::DateTime::<Utc>::from_timestamp_millis(milliseconds)
        .map(|value| value.to_rfc3339_opts(SecondsFormat::Millis, true))
}

fn number(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::AgentKind;
    use serde_json::json;

    #[test]
    fn maps_structured_history_without_counting_injected_context_as_human_input() {
        let mut state = ParseState::new(AgentKind::DeepSeekHarness, "fallback".into());
        for record in [
            json!({"version":0,"id":"session-dsh","createdAt":1775000000000_i64,"cwd":"/tmp/project"}),
            json!({"type":"turn/start","seq":1,"time":1775000001000_i64,"data":{"turn":1}}),
            json!({"type":"user/message","seq":2,"time":1775000001001_i64,"data":{"source":{"kind":"plugin"},"content":[{"type":"text","text":"private system context"}]}}),
            json!({"type":"user/message","seq":3,"time":1775000001002_i64,"data":{"source":{"kind":"user"},"content":[{"type":"text","text":"Build the feature"}]}}),
            json!({"type":"request/context","seq":4,"time":1775000001003_i64,"data":{"provider":"deepseek-official","model":"deepseek-v4-pro"}}),
            json!({"type":"tool/call","seq":5,"time":1775000002000_i64,"data":{"name":"write","arguments":"{\"path\":\"src/app.ts\",\"content\":\"x\"}"}}),
            json!({"type":"tool/result","seq":6,"time":1775000002500_i64,"data":{"message":{"content":[]}}}),
            json!({"type":"assistant/message","seq":7,"time":1775000003000_i64,"data":{"message":{"content":[{"type":"text","text":"Implemented"}]},"usage":{"inputTokens":10,"outputTokens":4,"cacheReadTokens":3,"reasoningTokens":2}}}),
            json!({"type":"turn/end","seq":8,"time":1775000004000_i64,"data":{"turn":1,"reason":{"kind":"completed"}}}),
        ] {
            parse_record(&mut state, &record);
        }

        assert_eq!(state.source_session_id, "session-dsh");
        assert_eq!(state.project_label.as_deref(), Some("project"));
        assert_eq!(state.title.as_deref(), Some("Build the feature"));
        assert_eq!(state.result_excerpt.as_deref(), Some("Implemented"));
        assert_eq!(state.human_interventions, 1);
        assert_eq!(state.current_model.as_deref(), Some("deepseek-v4-pro"));
        assert_eq!(state.usage.input_tokens, 10);
        assert_eq!(state.usage.cache_read_tokens, 3);
        assert_eq!(state.tool_calls, 1);
        assert_eq!(state.behavior.task_completions, 1);
        assert_eq!(state.unknown_records, 0);
        assert!(!format!("{state:?}").contains("private system context"));
    }
}
