use crate::live::project_label_from_cwd;
use crate::models::{LiveAction, LiveSession};
use crate::privacy::stable_hash;
use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration as StdDuration, SystemTime};

const KIMI_LIVE_WINDOW: StdDuration = StdDuration::from_secs(90);
const DEEPSEEK_LIVE_WINDOW: StdDuration = StdDuration::from_secs(10 * 60);
const DEEPSEEK_COMPLETED_WINDOW: StdDuration = StdDuration::from_secs(90);
const ZCODE_LIVE_WINDOW: StdDuration = StdDuration::from_secs(90);
const ZCODE_MODEL_IO_WINDOW: StdDuration = StdDuration::from_secs(20);
const MAX_DISCOVERY_FILES: usize = 64;
const MAX_SNAPSHOT_BYTES: u64 = 2 * 1024 * 1024;
const MAX_JSONL_TAIL_BYTES: u64 = 768 * 1024;
const MAX_JSONL_LINE_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug)]
struct RuntimeSignal {
    status: &'static str,
    phase: &'static str,
    action_kind: &'static str,
    action_label: String,
    occurred_at: String,
    started_at: Option<String>,
}

pub(crate) fn discover() -> Vec<LiveSession> {
    let now = Utc::now();
    let mut sessions = HashMap::new();
    if let Some(home) = dirs::home_dir() {
        for session in discover_kimi(&home.join(".kimi-code"), now) {
            sessions.insert(session.id.clone(), session);
        }
        for session in discover_deepseek_harness(&home.join(".dsh"), now) {
            sessions.insert(session.id.clone(), session);
        }
        for session in discover_zcode(&zcode_root(&home), now) {
            sessions
                .entry(session.id.clone())
                .and_modify(|existing| {
                    if existing.updated_at < session.updated_at
                        || existing.project_label == "Unknown project"
                    {
                        *existing = session.clone();
                    }
                })
                .or_insert(session);
        }
    }
    sessions.into_values().collect()
}

pub(crate) fn provider_available(provider: &str) -> bool {
    let Some(home) = dirs::home_dir() else {
        return false;
    };
    match provider {
        "kimi-code" => home.join(".kimi-code").is_dir(),
        "deepseek-harness" => home.join(".dsh/sessions").is_dir(),
        "zcode" => {
            let root = zcode_root(&home);
            root.is_dir() || Path::new("/Applications/ZCode.app").is_dir()
        }
        _ => false,
    }
}

fn discover_deepseek_harness(root: &Path, now: DateTime<Utc>) -> Vec<LiveSession> {
    recent_files(&root.join("sessions"), 5, |path| {
        path.file_name().and_then(|value| value.to_str()) == Some("session.jsonl.zstd")
    })
    .into_iter()
    .filter_map(|path| deepseek_harness_session(&path, now))
    .collect()
}

