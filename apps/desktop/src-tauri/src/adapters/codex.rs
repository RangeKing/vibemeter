use crate::adapters::common;
use crate::models::{ParseState, TokenUsage};
use serde_json::Value;

pub fn parse_record(state: &mut ParseState, record: &Value) {
    let timestamp = record.get("timestamp").and_then(Value::as_str);
    let record_type = record
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let payload = record.get("payload").unwrap_or(&Value::Null);
    let payload_type = payload
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();

    let human = record_type == "event_msg" && payload_type == "user_message";
    common::observe_timestamp(state, timestamp, human);

    match record_type {
        "session_meta" => {
            // Forked/subagent rollouts can replay the parent session_meta later in the
            // file. The first metadata record is the durable identity of this rollout;
            // allowing a later parent record to overwrite it collapses multiple local
            // token streams into one database row.
            if !state.source_session_observed {
                let source_session_id = payload
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty());
                if source_session_id.is_some() {
                    common::set_source_session(state, source_session_id);
                    state.source_session_observed = true;
                }
            }
            common::set_project(state, payload.get("cwd").and_then(Value::as_str));
        }
        "turn_context" => {
            common::set_model(state, payload.get("model").and_then(Value::as_str));
            common::set_project(state, payload.get("cwd").and_then(Value::as_str));
        }
        "event_msg" => parse_event_message(state, payload, timestamp),
        "response_item" => parse_response_item(state, payload, timestamp),
        "compacted" => common::record_context_compaction(state, timestamp),
        _ => common::mark_unknown(state),
    }
}

fn parse_event_message(state: &mut ParseState, payload: &Value, timestamp: Option<&str>) {
    match payload
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "user_message" => {
            let text = payload
                .get("message")
                .or_else(|| payload.get("text"))
                .and_then(common::text_from_message);
            common::consider_title(state, text.as_deref());
        }
        "token_count" => {
            let Some(total) = payload
                .get("info")
                .and_then(|info| info.get("total_token_usage"))
            else {
                return;
            };
            let cached = number(total, "cached_input_tokens");
            let current = TokenUsage {
                input_tokens: number(total, "input_tokens").saturating_sub(cached),
                output_tokens: number(total, "output_tokens"),
                cache_read_tokens: cached,
                cache_write_tokens: 0,
                cache_write_1h_tokens: 0,
                reasoning_tokens: number(total, "reasoning_output_tokens"),
            };
            let delta = current.saturating_delta(&state.previous_codex_total);
            state.previous_codex_total = current;
            let model = state.current_model.clone();
            common::record_usage(state, &delta, timestamp, model.as_deref());
        }
        "agent_message" => {
            let text = payload
                .get("message")
                .or_else(|| payload.get("text"))
                .and_then(common::text_from_message);
            common::consider_result(state, text.as_deref());
        }
        "context_compacted" => common::record_context_compaction(state, timestamp),
        "task_started" => common::record_task_start(state, timestamp),
        "task_complete" => {
            common::record_task_complete(state, number_option(payload, "duration_ms"), timestamp)
        }
        "turn_aborted" => common::record_task_abort(state, timestamp),
        "thread_rolled_back" => common::record_rollback(state, timestamp),
        "thread_goal_updated" => common::record_goal_change(state, timestamp),
        "sub_agent_activity" => common::record_subagent_activity(
            state,
            payload
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or("unknown"),
            timestamp,
        ),
        "mcp_tool_call_end" => {
            let success = payload
                .get("result")
                .and_then(|result| result.get("is_error").or_else(|| result.get("isError")))
                .and_then(Value::as_bool)
                != Some(true);
            common::record_tool_result(
                state,
                success,
                number_option(payload, "duration"),
                timestamp,
            );
        }
        "web_search_end" => common::record_tool(state, "web_search", None, timestamp),
        "agent_reasoning"
        | "thread_settings_applied"
        | "item_completed"
        | "image_generation_end" => {}
        "error" => common::record_error(state, timestamp),
        "retry" => common::increment_retry(state),
        "patch_apply_end" => common::record_codex_patch_result(state, payload, timestamp),
        _ => common::mark_unknown(state),
    }
}

