use crate::models::{CanonicalEvent, FileChangeAccumulator, ParseState, TokenUsage};
use crate::pricing;
use crate::privacy::{
    safe_project_relative_path, sanitize_prompt_excerpt, sanitize_result_excerpt, sanitize_title,
    sanitize_tool_name, stable_hash,
};
use chrono::{DateTime, Local, SecondsFormat, Utc};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::Path;

const ACTIVE_GAP_SECONDS: i64 = 10 * 60;
const MAX_EVENTS_PER_SESSION: usize = 4_000;

pub fn observe_timestamp(state: &mut ParseState, timestamp: Option<&str>, human: bool) {
    let Some(timestamp) = timestamp.filter(|value| !value.is_empty()) else {
        return;
    };
    let parsed = DateTime::parse_from_rfc3339(timestamp).ok();

    if state
        .started_at
        .as_ref()
        .is_none_or(|current| timestamp < current.as_str())
    {
        state.started_at = Some(timestamp.to_string());
    }
    if state
        .ended_at
        .as_ref()
        .is_none_or(|current| timestamp > current.as_str())
    {
        state.ended_at = Some(timestamp.to_string());
    }

    if let (Some(current), Some(previous)) = (
        parsed.as_ref(),
        state
            .last_timestamp
            .as_deref()
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok()),
    ) {
        let gap = current.signed_duration_since(previous).num_seconds();
        if (0..=ACTIVE_GAP_SECONDS).contains(&gap) {
            state.active_seconds = state.active_seconds.saturating_add(gap as u64);
            let date = previous
                .with_timezone(&Local)
                .date_naive()
                .format("%Y-%m-%d")
                .to_string();
            state.daily.entry(date).or_default().active_seconds = state
                .daily
                .get(
                    &previous
                        .with_timezone(&Local)
                        .date_naive()
                        .format("%Y-%m-%d")
                        .to_string(),
                )
                .map_or(gap as u64, |aggregate| {
                    aggregate.active_seconds.saturating_add(gap as u64)
                });
        }
    }

    if human {
        close_agent_run(state, timestamp);
        state.human_interventions = state.human_interventions.saturating_add(1);
        record_event(
            state,
            "prompt",
            "understand",
            "user",
            Some(true),
            Some(timestamp),
        );
    } else if state.current_run_started_at.is_none() {
        state.current_run_started_at = Some(timestamp.to_string());
    }

    state.last_timestamp = Some(timestamp.to_string());
    if let Some(parsed) = parsed {
        let date = parsed
            .with_timezone(&Local)
            .date_naive()
            .format("%Y-%m-%d")
            .to_string();
        let aggregate = state.daily.entry(date).or_default();
        aggregate.events = aggregate.events.saturating_add(1);
    }
    state.event_count = state.event_count.saturating_add(1);
}

fn close_agent_run(state: &mut ParseState, timestamp: &str) {
    let Some(started_at) = state.current_run_started_at.take() else {
        return;
    };
    let duration = agent_run_duration(&started_at, timestamp);
    state.longest_uninterrupted_seconds = state.longest_uninterrupted_seconds.max(duration);
}

fn agent_run_duration(started_at: &str, ended_at: &str) -> u64 {
    DateTime::parse_from_rfc3339(ended_at)
        .ok()
        .zip(DateTime::parse_from_rfc3339(started_at).ok())
        .map(|(end, start)| end.signed_duration_since(start).num_seconds())
        .unwrap_or(0)
        .max(0) as u64
}

pub fn finalize_run(state: &mut ParseState) {
    if let (Some(started_at), Some(ended_at)) = (
        state.current_run_started_at.as_deref(),
        state.ended_at.as_deref(),
    ) {
        state.longest_uninterrupted_seconds = state
            .longest_uninterrupted_seconds
            .max(agent_run_duration(started_at, ended_at));
    }
}

pub fn set_model(state: &mut ParseState, model: Option<&str>) {
    let Some(model) = model
        .map(str::trim)
        .filter(|model| !model.is_empty() && *model != "<synthetic>")
    else {
        return;
    };
    if state
        .current_model
        .as_deref()
        .is_some_and(|current| current != model)
    {
        state.model_switches = state.model_switches.saturating_add(1);
    }
    state.current_model = Some(model.to_string());
    *state.model_counts.entry(model.to_string()).or_default() += 1;
}

pub fn record_usage(
    state: &mut ParseState,
    usage: &TokenUsage,
    timestamp: Option<&str>,
    model: Option<&str>,
) {
    if usage.total() == 0 {
        return;
    }
    if let Some(model) = model {
        set_model(state, Some(model));
    }
    state.usage.add_assign(usage);

    let anchor = timestamp.and_then(|value| DateTime::parse_from_rfc3339(value).ok());

    if let Some(model) = model.or(state.current_model.as_deref())
        && let Some(cost) = pricing::estimate_cost(state.agent, model, usage)
    {
        state.estimated_cost_usd += cost;
        state.cost_coverage_tokens = state.cost_coverage_tokens.saturating_add(usage.total());
        if let Some(anchor) = anchor.as_ref() {
            let date = anchor
                .with_timezone(&Local)
                .date_naive()
                .format("%Y-%m-%d")
                .to_string();
            let aggregate = state.daily.entry(date).or_default();
            aggregate.estimated_cost_usd = Some(aggregate.estimated_cost_usd.unwrap_or(0.0) + cost);
        }
    }

    let Some(anchor) = anchor else {
        return;
    };
    let date = anchor
        .with_timezone(&Local)
        .date_naive()
        .format("%Y-%m-%d")
        .to_string();
    state.daily.entry(date).or_default().usage.add_assign(usage);
    let hour = anchor
        .with_timezone(&Local)
        .format("%Y-%m-%dT%H:00")
        .to_string();
    state.hourly.entry(hour).or_default().add_assign(usage);
}