fn deepseek_harness_session(path: &Path, now: DateTime<Utc>) -> Option<LiveSession> {
    let modified = modified_at(path)?;
    if !is_recent(modified, DEEPSEEK_LIVE_WINDOW) {
        return None;
    }
    let mut source_session_id = None;
    let mut project_label = "Unknown project".to_string();
    let mut started_at = None;
    let mut latest = None;
    let mut turn_open = false;
    crate::adapters::deepseek_harness::read_records(path, |record| {
        if record.get("version").is_some() && record.get("id").is_some() {
            source_session_id = record
                .get("id")
                .and_then(Value::as_str)
                .filter(|value| valid_identifier(value))
                .map(ToString::to_string);
            project_label = project_label_from_cwd(
                record
                    .get("cwd")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            );
            started_at = timestamp_from_value(record.get("createdAt"));
            return;
        }
        let event_type = record
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let data = record.get("data").unwrap_or(&Value::Null);
        let occurred_at = timestamp_from_value(record.get("time"))
            .unwrap_or_else(|| system_time_timestamp(modified));
        match event_type {
            "turn/start" => {
                turn_open = true;
                latest = Some(runtime_signal(
                    "running",
                    "thinking",
                    "think",
                    "Thinking",
                    occurred_at,
                    started_at.clone(),
                ));
            }
            "step/start" | "approval/decided" | "tool/result" => {
                if turn_open {
                    latest = Some(runtime_signal(
                        "running",
                        "thinking",
                        "think",
                        "Thinking",
                        occurred_at,
                        started_at.clone(),
                    ));
                }
            }
            "tool/call" => {
                turn_open = true;
                let label = data
                    .get("name")
                    .and_then(Value::as_str)
                    .and_then(safe_label)
                    .unwrap_or_else(|| "Tool".into());
                latest = Some(runtime_signal(
                    "running",
                    runtime_phase(&label),
                    "tool",
                    label,
                    occurred_at,
                    started_at.clone(),
                ));
            }
            "approval/asked" => {
                turn_open = true;
                latest = Some(runtime_signal(
                    "waiting",
                    "needs-you",
                    "waiting",
                    "Permission",
                    occurred_at,
                    started_at.clone(),
                ));
            }
            "compaction/start" => {
                turn_open = true;
                latest = Some(runtime_signal(
                    "running",
                    "compacting",
                    "compact",
                    "Compacting context",
                    occurred_at,
                    started_at.clone(),
                ));
            }
            "turn/end" => {
                turn_open = false;
                let reason = data
                    .get("reason")
                    .and_then(|reason| reason.get("kind"))
                    .and_then(Value::as_str);
                let (status, phase, kind, label) = match reason {
                    Some("error") | Some("blocked") => ("error", "error", "error", "Error"),
                    _ => ("completed", "completed", "complete", "Completed"),
                };
                latest = Some(runtime_signal(
                    status,
                    phase,
                    kind,
                    label,
                    occurred_at,
                    started_at.clone(),
                ));
            }
            _ => {}
        }
    })
    .ok()?;
    let source_session_id = source_session_id?;
    let signal = latest?;
    if signal.status == "completed"
        && !is_recent_timestamp(&signal.occurred_at, now, DEEPSEEK_COMPLETED_WINDOW)
    {
        return None;
    }
    Some(runtime_session(
        "deepseek-harness",
        source_session_id,
        project_label,
        signal,
        process_for("deepseek-harness").map(|(pid, _)| (pid, "web")),
    ))
}