fn parse_response_item(state: &mut ParseState, payload: &Value, timestamp: Option<&str>) {
    match payload
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "custom_tool_call" => {
            let tool_id = common::normalized_tool_id(payload);
            if tool_id
                .as_ref()
                .is_some_and(|tool_id| !state.seen_tool_ids.insert(tool_id.clone()))
            {
                return;
            }
            let name = payload
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("other");
            let raw_input = payload.get("input").and_then(Value::as_str);
            let input = raw_input
                .map(common::parsed_object_from_string)
                .unwrap_or(Value::Null);
            if name.contains("apply_patch") {
                if let Some(patch) = input
                    .as_str()
                    .or_else(|| input.get("patch").and_then(Value::as_str))
                    .or_else(|| input.get("input").and_then(Value::as_str))
                {
                    common::inspect_codex_requested_patch(state, patch);
                }
                common::record_tool_with_source(state, name, None, timestamp, tool_id.as_deref());
            } else if name == "exec"
                && let Some(source) = raw_input
            {
                let patches = common::inspect_codex_exec_patches(state, source);
                let commands = common::record_codex_exec_commands(
                    state,
                    source,
                    timestamp,
                    tool_id.as_deref(),
                );
                if patches == 0 && commands == 0 {
                    common::record_tool_with_source(
                        state,
                        name,
                        Some(&input),
                        timestamp,
                        tool_id.as_deref(),
                    );
                } else if patches > 0 {
                    for index in 0..patches {
                        let patch_id = tool_id.as_deref().map(|tool_id| {
                            common::derived_source_event_id(tool_id, "apply-patch", index)
                        });
                        common::record_tool_with_source(
                            state,
                            "apply_patch",
                            None,
                            timestamp,
                            patch_id.as_deref(),
                        );
                    }
                }
            } else {
                common::record_tool_with_source(
                    state,
                    name,
                    Some(&input),
                    timestamp,
                    tool_id.as_deref(),
                );
            }
        }
        "function_call" => {
            let tool_id = common::normalized_tool_id(payload);
            if tool_id
                .as_ref()
                .is_some_and(|tool_id| !state.seen_tool_ids.insert(tool_id.clone()))
            {
                return;
            }
            let name = payload
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("other");
            let input = payload
                .get("arguments")
                .and_then(Value::as_str)
                .map(common::parsed_object_from_string)
                .unwrap_or(Value::Null);
            if name.contains("apply_patch") {
                if let Some(patch) = input
                    .as_str()
                    .or_else(|| input.get("patch").and_then(Value::as_str))
                    .or_else(|| input.get("input").and_then(Value::as_str))
                {
                    common::inspect_codex_requested_patch(state, patch);
                }
                common::record_tool_with_source(state, name, None, timestamp, tool_id.as_deref());
            } else {
                common::record_tool_with_source(
                    state,
                    name,
                    Some(&input),
                    timestamp,
                    tool_id.as_deref(),
                );
            }
        }
        "custom_tool_call_output" | "function_call_output" => {
            if let Some((success, duration_ms)) = tool_output_result(payload) {
                common::record_tool_result(state, success, duration_ms, timestamp);
            }
        }
        "agent_message" => {
            state.behavior.subagent_interactions =
                state.behavior.subagent_interactions.saturating_add(1);
        }
        "message" => {
            if payload.get("role").and_then(Value::as_str) == Some("assistant") {
                let text = payload
                    .get("content")
                    .or_else(|| payload.get("message"))
                    .and_then(common::text_from_message);
                common::consider_result(state, text.as_deref());
            }
        }
        "reasoning" | "web_search_call" => {}
        _ => common::mark_unknown(state),
    }
}