pub fn consider_title(state: &mut ParseState, text: Option<&str>) {
    let Some(text) = text else {
        return;
    };
    let trimmed = text.trim();
    crate::skill_usage::observe_explicit_invocations(state, trimmed);
    if trimmed.is_empty()
        || trimmed.starts_with('<')
        || trimmed.starts_with("You are ")
        || trimmed.contains("<system-reminder>")
    {
        return;
    }
    crate::phrases::observe(state, "user", trimmed);
    observe_prompt_structure(state, trimmed);
    if state.prompt_excerpt.is_none() {
        state.prompt_excerpt = sanitize_prompt_excerpt(trimmed);
    }
    if state.title.is_none() {
        state.title = sanitize_title(trimmed);
    }
}

fn observe_prompt_structure(state: &mut ParseState, text: &str) {
    let behavior = &mut state.behavior;
    behavior.prompt_count = behavior.prompt_count.saturating_add(1);
    if behavior.first_prompt_at.is_none() {
        behavior.first_prompt_at = state.last_timestamp.clone();
    }
    if !behavior.prompt_structure_enabled {
        return;
    }
    behavior.prompt_characters = behavior
        .prompt_characters
        .saturating_add(text.chars().count() as u64);
    let lower = text.to_ascii_lowercase();
    let lines = text.lines().map(str::trim).collect::<Vec<_>>();
    let structured = lines.len() >= 3
        || lines.iter().any(|line| {
            line.starts_with("- ")
                || line.starts_with("* ")
                || line.starts_with("1.")
                || line.starts_with("1、")
                || line.starts_with("##")
        })
        || text.contains("```");
    if structured {
        behavior.structured_prompts = behavior.structured_prompts.saturating_add(1);
    }
    let acceptance = [
        "验收",
        "完成条件",
        "必须通过",
        "成功标准",
        "acceptance",
        "done when",
        "must pass",
        "definition of done",
    ]
    .iter()
    .any(|keyword| lower.contains(keyword));
    if acceptance {
        behavior.acceptance_criteria_prompts =
            behavior.acceptance_criteria_prompts.saturating_add(1);
    }
    let file_scope = [
        "文件",
        "目录",
        "模块",
        "不要修改",
        "只修改",
        "file",
        "folder",
        "directory",
        "module",
        "do not change",
        "only change",
    ]
    .iter()
    .any(|keyword| lower.contains(keyword));
    if file_scope {
        behavior.file_scope_prompts = behavior.file_scope_prompts.saturating_add(1);
    }
}

pub fn set_prompt_structure_enabled(state: &mut ParseState, enabled: bool) {
    state.behavior.prompt_structure_enabled = enabled;
}

pub fn consider_result(state: &mut ParseState, text: Option<&str>) {
    let Some(text) = text.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    crate::phrases::observe(state, "agent", text);
    state.result_excerpt = sanitize_result_excerpt(text);
}

pub fn append_result(state: &mut ParseState, text: Option<&str>) {
    let Some(text) = text.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    crate::phrases::observe(state, "agent", text);
    let combined = state
        .result_excerpt
        .as_deref()
        .map_or_else(|| text.to_string(), |current| format!("{current} {text}"));
    state.result_excerpt = sanitize_result_excerpt(&combined);
}

pub fn set_project(state: &mut ParseState, path: Option<&str>) {
    let Some(path) = path.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    let root = Path::new(path);
    state.project_root = Some(root.to_path_buf());
    if state.project_hash.is_none() {
        state.project_hash = Some(stable_hash(path));
    }
    if state.project_label.is_none() {
        state.project_label = root
            .file_name()
            .and_then(|value| value.to_str())
            .and_then(sanitize_title);
    }
    if state.ignore_patterns.is_empty() {
        for name in [".gitignore", ".vibemeterignore", ".aftervibeignore"] {
            if let Ok(content) = std::fs::read_to_string(root.join(name)) {
                state.ignore_patterns.extend(
                    content
                        .lines()
                        .map(str::trim)
                        .filter(|line| !line.is_empty() && !line.starts_with('#'))
                        .map(ToString::to_string),
                );
            }
        }
    }
}

pub fn record_event(
    state: &mut ParseState,
    event_type: &str,
    category: &str,
    name: &str,
    success: Option<bool>,
    timestamp: Option<&str>,
) {
    record_event_with_source(state, event_type, category, name, success, timestamp, None);
}

pub fn record_event_with_source(
    state: &mut ParseState,
    event_type: &str,
    category: &str,
    name: &str,
    success: Option<bool>,
    timestamp: Option<&str>,
    source_event_id: Option<&str>,
) {
    if state.events.len() >= MAX_EVENTS_PER_SESSION {
        return;
    }
    let sanitized_name = sanitize_tool_name(name);
    let fingerprint_base = source_event_fingerprint_base(
        event_type,
        category,
        &sanitized_name,
        success,
        None,
        timestamp,
        "observed",
    );
    let fingerprint_ordinal = state
        .events
        .iter()
        .filter(|event| {
            event.event_type == event_type
                && event.category == category
                && event.name == sanitized_name
                && event.success == success
                && event.occurred_at.as_deref() == timestamp
        })
        .count();
    state.events.push(CanonicalEvent {
        sequence: state.events.len() as u64 + 1,
        source_event_id: source_event_id.map(ToString::to_string),
        source_event_fingerprint: Some(if fingerprint_ordinal == 0 {
            fingerprint_base
        } else {
            stable_hash(&format!("{fingerprint_base}|{fingerprint_ordinal}"))
        }),
        occurred_at: timestamp.map(ToString::to_string),
        event_type: event_type.into(),
        category: category.into(),
        name: sanitized_name,
        success,
        duration_ms: None,
        provenance: "observed".into(),
    });
}