fn zcode_root(home: &Path) -> PathBuf {
    std::env::var_os("ZCODE_DATA_BASE_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.to_path_buf())
        .join(".zcode")
}

fn discover_kimi(root: &Path, now: DateTime<Utc>) -> Vec<LiveSession> {
    let sessions_root = root.join("sessions");
    let Ok(workspaces) = fs::read_dir(&sessions_root) else {
        return Vec::new();
    };
    let mut result = Vec::new();
    for workspace in workspaces.flatten() {
        if !workspace.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let Ok(session_entries) = fs::read_dir(workspace.path()) else {
            continue;
        };
        for entry in session_entries.flatten() {
            if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            let session_root = entry.path();
            let Some(session_id) = entry.file_name().to_str().and_then(|name| {
                name.strip_prefix("session_")
                    .filter(|value| valid_identifier(value))
                    .map(str::to_string)
            }) else {
                continue;
            };
            let wire = session_root.join("agents/main/wire.jsonl");
            let Some(modified) = modified_at(&wire) else {
                continue;
            };
            if !is_recent(modified, KIMI_LIVE_WINDOW) {
                continue;
            }
            let Some(signal) = parse_kimi_wire(&wire, now) else {
                continue;
            };
            let state = read_json_object(&session_root.join("state.json"));
            let work_dir = state
                .as_ref()
                .and_then(|value| value.get("workDir"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            let process = process_for("kimi");
            result.push(runtime_session(
                "kimi-code",
                session_id,
                project_label_from_cwd(work_dir),
                signal,
                process.map(|(pid, _)| (pid, "cli")),
            ));
        }
    }
    result
}

fn parse_kimi_wire(path: &Path, now: DateTime<Utc>) -> Option<RuntimeSignal> {
    let lines = tail_lines(path, MAX_JSONL_TAIL_BYTES)?;
    let mut open = false;
    let mut latest: Option<RuntimeSignal> = None;
    let mut started_at = None;
    for line in lines {
        let Ok(record) = serde_json::from_slice::<Value>(&line) else {
            continue;
        };
        let occurred_at = record_timestamp(&record).unwrap_or_else(|| now.to_rfc3339());
        match record
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "turn.prompt" => {
                open = true;
                started_at = Some(occurred_at.clone());
                latest = Some(RuntimeSignal {
                    status: "running",
                    phase: "thinking",
                    action_kind: "prompt",
                    action_label: "Prompt".into(),
                    occurred_at,
                    started_at: started_at.clone(),
                });
            }
            "turn.cancel" => {
                open = false;
                latest = None;
            }
            "permission.request" | "permission.required" | "permission.prompt" => {
                open = true;
                latest = Some(RuntimeSignal {
                    status: "waiting",
                    phase: "needs-you",
                    action_kind: "waiting",
                    action_label: "Permission".into(),
                    occurred_at,
                    started_at: started_at.clone(),
                });
            }
            "permission.record_approval_result" => {
                open = true;
                latest = Some(runtime_signal(
                    "running",
                    "thinking",
                    "session",
                    "Kimi Code",
                    occurred_at,
                    started_at.clone(),
                ));
            }
            "llm.request" => {
                open = true;
                latest = Some(runtime_signal(
                    "running",
                    "thinking",
                    "think",
                    "Thinking",
                    occurred_at,
                    started_at.clone(),
                ));
            }
            "full_compaction.begin" => {
                open = true;
                latest = Some(runtime_signal(
                    "running",
                    "compacting",
                    "compact",
                    "Compacting context",
                    occurred_at,
                    started_at.clone(),
                ));
            }
            "context.apply_compaction" => {
                open = true;
                latest = Some(runtime_signal(
                    "running",
                    "compacting",
                    "compact",
                    "Applying compaction",
                    occurred_at,
                    started_at.clone(),
                ));
            }
            "context.append_loop_event" => {
                let event = record.get("event").and_then(Value::as_object);
                let event_type = event
                    .and_then(|event| event.get("type"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                match event_type {
                    "step.begin" => {
                        open = true;
                        latest = Some(runtime_signal(
                            "running",
                            "thinking",
                            "think",
                            "Thinking",
                            occurred_at,
                            started_at.clone(),
                        ));
                    }
                    "step.end" => {
                        open = false;
                        latest = None;
                    }
                    "tool.call" => {
                        open = true;
                        let label = event
                            .and_then(|event| event.get("name"))
                            .and_then(Value::as_str)
                            .and_then(safe_label)
                            .unwrap_or_else(|| "Tool".into());
                        latest = Some(runtime_signal(
                            "running",
                            runtime_phase(&label),
                            "tool",
                            label,
                            occurred_at,
                            started_at.clone(),
                        ));
                    }
                    "tool.result" | "content.part" => {
                        open = true;
                        latest = Some(runtime_signal(
                            "running",
                            "running-tool",
                            "tool",
                            "Running tool",
                            occurred_at,
                            started_at.clone(),
                        ));
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    let signal = latest?;
    open.then_some(signal)
}

fn discover_zcode(root: &Path, now: DateTime<Utc>) -> Vec<LiveSession> {
    let mut result = Vec::new();
    let snapshots_root = root.join("v2/sessions");
    for path in recent_files(&snapshots_root, 5, |path| {
        path.extension().and_then(|value| value.to_str()) == Some("json")
    }) {
        let Some(snapshot) = read_json(&path, MAX_SNAPSHOT_BYTES) else {
            continue;
        };
        let Some(session) = zcode_snapshot_session(&snapshot, &path, now) else {
            continue;
        };
        result.push(session);
    }
    let index = root.join("v2/tasks-index.sqlite");
    result.extend(discover_zcode_tasks(&index, now));
    for directory in [root.join("cli/debug"), root.join("cli/rollout")] {
        for path in recent_files(&directory, 3, |path| {
            path.file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.starts_with("model-io-") && name.ends_with(".jsonl"))
        }) {
            let Some(session) = zcode_model_io_session(&path, now) else {
                continue;
            };
            result.push(session);
        }
    }
    result
}

fn zcode_snapshot_session(
    snapshot: &Value,
    path: &Path,
    now: DateTime<Utc>,
) -> Option<LiveSession> {
    let meta = snapshot.get("meta").and_then(Value::as_object)?;
    let source_id = meta
        .get("taskId")
        .or_else(|| meta.get("sessionId"))
        .and_then(Value::as_str)
        .filter(|value| valid_identifier(value))?;
    let status = runtime_status(meta.get("status").and_then(Value::as_str)?)?;
    let updated_at = timestamp_from_value(meta.get("updatedAt"))
        .or_else(|| modified_at(path).map(system_time_timestamp))?;
    if !is_recent_timestamp(&updated_at, now, ZCODE_LIVE_WINDOW) {
        return None;
    }
    let work_dir = meta
        .get("workspacePath")
        .or_else(|| meta.get("cwd"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let (phase, action_kind, action_label) = zcode_snapshot_action(snapshot, status);
    let started_at =
        timestamp_from_value(meta.get("createdAt")).or_else(|| Some(updated_at.clone()));
    Some(runtime_session(
        "zcode",
        source_id.to_string(),
        project_label_from_cwd(work_dir),
        runtime_signal(
            status,
            phase,
            action_kind,
            action_label,
            updated_at,
            started_at,
        ),
        process_for("zcode").map(|(pid, _)| (pid, "desktop")),
    ))
}

fn zcode_snapshot_action(
    snapshot: &Value,
    status: &'static str,
) -> (&'static str, &'static str, String) {
    if status == "waiting" {
        return ("needs-you", "waiting", "Permission".into());
    }
    if status == "error" {
        return ("error", "error", "Error".into());
    }
    let event = snapshot
        .get("events")
        .and_then(Value::as_array)
        .and_then(|events| events.last())
        .or_else(|| {
            snapshot
                .get("messages")
                .and_then(Value::as_array)
                .and_then(|messages| messages.last())
        });
    let tool = event
        .and_then(|value| value.get("toolName").or_else(|| value.get("name")))
        .and_then(Value::as_str)
        .and_then(safe_label);
    let tool = tool.or_else(|| {
        event
            .and_then(|value| value.get("tools"))
            .and_then(Value::as_array)
            .and_then(|tools| tools.last())
            .and_then(|tool| {
                tool.get("toolName")
                    .or_else(|| tool.get("name"))
                    .and_then(Value::as_str)
            })
            .and_then(safe_label)
    });
    if let Some(tool) = tool {
        return (runtime_phase(&tool), "tool", tool);
    }
    ("thinking", "think", "Thinking".into())
}

fn discover_zcode_tasks(path: &Path, now: DateTime<Utc>) -> Vec<LiveSession> {
    if !path.is_file() {
        return Vec::new();
    }
    let Ok(connection) = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) else {
        return Vec::new();
    };
    let mut statement = match connection.prepare(
        "SELECT workspace_path, task_id, task_status, created_at, updated_at
         FROM tasks WHERE deleted=0 AND archived=0",
    ) {
        Ok(statement) => statement,
        Err(_) => return Vec::new(),
    };
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, i64>(4)?,
        ))
    });
    let Ok(rows) = rows else {
        return Vec::new();
    };
    let mut result = Vec::new();
    for row in rows.flatten() {
        let (workspace_path, task_id, raw_status, created_at, updated_at) = row;
        let Some(status) = raw_status.as_deref().and_then(runtime_status) else {
            continue;
        };
        let Some(updated_at) = timestamp_from_number(updated_at) else {
            continue;
        };
        if !is_recent_timestamp(&updated_at, now, ZCODE_LIVE_WINDOW) {
            continue;
        }
        let started_at = timestamp_from_number(created_at).unwrap_or_else(|| updated_at.clone());
        result.push(runtime_session(
            "zcode",
            task_id,
            project_label_from_cwd(&workspace_path),
            runtime_signal(
                status,
                if status == "waiting" {
                    "needs-you"
                } else {
                    "thinking"
                },
                if status == "waiting" {
                    "waiting"
                } else {
                    "think"
                },
                if status == "waiting" {
                    "Permission"
                } else {
                    "Thinking"
                },
                updated_at,
                Some(started_at),
            ),
            process_for("zcode").map(|(pid, _)| (pid, "desktop")),
        ));
    }
    result
}

fn zcode_model_io_session(path: &Path, now: DateTime<Utc>) -> Option<LiveSession> {
    let modified = modified_at(path)?;
    if !is_recent(modified, ZCODE_MODEL_IO_WINDOW) {
        return None;
    }
    let mut latest = None;
    for line in tail_lines(path, MAX_JSONL_TAIL_BYTES)? {
        let Ok(record) = serde_json::from_slice::<Value>(&line) else {
            continue;
        };
        if record.get("type").and_then(Value::as_str) == Some("model_io") {
            latest = Some(record);
        }
    }
    let record = latest?;
    let source_id = record
        .get("sessionId")
        .and_then(Value::as_str)
        .filter(|value| valid_identifier(value))?;
    let status = if record.get("error").is_some() {
        "error"
    } else if record.get("response").is_some() {
        return None;
    } else {
        "running"
    };
    let updated_at = record_timestamp(&record)
        .or_else(|| modified_at(path).map(system_time_timestamp))
        .unwrap_or_else(|| now.to_rfc3339());
    let work_dir = record
        .get("workspacePath")
        .or_else(|| record.get("cwd"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    Some(runtime_session(
        "zcode",
        source_id.to_string(),
        project_label_from_cwd(work_dir),
        runtime_signal(
            status,
            if status == "error" {
                "error"
            } else {
                "thinking"
            },
            if status == "error" { "error" } else { "think" },
            if status == "error" {
                "Error"
            } else {
                "Thinking"
            },
            updated_at.clone(),
            Some(updated_at),
        ),
        process_for("zcode").map(|(pid, _)| (pid, "desktop")),
    ))
}

fn runtime_session(
    agent: &str,
    source_session_id: String,
    project_label: String,
    signal: RuntimeSignal,
    process: Option<(u32, &str)>,
) -> LiveSession {
    let id = stable_hash(&format!("{agent}:{source_session_id}"));
    let occurred_at = signal.occurred_at;
    let started_at = signal.started_at.unwrap_or_else(|| occurred_at.clone());
    let event_order_key = stable_hash(&format!(
        "{}|{}|{}|{}|{}",
        occurred_at, signal.status, signal.phase, signal.action_kind, signal.action_label
    ));
    LiveSession {
        id,
        source_session_id,
        agent: agent.into(),
        project_label,
        conversation_title: None,
        status: signal.status.into(),
        phase: signal.phase.into(),
        started_at,
        updated_at: occurred_at.clone(),
        activity_ended_at: (!matches!(signal.status, "idle" | "running"))
            .then(|| occurred_at.clone()),
        event_order_key,
        waiting_reason: (signal.status == "waiting").then(|| "Permission required".into()),
        actions: vec![LiveAction {
            kind: signal.action_kind.into(),
            label: signal.action_label,
            occurred_at,
        }],
        process_id: process.map(|(pid, _)| pid),
        origin: process.map(|(_, origin)| origin.into()),
        pulse: Default::default(),
        jump_context: None,
    }
}

fn runtime_signal(
    status: &'static str,
    phase: &'static str,
    action_kind: &'static str,
    action_label: impl Into<String>,
    occurred_at: String,
    started_at: Option<String>,
) -> RuntimeSignal {
    RuntimeSignal {
        status,
        phase,
        action_kind,
        action_label: action_label.into(),
        occurred_at,
        started_at,
    }
}

fn runtime_status(value: &str) -> Option<&'static str> {
    match value.to_ascii_lowercase().as_str() {
        "running" | "active" | "working" | "processing" | "thinking" | "generating"
        | "executing" | "in_progress" | "in-progress" => Some("running"),
        "waiting"
        | "needs_approval"
        | "needs-approval"
        | "awaiting_approval"
        | "awaiting-approval"
        | "permission"
        | "permission_required" => Some("waiting"),
        "error" | "failed" | "failure" => Some("error"),
        _ => None,
    }
}

fn runtime_phase(label: &str) -> &'static str {
    let normalized = label.to_ascii_lowercase();
    if normalized.contains("plan") {
        "planning"
    } else if normalized.contains("read")
        || normalized.contains("search")
        || normalized.contains("grep")
    {
        "reading"
    } else if normalized.contains("write")
        || normalized.contains("edit")
        || normalized.contains("patch")
    {
        "editing"
    } else if normalized.contains("test")
        || normalized.contains("lint")
        || normalized.contains("check")
    {
        "verifying"
    } else {
        "running-tool"
    }
}

fn safe_label(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()
        && value.len() <= 80
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "_-. ".contains(character)))
    .then(|| value.to_string())
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 160
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_:.".contains(character))
}

fn read_json_object(path: &Path) -> Option<Value> {
    let value = read_json(path, 128 * 1024)?;
    value.is_object().then_some(value)
}

fn read_json(path: &Path, max_bytes: u64) -> Option<Value> {
    let metadata = fs::metadata(path).ok()?;
    if metadata.len() > max_bytes {
        return None;
    }
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

fn tail_lines(path: &Path, max_bytes: u64) -> Option<Vec<Vec<u8>>> {
    let metadata = fs::metadata(path).ok()?;
    let offset = metadata.len().saturating_sub(max_bytes);
    let mut file = fs::File::open(path).ok()?;
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut bytes = Vec::new();
    file.take(max_bytes).read_to_end(&mut bytes).ok()?;
    let mut lines = bytes.split(|byte| *byte == b'\n');
    if offset > 0 {
        let _ = lines.next();
    }
    Some(
        lines
            .filter(|line| !line.is_empty() && line.len() <= MAX_JSONL_LINE_BYTES)
            .map(ToOwned::to_owned)
            .collect(),
    )
}

fn recent_files<F>(root: &Path, max_depth: usize, predicate: F) -> Vec<PathBuf>
where
    F: Fn(&Path) -> bool + Copy,
{
    let mut stack = vec![(root.to_path_buf(), 0_usize)];
    let mut files = Vec::new();
    while let Some((directory, depth)) = stack.pop() {
        if depth > max_depth {
            continue;
        }
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            let path = entry.path();
            if kind.is_dir() {
                stack.push((path, depth + 1));
            } else if kind.is_file()
                && predicate(&path)
                && let Some(modified) = modified_at(&path)
            {
                files.push((modified, path));
            }
        }
    }
    files.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    files
        .into_iter()
        .take(MAX_DISCOVERY_FILES)
        .map(|(_, path)| path)
        .collect()
}

fn modified_at(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).ok()?.modified().ok()
}

fn is_recent(modified: SystemTime, window: StdDuration) -> bool {
    SystemTime::now()
        .duration_since(modified)
        .is_ok_and(|age| age <= window)
}

fn is_recent_timestamp(value: &str, now: DateTime<Utc>, window: StdDuration) -> bool {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|timestamp| {
            let age = now.signed_duration_since(timestamp.with_timezone(&Utc));
            age >= chrono::Duration::zero()
                && age
                    <= chrono::Duration::from_std(window).unwrap_or(chrono::Duration::seconds(90))
        })
        .unwrap_or(false)
}

fn record_timestamp(record: &Value) -> Option<String> {
    [
        "timestamp",
        "time",
        "startedAt",
        "createdAt",
        "updatedAt",
        "completedAt",
    ]
    .iter()
    .find_map(|key| timestamp_from_value(record.get(*key)))
}

fn timestamp_from_value(value: Option<&Value>) -> Option<String> {
    let value = value?;
    if let Some(value) = value.as_str().filter(|value| !value.is_empty()) {
        return DateTime::parse_from_rfc3339(value)
            .ok()
            .map(|_| value.to_string());
    }
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
        .and_then(timestamp_from_number)
}

fn timestamp_from_number(value: i64) -> Option<String> {
    if value >= 10_000_000_000 {
        DateTime::<Utc>::from_timestamp_millis(value)
    } else {
        DateTime::<Utc>::from_timestamp(value, 0)
    }
    .map(|value| value.to_rfc3339_opts(SecondsFormat::Millis, true))
}

fn system_time_timestamp(value: SystemTime) -> String {
    DateTime::<Utc>::from(value).to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn process_for(provider: &str) -> Option<(u32, bool)> {
    let output = Command::new("/bin/ps")
        .args(["-axo", "pid=,tty=,command="])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let mut candidates = output
        .stdout
        .split(|byte| *byte == b'\n')
        .filter_map(|line| {
            let line = String::from_utf8_lossy(line);
            let mut fields = line.split_whitespace();
            let pid = fields.next()?.parse::<u32>().ok()?;
            let tty = fields.next()?;
            let command = fields.collect::<Vec<_>>().join(" ");
            let lower = command.to_ascii_lowercase();
            let matches = match provider {
                "kimi" => {
                    lower.ends_with("/kimi") || lower == "kimi" || lower.contains("kimi-code")
                }
                "zcode" => lower == "zcode" || lower.contains("zcode-host-local"),
                "deepseek-harness" => {
                    lower.ends_with("/dsh") || lower.contains("/dsh web") || lower == "dsh"
                }
                _ => false,
            };
            matches.then_some((pid, tty != "??"))
        });
    candidates.next()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn kimi_wire_requires_an_open_turn_and_does_not_leak_prompt_text() {
        let directory = tempdir().expect("tempdir");
        let wire = directory.path().join("wire.jsonl");
        fs::write(
            &wire,
            format!(
                "{}\n{}\n",
                serde_json::json!({"type":"turn.prompt","timestamp":Utc::now().to_rfc3339(),"prompt":"private"}),
                serde_json::json!({"type":"context.append_loop_event","timestamp":Utc::now().to_rfc3339(),"event":{"type":"step.begin"}})
            ),
        )
        .expect("wire");
        let signal = parse_kimi_wire(&wire, Utc::now()).expect("active signal");
        assert_eq!(signal.status, "running");
        assert_eq!(signal.action_label, "Thinking");
        assert!(!format!("{signal:?}").contains("private"));

        fs::OpenOptions::new()
            .append(true)
            .open(&wire)
            .expect("open wire")
            .write_all(
                format!(
                    "{}\n",
                    serde_json::json!({"type":"context.append_loop_event","timestamp":Utc::now().to_rfc3339(),"event":{"type":"step.end"}})
                )
                .as_bytes(),
            )
            .expect("append");
        assert!(parse_kimi_wire(&wire, Utc::now()).is_none());
    }

    #[test]
    fn zcode_snapshot_reports_only_recent_active_statuses() {
        let directory = tempdir().expect("tempdir");
        let path = directory.path().join("task.json");
        let now = Utc::now();
        let snapshot = serde_json::json!({
            "meta": {
                "taskId": "zcode-task",
                "workspacePath": "/tmp/zcode-project",
                "createdAt": (now - chrono::Duration::seconds(4)).to_rfc3339(),
                "updatedAt": now.to_rfc3339(),
                "status": "running"
            },
            "messages": [{"role":"assistant","content":"private","tools":[{"toolName":"apply_patch"}]}]
        });
        fs::write(&path, snapshot.to_string()).expect("snapshot");
        let session = zcode_snapshot_session(&snapshot, &path, now).expect("active snapshot");
        assert_eq!(session.agent, "zcode");
        assert_eq!(session.status, "running");
        assert_eq!(session.actions[0].label, "apply_patch");
        assert!(!format!("{session:?}").contains("private"));
    }

    #[test]
    fn deepseek_harness_log_reports_exact_waiting_state_without_payload_text() {
        let directory = tempdir().expect("tempdir");
        let path = directory.path().join("session.jsonl.zstd");
        let now = Utc::now();
        let lines = [
            serde_json::json!({
                "version": 0,
                "id": "session-deepseek-live",
                "createdAt": (now - chrono::Duration::seconds(3)).timestamp_millis(),
                "cwd": "/tmp/deepseek-project"
            }),
            serde_json::json!({
                "type": "turn/start",
                "seq": 1,
                "time": (now - chrono::Duration::seconds(2)).timestamp_millis(),
                "data": {"turn": 1}
            }),
            serde_json::json!({
                "type": "user/message",
                "seq": 2,
                "time": (now - chrono::Duration::seconds(1)).timestamp_millis(),
                "data": {"content": [{"type":"text","text":"private prompt"}], "source":{"kind":"user"}}
            }),
            serde_json::json!({
                "type": "approval/asked",
                "seq": 3,
                "time": now.timestamp_millis(),
                "data": {"id":"approval-1","toolName":"bash","reason":"private command"}
            }),
        ]
        .into_iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n")
            + "\n";
        fs::write(
            &path,
            zstd::stream::encode_all(lines.as_bytes(), 1).expect("zstd"),
        )
        .expect("session log");

        let session = deepseek_harness_session(&path, now).expect("live session");
        assert_eq!(session.agent, "deepseek-harness");
        assert_eq!(session.status, "waiting");
        assert_eq!(session.phase, "needs-you");
        assert_eq!(session.project_label, "deepseek-project");
        assert!(!format!("{session:?}").contains("private"));
    }
}