fn number(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn number_option(value: &Value, key: &str) -> Option<u64> {
    value.get(key).and_then(|item| {
        item.as_u64().or_else(|| {
            item.as_f64()
                .filter(|raw| raw.is_finite() && *raw >= 0.0)
                .map(|raw| raw.round() as u64)
        })
    })
}

fn tool_output_result(payload: &Value) -> Option<(bool, Option<u64>)> {
    if payload
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|status| status == "failed")
    {
        return Some((false, None));
    }
    let output = payload.get("output").and_then(Value::as_str)?;
    if let Ok(parsed) = serde_json::from_str::<Value>(output) {
        let timed_out = parsed
            .get("timed_out")
            .or_else(|| parsed.get("timedOut"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let is_error = parsed
            .get("is_error")
            .or_else(|| parsed.get("isError"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let exit_success = parsed
            .get("exit_code")
            .or_else(|| parsed.get("exitCode"))
            .and_then(Value::as_i64)
            .is_none_or(|code| code == 0);
        let duration_ms = parsed
            .get("wall_time_seconds")
            .or_else(|| parsed.get("wallTimeSeconds"))
            .and_then(Value::as_f64)
            .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
            .map(|seconds| (seconds * 1_000.0).round() as u64);
        return Some((!timed_out && !is_error && exit_success, duration_ms));
    }
    if output.starts_with("Script completed") {
        return Some((true, None));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::AgentKind;
    use serde_json::json;

    #[test]
    fn keeps_the_first_rollout_identity_when_parent_metadata_is_replayed() {
        let mut state = ParseState::new(AgentKind::Codex, "fallback".into());
        for id in ["child-rollout", "parent-rollout"] {
            parse_record(
                &mut state,
                &json!({
                    "type": "session_meta",
                    "timestamp": "2026-07-23T00:00:00Z",
                    "payload": {"id": id}
                }),
            );
        }

        assert_eq!(state.source_session_id, "child-rollout");
        assert!(state.source_session_observed);
    }

    #[test]
    fn observes_patch_evidence_inside_the_codex_exec_wrapper() {
        let mut state = ParseState::new(AgentKind::Codex, "session".into());
        let source = r#"const patch = "*** Begin Patch\n*** Update File: src/app.ts\n@@\n-old value\n+new value\n+another line\n*** End Patch"; const result = await tools.apply_patch(patch); text(result);"#;
        parse_record(
            &mut state,
            &json!({
                "type": "response_item",
                "timestamp": "2026-07-19T00:00:00Z",
                "payload": {
                    "type": "custom_tool_call",
                    "call_id": "call-1",
                    "name": "exec",
                    "input": source
                }
            }),
        );
        assert_eq!(state.lines_added, 2);
        assert_eq!(state.lines_deleted, 1);
        assert_eq!(state.touched_file_hashes.len(), 1);
        assert_eq!(state.tool_counts.get("edit"), Some(&1));
    }

    #[test]
    fn ignores_apply_patch_text_that_is_only_inside_a_command_string() {
        let mut state = ParseState::new(AgentKind::Codex, "session".into());
        let source = r#"const result = await tools.exec_command({"cmd":"printf 'tools.apply_patch(patch)'"}); text(result.output);"#;
        parse_record(
            &mut state,
            &json!({
                "type": "response_item",
                "timestamp": "2026-07-19T00:00:00Z",
                "payload": {
                    "type": "custom_tool_call",
                    "call_id": "call-2",
                    "name": "exec",
                    "input": source
                }
            }),
        );
        assert_eq!(state.lines_added, 0);
        assert_eq!(state.lines_deleted, 0);
        assert_eq!(state.tool_counts.get("shell"), Some(&1));
    }

    #[test]
    fn observes_wrapped_patch_after_multibyte_source_text() {
        let mut state = ParseState::new(AgentKind::Codex, "session".into());
        let record = serde_json::json!({
            "timestamp": "2026-07-19T00:00:00Z",
            "type": "response_item",
            "payload": {
                "type": "custom_tool_call",
                "name": "exec",
                "input": "const note = '先检查中文路径';\nconst patch = \"*** Begin Patch\\n*** Update File: src/main.rs\\n@@\\n-old\\n+new\\n*** End Patch\";\ntext(await tools.apply_patch(patch));"
            }
        });

        parse_record(&mut state, &record);

        assert_eq!(state.lines_added, 1);
        assert_eq!(state.lines_deleted, 1);
        assert_eq!(state.touched_file_hashes.len(), 1);
        assert_eq!(state.tool_counts.get("edit"), Some(&1));
    }

    #[test]
    fn successful_patch_result_replaces_requested_line_estimate() {
        let mut state = ParseState::new(AgentKind::Codex, "session".into());
        let request = serde_json::json!({
            "type": "response_item",
            "timestamp": "2026-07-19T00:00:00Z",
            "payload": {
                "type": "custom_tool_call",
                "call_id": "call-1",
                "name": "apply_patch",
                "input": "*** Begin Patch\n*** Update File: src/app.ts\n@@\n-old\n+new\n+requested-only\n*** End Patch"
            }
        });
        let result = serde_json::json!({
            "type": "event_msg",
            "timestamp": "2026-07-19T00:00:01Z",
            "payload": {
                "type": "patch_apply_end",
                "call_id": "call-1",
                "status": "completed",
                "success": true,
                "changes": {
                    "src/app.ts": {
                        "type": "update",
                        "move_path": null,
                        "unified_diff": "@@\n-old\n+new"
                    }
                }
            }
        });

        parse_record(&mut state, &request);
        assert_eq!((state.lines_added, state.lines_deleted), (2, 1));
        parse_record(&mut state, &result);

        assert_eq!((state.lines_added, state.lines_deleted), (1, 1));
        assert_eq!(state.codex_patch_result_events, 1);
        assert_eq!(state.touched_file_hashes.len(), 1);
    }

    #[test]
    fn failed_patch_result_removes_requested_lines() {
        let mut state = ParseState::new(AgentKind::Codex, "session".into());
        parse_record(
            &mut state,
            &serde_json::json!({
                "type": "response_item",
                "timestamp": "2026-07-19T00:00:00Z",
                "payload": {
                    "type": "custom_tool_call",
                    "call_id": "call-1",
                    "name": "apply_patch",
                    "input": "*** Begin Patch\n*** Update File: src/app.ts\n@@\n-old\n+new\n*** End Patch"
                }
            }),
        );
        parse_record(
            &mut state,
            &serde_json::json!({
                "type": "event_msg",
                "timestamp": "2026-07-19T00:00:01Z",
                "payload": {
                    "type": "patch_apply_end",
                    "call_id": "call-1",
                    "status": "failed",
                    "success": false,
                    "changes": {}
                }
            }),
        );

        assert_eq!((state.lines_added, state.lines_deleted), (0, 0));
        assert_eq!(state.errors, 1);
    }

    #[test]
    fn classifies_each_command_inside_the_codex_exec_wrapper() {
        let mut state = ParseState::new(AgentKind::Codex, "session".into());
        parse_record(
            &mut state,
            &serde_json::json!({
                "type": "response_item",
                "timestamp": "2026-07-19T00:00:00Z",
                "payload": {
                    "type": "custom_tool_call",
                    "call_id": "call-commands",
                    "name": "exec",
                    "input": "const results = await Promise.all([tools.exec_command({cmd: \"cargo test\"}), tools.exec_command({cmd: \"rg -n 'cargo test' README.md\"}), tools.exec_command({command: \"npm run build\"})]);"
                }
            }),
        );

        assert_eq!(state.tool_counts.get("test"), Some(&1));
        assert_eq!(state.tool_counts.get("shell"), Some(&1));
        assert_eq!(state.tool_counts.get("build"), Some(&1));
        assert_eq!(state.verification_events, 2);
    }
}