#[allow(clippy::too_many_arguments)]
pub fn source_event_fingerprint_base(
    event_type: &str,
    category: &str,
    name: &str,
    success: Option<bool>,
    duration_ms: Option<u64>,
    timestamp: Option<&str>,
    provenance: &str,
) -> String {
    let timestamp = timestamp
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| {
            value
                .with_timezone(&Utc)
                .to_rfc3339_opts(SecondsFormat::AutoSi, true)
        })
        .unwrap_or_else(|| "time-unavailable".into());
    stable_hash(&format!(
        "{event_type}|{category}|{}|{}|{}|{timestamp}|{provenance}",
        sanitize_tool_name(name),
        success.map(i64::from).unwrap_or(-1),
        duration_ms.unwrap_or_default(),
    ))
}

pub fn text_from_message(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return Some(text.to_string());
    }
    let items = value.as_array()?;
    items.iter().find_map(|item| {
        let kind = item.get("type").and_then(Value::as_str).unwrap_or_default();
        if kind == "text" || kind == "input_text" || kind == "output_text" {
            item.get("text")
                .and_then(Value::as_str)
                .map(ToString::to_string)
        } else {
            None
        }
    })
}

pub fn record_error(state: &mut ParseState, timestamp: Option<&str>) {
    state.errors = state.errors.saturating_add(1);
    if let Some(date) = local_date(timestamp) {
        state.daily.entry(date).or_default().errors = state
            .daily
            .get(&local_date(timestamp).unwrap_or_default())
            .map_or(1, |aggregate| aggregate.errors.saturating_add(1));
    }
    record_event(state, "error", "error", "error", Some(false), timestamp);
}

pub fn record_tool_result(
    state: &mut ParseState,
    success: bool,
    duration_ms: Option<u64>,
    timestamp: Option<&str>,
) {
    if success {
        state.behavior.successful_tools = state.behavior.successful_tools.saturating_add(1);
    } else {
        state.behavior.failed_tools = state.behavior.failed_tools.saturating_add(1);
        record_error(state, timestamp);
    }
    if let Some(duration_ms) = duration_ms {
        state.behavior.tool_duration_ms =
            state.behavior.tool_duration_ms.saturating_add(duration_ms);
    }
}

pub fn record_task_start(state: &mut ParseState, timestamp: Option<&str>) {
    state.behavior.task_starts = state.behavior.task_starts.saturating_add(1);
    record_event(
        state,
        "task-start",
        "execute",
        "task",
        Some(true),
        timestamp,
    );
}

pub fn record_task_complete(
    state: &mut ParseState,
    duration_ms: Option<u64>,
    timestamp: Option<&str>,
) {
    state.behavior.task_completions = state.behavior.task_completions.saturating_add(1);
    if let Some(duration_ms) = duration_ms {
        state.behavior.completed_task_duration_ms = state
            .behavior
            .completed_task_duration_ms
            .saturating_add(duration_ms);
    }
    record_event(
        state,
        "task-complete",
        "execute",
        "task",
        Some(true),
        timestamp,
    );
}

pub fn record_task_abort(state: &mut ParseState, timestamp: Option<&str>) {
    state.behavior.task_aborts = state.behavior.task_aborts.saturating_add(1);
    record_event(
        state,
        "task-abort",
        "execute",
        "task",
        Some(false),
        timestamp,
    );
}

pub fn record_goal_change(state: &mut ParseState, timestamp: Option<&str>) {
    state.behavior.goal_changes = state.behavior.goal_changes.saturating_add(1);
    record_event(
        state,
        "goal-change",
        "understand",
        "goal",
        Some(true),
        timestamp,
    );
}

pub fn record_context_compaction(state: &mut ParseState, timestamp: Option<&str>) {
    state.behavior.context_compactions = state.behavior.context_compactions.saturating_add(1);
    record_event(
        state,
        "context-compaction",
        "plan",
        "context",
        Some(true),
        timestamp,
    );
}

pub fn record_rollback(state: &mut ParseState, timestamp: Option<&str>) {
    state.behavior.rollbacks = state.behavior.rollbacks.saturating_add(1);
    record_event(state, "rollback", "fix", "rollback", Some(true), timestamp);
}

pub fn record_subagent_activity(state: &mut ParseState, kind: &str, timestamp: Option<&str>) {
    match kind {
        "started" => {
            state.behavior.subagent_starts = state.behavior.subagent_starts.saturating_add(1);
            state.subagent_count = state.subagent_count.saturating_add(1);
        }
        "interacted" => {
            state.behavior.subagent_interactions =
                state.behavior.subagent_interactions.saturating_add(1);
        }
        "interrupted" => {
            state.behavior.subagent_interruptions =
                state.behavior.subagent_interruptions.saturating_add(1);
        }
        _ => {}
    }
    record_event(
        state,
        "subagent",
        "subagent",
        kind,
        Some(kind != "interrupted"),
        timestamp,
    );
}

pub fn record_tool(
    state: &mut ParseState,
    raw_name: &str,
    input: Option<&Value>,
    timestamp: Option<&str>,
) {
    record_tool_with_source(state, raw_name, input, timestamp, None);
}

pub fn record_tool_with_source(
    state: &mut ParseState,
    raw_name: &str,
    input: Option<&Value>,
    timestamp: Option<&str>,
    source_event_id: Option<&str>,
) {
    let name = raw_name.to_ascii_lowercase();
    let mut category = match name.as_str() {
        value if value.contains("read") || value.contains("view") => "read",
        value if value.contains("search") || value.contains("grep") || value.contains("glob") => {
            "search"
        }
        value if value.contains("edit") || value.contains("write") || value.contains("patch") => {
            "edit"
        }
        value if value.contains("task") || value.contains("agent") => "subagent",
        value if value.contains("web") || value.contains("browser") => "web",
        value if value.contains("bash") || value.contains("exec") || value.contains("command") => {
            "shell"
        }
        _ => "other",
    };

    let command = input.and_then(extract_command);
    if let Some(command) = command {
        category = classify_command(command);
    }
    if category == "subagent" {
        state.subagent_count = state.subagent_count.saturating_add(1);
    }
    if name.contains("update_plan")
        || name.contains("todolist")
        || name.contains("todo_write")
        || name == "plan"
    {
        state.behavior.plan_events = state.behavior.plan_events.saturating_add(1);
    }
    if state.behavior.first_tool_at.is_none() {
        state.behavior.first_tool_at = timestamp.map(ToString::to_string);
        state.behavior.time_to_first_tool_ms = timestamp
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .zip(
                state
                    .behavior
                    .first_prompt_at
                    .as_deref()
                    .and_then(|value| DateTime::parse_from_rfc3339(value).ok()),
            )
            .map(|(tool, prompt)| {
                tool.signed_duration_since(prompt).num_milliseconds().max(0) as u64
            });
    }
    if matches!(
        category,
        "test" | "build" | "lint" | "typecheck" | "git-review"
    ) {
        state.verification_events = state.verification_events.saturating_add(1);
        if let Some(date) = local_date(timestamp) {
            let aggregate = state.daily.entry(date).or_default();
            aggregate.verification_events = aggregate.verification_events.saturating_add(1);
        }
    }

    if matches!(category, "edit") {
        inspect_modification_input(state, input, timestamp);
    }
    match category {
        "deploy" => state.behavior.deploy_events = state.behavior.deploy_events.saturating_add(1),
        "dependency" => {
            state.behavior.dependency_events = state.behavior.dependency_events.saturating_add(1)
        }
        "preview" => {
            state.behavior.preview_events = state.behavior.preview_events.saturating_add(1)
        }
        _ => {}
    }
    state.tool_calls = state.tool_calls.saturating_add(1);
    *state.tool_counts.entry(category.into()).or_default() += 1;
    if let Some(date) = local_date(timestamp) {
        state.daily.entry(date).or_default().tool_calls = state
            .daily
            .get(&local_date(timestamp).unwrap_or_default())
            .map_or(1, |aggregate| aggregate.tool_calls.saturating_add(1));
    }
    record_event_with_source(
        state,
        "tool",
        category,
        raw_name,
        None,
        timestamp,
        source_event_id,
    );
}

fn extract_command(input: &Value) -> Option<&str> {
    input
        .get("cmd")
        .or_else(|| input.get("command"))
        .and_then(Value::as_str)
}

fn classify_command(command: &str) -> &'static str {
    let compact = command
        .to_ascii_lowercase()
        .replace("&&", "\n")
        .replace("||", "\n")
        .replace(';', "\n");
    let commands = compact
        .lines()
        .map(strip_shell_prefix)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if commands
        .iter()
        .any(|value| value.starts_with("git diff") || value.starts_with("git status"))
    {
        "git-review"
    } else if commands.iter().any(|value| {
        value.starts_with("cargo test")
            || value.starts_with("npm test")
            || value.starts_with("npm run test")
            || value.starts_with("pnpm test")
            || value.starts_with("pnpm run test")
            || value.starts_with("yarn test")
            || value.starts_with("yarn run test")
            || value.starts_with("vitest")
            || value.starts_with("npx vitest")
            || value.starts_with("npm exec vitest")
            || value.starts_with("npm exec -- vitest")
            || value.starts_with("pytest")
            || value.starts_with("python -m pytest")
            || value.starts_with("python3 -m pytest")
            || (value.starts_with("xcodebuild")
                && value.split_whitespace().any(|word| word == "test"))
    }) {
        "test"
    } else if commands.iter().any(|value| {
        value.starts_with("cargo clippy")
            || value.starts_with("npm run lint")
            || value.starts_with("pnpm run lint")
            || value.starts_with("yarn lint")
            || value.starts_with("eslint")
            || value.starts_with("npx eslint")
    }) {
        "lint"
    } else if commands.iter().any(|value| {
        value.starts_with("tsc")
            || value.starts_with("npx tsc")
            || value.starts_with("npm run typecheck")
            || value.starts_with("npm run check")
            || value.starts_with("pnpm run typecheck")
            || value.starts_with("pnpm run check")
            || value.starts_with("yarn typecheck")
            || value.starts_with("yarn check")
            || value.starts_with("cargo check")
    }) {
        "typecheck"
    } else if commands.iter().any(|value| {
        value.starts_with("cargo build")
            || value.starts_with("npm run build")
            || value.starts_with("pnpm run build")
            || value.starts_with("yarn build")
            || value.starts_with("xcodebuild")
    }) {
        "build"
    } else if commands.iter().any(|value| {
        value.starts_with("npm install")
            || value.starts_with("npm add")
            || value.starts_with("pnpm add")
            || value.starts_with("yarn add")
            || value.starts_with("cargo add")
            || value.starts_with("pip install")
            || value.starts_with("uv add")
    }) {
        "dependency"
    } else if commands.iter().any(|value| {
        value.starts_with("npm run dev")
            || value.starts_with("pnpm dev")
            || value.starts_with("yarn dev")
            || value.starts_with("vite")
            || value.starts_with("open ")
    }) {
        "preview"
    } else if commands.iter().any(|value| {
        value.starts_with("vercel")
            || value.starts_with("netlify")
            || value.starts_with("fly deploy")
            || value.starts_with("gh release")
            || value.starts_with("cargo publish")
            || value.starts_with("npm publish")
            || value.starts_with("docker push")
            || value.contains(" tauri build")
    }) {
        "deploy"
    } else {
        "shell"
    }
}

fn strip_shell_prefix(mut command: &str) -> &str {
    command = command.trim().trim_start_matches(['(', '{', '!', ' ']);
    if let Some(rest) = command.strip_prefix("env ") {
        command = rest.trim_start();
    }
    loop {
        let Some((word, rest)) = command.split_once(char::is_whitespace) else {
            return command;
        };
        let Some((name, _)) = word.split_once('=') else {
            return command;
        };
        if name.is_empty()
            || !name
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_')
        {
            return command;
        }
        command = rest.trim_start();
    }
}

fn inspect_modification_input(
    state: &mut ParseState,
    input: Option<&Value>,
    timestamp: Option<&str>,
) {
    let Some(input) = input else {
        return;
    };
    for key in ["file_path", "path", "filePath"] {
        if let Some(path) = input.get(key).and_then(Value::as_str) {
            state.touched_file_hashes.insert(stable_hash(path));
            let added = input
                .get("new_string")
                .or_else(|| input.get("content"))
                .and_then(Value::as_str)
                .map_or(0, |value| value.lines().count() as u64);
            let deleted = input
                .get("old_string")
                .and_then(Value::as_str)
                .map_or(0, |value| value.lines().count() as u64);
            record_file_change(state, path, "modified", added, deleted, timestamp);
        }
    }
    if let Some(patch) = input
        .get("patch")
        .or_else(|| input.get("input"))
        .and_then(Value::as_str)
    {
        inspect_patch(state, patch);
    }
    if let Some(new_string) = input.get("new_string").and_then(Value::as_str) {
        state.lines_added = state
            .lines_added
            .saturating_add(new_string.lines().count() as u64);
    }
    if let Some(old_string) = input.get("old_string").and_then(Value::as_str) {
        state.lines_deleted = state
            .lines_deleted
            .saturating_add(old_string.lines().count() as u64);
    }
    if let Some(content) = input.get("content").and_then(Value::as_str) {
        state.lines_added = state
            .lines_added
            .saturating_add(content.lines().count() as u64);
    }
}

fn record_file_change(
    state: &mut ParseState,
    raw_path: &str,
    change_kind: &str,
    lines_added: u64,
    lines_deleted: u64,
    timestamp: Option<&str>,
) {
    let Some(path) = safe_project_relative_path(state.project_root.as_deref(), raw_path) else {
        return;
    };
    if ignored_path(&path, &state.ignore_patterns) {
        return;
    }
    let item = state
        .file_changes
        .entry(path.clone())
        .or_insert_with(|| FileChangeAccumulator {
            path: path.clone(),
            change_kind: change_kind.into(),
            ..FileChangeAccumulator::default()
        });
    if item.change_kind != change_kind && item.change_kind != "added" {
        item.change_kind = change_kind.into();
    }
    item.lines_added = item.lines_added.saturating_add(lines_added);
    item.lines_deleted = item.lines_deleted.saturating_add(lines_deleted);
    item.modification_count = item.modification_count.saturating_add(1);
    if item.first_observed_at.is_none() {
        item.first_observed_at = timestamp.map(ToString::to_string);
    }
    item.last_observed_at = timestamp.map(ToString::to_string);
    observe_file_category(state, &path);
}

fn observe_file_category(state: &mut ParseState, path: &str) {
    let lower = path.to_ascii_lowercase();
    let file_name = lower.rsplit('/').next().unwrap_or(&lower);
    if lower.ends_with(".md")
        || lower.ends_with(".mdx")
        || lower.contains("/docs/")
        || file_name.starts_with("readme")
    {
        state.behavior.document_events = state.behavior.document_events.saturating_add(1);
    }
    if matches!(
        file_name,
        "agents.md" | "agent.md" | "claude.md" | "cursor.md" | ".cursorrules"
    ) {
        state.behavior.instruction_file_events =
            state.behavior.instruction_file_events.saturating_add(1);
    }
    if lower.ends_with(".css")
        || lower.ends_with(".scss")
        || lower.ends_with(".sass")
        || lower.ends_with(".less")
        || lower.contains("/styles/")
        || lower.contains("/assets/")
    {
        state.behavior.style_events = state.behavior.style_events.saturating_add(1);
    }
    if lower.contains("/test")
        || lower.contains("/tests/")
        || lower.contains("/__tests__/")
        || file_name.contains(".test.")
        || file_name.contains(".spec.")
    {
        state.behavior.test_file_events = state.behavior.test_file_events.saturating_add(1);
    }
    if lower.contains("/.github/workflows/")
        || lower.contains("/infra/")
        || lower.contains("/terraform/")
        || lower.contains("/migrations/")
        || matches!(
            file_name,
            "dockerfile"
                | "docker-compose.yml"
                | "docker-compose.yaml"
                | "package.json"
                | "cargo.toml"
                | "tauri.conf.json"
        )
    {
        state.behavior.infrastructure_events =
            state.behavior.infrastructure_events.saturating_add(1);
    }
    if lower.contains("/scripts/")
        || lower.ends_with(".sh")
        || lower.ends_with(".zsh")
        || lower.ends_with(".command")
        || lower.contains("/.github/workflows/")
    {
        state.behavior.automation_events = state.behavior.automation_events.saturating_add(1);
    }
}

fn ignored_path(path: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|pattern| {
        let pattern = pattern.trim_start_matches('/').trim_end_matches('/');
        if pattern.is_empty() || pattern.starts_with('!') {
            return false;
        }
        if let Some(suffix) = pattern.strip_prefix("*.") {
            return path.ends_with(&format!(".{suffix}"));
        }
        path == pattern
            || path.starts_with(&format!("{pattern}/"))
            || path.split('/').any(|component| component == pattern)
    })
}

pub fn inspect_patch(state: &mut ParseState, patch: &str) {
    let evidence = patch_evidence(patch);
    state
        .touched_file_hashes
        .extend(evidence.file_hashes.iter().cloned());
    state.lines_added = state.lines_added.saturating_add(evidence.lines_added);
    state.lines_deleted = state.lines_deleted.saturating_add(evidence.lines_deleted);
    let timestamp = state.last_timestamp.clone();
    for file in evidence.files {
        record_file_change(
            state,
            &file.path,
            &file.change_kind,
            file.lines_added,
            file.lines_deleted,
            timestamp.as_deref(),
        );
    }
}

pub fn inspect_codex_requested_patch(state: &mut ParseState, patch: &str) {
    let evidence = patch_evidence(patch);
    state
        .codex_requested_file_hashes
        .extend(evidence.file_hashes.iter().cloned());
    state.codex_requested_lines_added = state
        .codex_requested_lines_added
        .saturating_add(evidence.lines_added);
    state.codex_requested_lines_deleted = state
        .codex_requested_lines_deleted
        .saturating_add(evidence.lines_deleted);
    if state.codex_patch_result_events == 0 {
        state.touched_file_hashes = state.codex_requested_file_hashes.clone();
        state.lines_added = state.codex_requested_lines_added;
        state.lines_deleted = state.codex_requested_lines_deleted;
    }
    let timestamp = state.last_timestamp.clone();
    for file in evidence.files {
        record_file_change(
            state,
            &file.path,
            &file.change_kind,
            file.lines_added,
            file.lines_deleted,
            timestamp.as_deref(),
        );
    }
}

pub fn record_codex_patch_result(state: &mut ParseState, payload: &Value, timestamp: Option<&str>) {
    if state.codex_patch_result_events == 0 {
        state.touched_file_hashes.clear();
        state.file_changes.clear();
        state.lines_added = 0;
        state.lines_deleted = 0;
    }
    state.codex_patch_result_events = state.codex_patch_result_events.saturating_add(1);
    if payload.get("success").and_then(Value::as_bool) != Some(true) {
        record_error(state, timestamp);
        return;
    }
    let Some(changes) = payload.get("changes").and_then(Value::as_object) else {
        return;
    };
    for (path, change) in changes {
        state.touched_file_hashes.insert(stable_hash(path));
        if let Some(move_path) = change.get("move_path").and_then(Value::as_str) {
            state.touched_file_hashes.insert(stable_hash(move_path));
        }
        match change.get("type").and_then(Value::as_str) {
            Some("update") => {
                if let Some(diff) = change.get("unified_diff").and_then(Value::as_str) {
                    let (added, deleted) = diff_line_counts(diff);
                    state.lines_added = state.lines_added.saturating_add(added);
                    state.lines_deleted = state.lines_deleted.saturating_add(deleted);
                    record_file_change(state, path, "modified", added, deleted, timestamp);
                }
            }
            Some("add") => {
                if let Some(content) = change.get("content").and_then(Value::as_str) {
                    let added = content.lines().count() as u64;
                    state.lines_added = state.lines_added.saturating_add(added);
                    record_file_change(state, path, "added", added, 0, timestamp);
                }
            }
            Some("delete") => {
                if let Some(content) = change.get("content").and_then(Value::as_str) {
                    let deleted = content.lines().count() as u64;
                    state.lines_deleted = state.lines_deleted.saturating_add(deleted);
                    record_file_change(state, path, "deleted", 0, deleted, timestamp);
                }
            }
            _ => {}
        }
    }
}

#[derive(Default)]
struct PatchEvidence {
    file_hashes: HashSet<String>,
    files: Vec<PatchFileEvidence>,
    lines_added: u64,
    lines_deleted: u64,
}

struct PatchFileEvidence {
    path: String,
    change_kind: String,
    lines_added: u64,
    lines_deleted: u64,
}

fn patch_evidence(patch: &str) -> PatchEvidence {
    let mut evidence = PatchEvidence::default();
    let mut current: Option<PatchFileEvidence> = None;
    for line in patch.lines() {
        let header = line
            .strip_prefix("*** Update File: ")
            .map(|path| (path, "modified"))
            .or_else(|| {
                line.strip_prefix("*** Add File: ")
                    .map(|path| (path, "added"))
            })
            .or_else(|| {
                line.strip_prefix("*** Delete File: ")
                    .map(|path| (path, "deleted"))
            });
        if let Some((path, change_kind)) = header {
            if let Some(previous) = current.take() {
                evidence.files.push(previous);
            }
            evidence.file_hashes.insert(stable_hash(path));
            current = Some(PatchFileEvidence {
                path: path.to_string(),
                change_kind: change_kind.into(),
                lines_added: 0,
                lines_deleted: 0,
            });
            continue;
        }
        if line.starts_with("+++") || line.starts_with("---") {
            continue;
        }
        if line.starts_with('+') {
            evidence.lines_added = evidence.lines_added.saturating_add(1);
            if let Some(file) = current.as_mut() {
                file.lines_added = file.lines_added.saturating_add(1);
            }
        } else if line.starts_with('-') {
            evidence.lines_deleted = evidence.lines_deleted.saturating_add(1);
            if let Some(file) = current.as_mut() {
                file.lines_deleted = file.lines_deleted.saturating_add(1);
            }
        }
    }
    if let Some(previous) = current {
        evidence.files.push(previous);
    }
    evidence
}

fn diff_line_counts(diff: &str) -> (u64, u64) {
    let mut added = 0_u64;
    let mut deleted = 0_u64;
    for line in diff.lines() {
        if line.starts_with("+++") || line.starts_with("---") {
            continue;
        }
        if line.starts_with('+') {
            added = added.saturating_add(1);
        } else if line.starts_with('-') {
            deleted = deleted.saturating_add(1);
        }
    }
    (added, deleted)
}

/// Extracts `apply_patch` calls nested inside the newer Codex `exec` wrapper.
///
/// Codex records the JavaScript orchestration source, not the nested tool payloads,
/// so the adapter resolves only literal patch variables that are present in that
/// observed source. Dynamic values remain unavailable rather than being guessed.
pub fn inspect_codex_exec_patches(state: &mut ParseState, source: &str) -> usize {
    let calls = tool_call_arguments(source, "apply_patch");
    let mut observed = 0;
    for (call_start, argument_start) in calls {
        if let Some(patch) = resolve_literal_argument(source, call_start, argument_start) {
            inspect_codex_requested_patch(state, &patch);
            observed += 1;
        }
    }
    observed
}

/// Records literal `cmd`/`command` payloads nested in Codex orchestration source.
///
/// The wrapper may contain several `tools.exec_command` calls. Each observed
/// command is classified independently so a real test/build/check becomes
/// verification evidence without retaining the command text itself.
pub fn record_codex_exec_commands(
    state: &mut ParseState,
    source: &str,
    timestamp: Option<&str>,
    parent_source_event_id: Option<&str>,
) -> usize {
    let mut observed = 0;
    let calls = tool_call_arguments(source, "exec_command");
    if calls.len() > 1 || source.contains("Promise.all") || source.contains("Promise.allSettled") {
        state.behavior.parallel_batches = state.behavior.parallel_batches.saturating_add(1);
    }
    for (index, (call_start, argument_start)) in calls.into_iter().enumerate() {
        let Some(command) =
            object_string_property(source, call_start, argument_start, &["cmd", "command"])
        else {
            continue;
        };
        let input = serde_json::json!({ "cmd": command });
        let source_event_id = parent_source_event_id
            .map(|parent| stable_hash(&format!("{parent}|exec-command|{index}")));
        record_tool_with_source(
            state,
            "exec",
            Some(&input),
            timestamp,
            source_event_id.as_deref(),
        );
        observed += 1;
    }
    observed
}

fn object_string_property(
    source: &str,
    call_start: usize,
    argument_start: usize,
    names: &[&str],
) -> Option<String> {
    let bytes = source.as_bytes();
    let mut index = skip_whitespace(bytes, argument_start);
    if bytes.get(index) != Some(&b'{') {
        return None;
    }
    index += 1;
    let mut depth = 1_u32;
    while index < bytes.len() && depth > 0 {
        match bytes[index] {
            b'\'' | b'"' | b'`' => {
                let (key, end) = parse_js_string(source, index)?;
                let next = skip_whitespace(bytes, end);
                if names.contains(&key.as_str()) && bytes.get(next) == Some(&b':') {
                    return resolve_literal_argument(source, call_start, next + 1);
                }
                index = end;
            }
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index = bytes[index + 2..]
                    .iter()
                    .position(|byte| *byte == b'\n')
                    .map_or(bytes.len(), |offset| index + 3 + offset);
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index = source[index + 2..]
                    .find("*/")
                    .map_or(bytes.len(), |offset| index + 4 + offset);
            }
            b'{' => {
                depth += 1;
                index += 1;
            }
            b'}' => {
                depth = depth.saturating_sub(1);
                index += 1;
            }
            byte if byte.is_ascii_alphabetic() || matches!(byte, b'_' | b'$') => {
                let start = index;
                while bytes.get(index).is_some_and(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(*byte, b'_' | b'$')
                }) {
                    index += 1;
                }
                let key = &source[start..index];
                let next = skip_whitespace(bytes, index);
                if names.contains(&key) && bytes.get(next) == Some(&b':') {
                    return resolve_literal_argument(source, call_start, next + 1);
                }
            }
            _ => index += 1,
        }
    }
    None
}

fn tool_call_arguments(source: &str, target: &str) -> Vec<(usize, usize)> {
    let bytes = source.as_bytes();
    let mut calls = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\'' | b'"' | b'`' => {
                index = string_end(bytes, index).unwrap_or(bytes.len().saturating_sub(1)) + 1;
            }
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index = bytes[index + 2..]
                    .iter()
                    .position(|byte| *byte == b'\n')
                    .map_or(bytes.len(), |offset| index + 3 + offset);
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index = source[index + 2..]
                    .find("*/")
                    .map_or(bytes.len(), |offset| index + 4 + offset);
            }
            _ if bytes[index..].starts_with(b"tools.") => {
                let method_start = index + "tools.".len();
                let mut method_end = method_start;
                while bytes
                    .get(method_end)
                    .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
                {
                    method_end += 1;
                }
                let mut open = method_end;
                while bytes.get(open).is_some_and(u8::is_ascii_whitespace) {
                    open += 1;
                }
                if &source[method_start..method_end] == target && bytes.get(open) == Some(&b'(') {
                    calls.push((index, open + 1));
                }
                index = open.saturating_add(1);
            }
            _ => index += 1,
        }
    }
    calls
}

fn resolve_literal_argument(
    source: &str,
    call_start: usize,
    argument_start: usize,
) -> Option<String> {
    let bytes = source.as_bytes();
    let mut index = skip_whitespace(bytes, argument_start);
    match bytes.get(index).copied()? {
        b'\'' | b'"' | b'`' => parse_js_string(source, index).map(|(value, _)| value),
        byte if byte.is_ascii_alphabetic() || matches!(byte, b'_' | b'$') => {
            let start = index;
            while bytes
                .get(index)
                .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'_' | b'$'))
            {
                index += 1;
            }
            literal_assignment_before(source, &source[start..index], call_start)
        }
        _ => None,
    }
}

fn literal_assignment_before(source: &str, name: &str, before: usize) -> Option<String> {
    let bytes = source.as_bytes();
    let mut index = 0;
    let mut latest = None;
    while index < before.min(bytes.len()) {
        match bytes[index] {
            b'\'' | b'"' | b'`' => {
                index = string_end(bytes, index).unwrap_or(bytes.len().saturating_sub(1)) + 1;
            }
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index = bytes[index + 2..]
                    .iter()
                    .position(|byte| *byte == b'\n')
                    .map_or(bytes.len(), |offset| index + 3 + offset);
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index = source[index + 2..]
                    .find("*/")
                    .map_or(bytes.len(), |offset| index + 4 + offset);
            }
            byte if byte.is_ascii_alphabetic() || matches!(byte, b'_' | b'$') => {
                let token_start = index;
                while bytes.get(index).is_some_and(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(*byte, b'_' | b'$')
                }) {
                    index += 1;
                }
                if !matches!(&source[token_start..index], "const" | "let" | "var") {
                    continue;
                }
                index = skip_whitespace(bytes, index);
                let variable_start = index;
                while bytes.get(index).is_some_and(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(*byte, b'_' | b'$')
                }) {
                    index += 1;
                }
                if &source[variable_start..index] != name {
                    continue;
                }
                index = skip_whitespace(bytes, index);
                if bytes.get(index) != Some(&b'=') {
                    continue;
                }
                index = skip_whitespace(bytes, index + 1);
                if let Some((value, end)) = parse_js_string(source, index) {
                    if end <= before {
                        latest = Some(value);
                    }
                    index = end;
                }
            }
            _ => index += 1,
        }
    }
    latest
}

fn skip_whitespace(bytes: &[u8], mut index: usize) -> usize {
    while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
        index += 1;
    }
    index
}

fn string_end(bytes: &[u8], start: usize) -> Option<usize> {
    let quote = *bytes.get(start)?;
    let mut index = start + 1;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            index = index.saturating_add(2);
        } else if bytes[index] == quote {
            return Some(index);
        } else {
            index += 1;
        }
    }
    None
}

fn parse_js_string(source: &str, start: usize) -> Option<(String, usize)> {
    let bytes = source.as_bytes();
    let quote = *bytes.get(start)?;
    if !matches!(quote, b'\'' | b'"' | b'`') {
        return None;
    }
    let end = string_end(bytes, start)?;
    let literal = &source[start..=end];
    if quote == b'"'
        && let Ok(value) = serde_json::from_str::<String>(literal)
    {
        return Some((value, end + 1));
    }
    let mut value = String::new();
    let mut characters = source[start + 1..end].chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            value.push(character);
            continue;
        }
        let Some(escaped) = characters.next() else {
            break;
        };
        value.push(match escaped {
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            other => other,
        });
    }
    Some((value, end + 1))
}

pub fn parsed_object_from_string(value: &str) -> Value {
    serde_json::from_str(value).unwrap_or_else(|_| Value::String(value.to_string()))
}

pub fn increment_retry(state: &mut ParseState) {
    state.retries = state.retries.saturating_add(1);
}

fn local_date(timestamp: Option<&str>) -> Option<String> {
    timestamp
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| {
            value
                .with_timezone(&Local)
                .date_naive()
                .format("%Y-%m-%d")
                .to_string()
        })
}

pub fn normalized_tool_id(value: &Value) -> Option<String> {
    normalized_source_record_id(value)
}

pub fn normalized_source_record_id(value: &Value) -> Option<String> {
    source_record_native_id(value).map(stable_hash)
}

fn source_record_native_id(value: &Value) -> Option<&str> {
    value
        .get("id")
        .or_else(|| value.get("call_id"))
        .or_else(|| value.get("event_id"))
        .or_else(|| value.get("eventId"))
        .or_else(|| value.get("request_id"))
        .or_else(|| value.get("requestId"))
        .or_else(|| value.get("tool_use_id"))
        .or_else(|| value.get("toolUseId"))
        .and_then(Value::as_str)
}

pub fn source_record_receipt(value: &Value, key: &[u8; 32]) -> String {
    let record_kind = value
        .get("type")
        .or_else(|| value.get("event"))
        .and_then(Value::as_str)
        .unwrap_or("record");
    if let Some(source_id) = source_record_native_id(value) {
        let mut material = Vec::with_capacity(record_kind.len() + source_id.len() + 24);
        material.extend_from_slice(b"vibemeter-native-record\0");
        material.extend_from_slice(&(record_kind.len() as u64).to_be_bytes());
        material.extend_from_slice(record_kind.as_bytes());
        material.extend_from_slice(&(source_id.len() as u64).to_be_bytes());
        material.extend_from_slice(source_id.as_bytes());
        return format!("native:{}", hmac_sha256(key, &material));
    }
    let material = serde_json::to_vec(value).unwrap_or_default();
    format!("keyed:{}", hmac_sha256(key, &material))
}

fn hmac_sha256(key: &[u8; 32], material: &[u8]) -> String {
    const BLOCK_BYTES: usize = 64;
    let mut inner_pad = [0x36_u8; BLOCK_BYTES];
    let mut outer_pad = [0x5c_u8; BLOCK_BYTES];
    for (index, byte) in key.iter().enumerate() {
        inner_pad[index] ^= byte;
        outer_pad[index] ^= byte;
    }
    let mut inner = Sha256::new();
    inner.update(inner_pad);
    inner.update(material);
    let inner_digest = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(outer_pad);
    outer.update(inner_digest);
    hex::encode(outer.finalize())
}

pub fn source_record_once(state: &mut ParseState, value: &Value) -> (bool, Option<String>) {
    let identity = source_record_receipt(value, &state.source_record_receipt_key);
    let source_id = normalized_source_record_id(value);
    if !state.source_record_ids.insert(identity.clone()) {
        return (false, source_id);
    }
    state.new_source_record_ids.insert(identity);
    (true, source_id)
}

pub fn derived_source_event_id(parent: &str, kind: &str, index: usize) -> String {
    stable_hash(&format!("{parent}|{kind}|{index}"))
}

pub fn set_source_session(state: &mut ParseState, value: Option<&str>) {
    if let Some(value) = value.filter(|value| !value.is_empty()) {
        state.source_session_id = value.to_string();
    }
}

pub fn mark_unknown(state: &mut ParseState) {
    state.unknown_records = state.unknown_records.saturating_add(1);
}
