use crate::adapters::{claude, codex, common, cursor, kimi, openclaw, zcode};
use crate::database::Database;
use crate::errors::AppResult;
use crate::git_evidence;
use crate::models::{AgentKind, IndexStatus, PARSER_VERSION, ParseState, TokenUsage};
use crate::privacy::stable_hash;
use chrono::Utc;
use chrono::{DateTime, SecondsFormat};
use rusqlite::Connection;
use serde_json::Value;
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use tauri::{AppHandle, Emitter};
use walkdir::WalkDir;

const MAX_RECORD_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug)]
struct SourceFile {
    path: PathBuf,
    agent: AgentKind,
    adapter: SourceAdapter,
    size: u64,
    modified: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceAdapter {
    Claude,
    Codex,
    Cursor,
    Kimi,
    OpenClaw,
    ZCode,
}

#[derive(Debug)]
struct SourceRoot {
    path: PathBuf,
    agent: AgentKind,
    adapter: SourceAdapter,
}

pub fn start_indexing(
    database: Database,
    status: Arc<RwLock<IndexStatus>>,
    app: AppHandle,
    force: bool,
) -> bool {
    if status.read().map(|current| current.running).unwrap_or(true) {
        return false;
    }
    if let Ok(mut current) = status.write() {
        *current = IndexStatus {
            phase: "discovering".into(),
            running: true,
            started_at: Some(Utc::now().to_rfc3339()),
            message_key: "index.discovering".into(),
            ..IndexStatus::default()
        };
    }
    let status_for_thread = status.clone();
    std::thread::spawn(move || {
        let outcome = run_index(&database, &status_for_thread, &app, force);
        if let Ok(mut current) = status_for_thread.write() {
            current.running = false;
            current.finished_at = Some(Utc::now().to_rfc3339());
            match outcome {
                Ok(()) => {
                    current.phase = "complete".into();
                    current.message_key = "index.complete".into();
                }
                Err(_) => {
                    current.phase = "partial".into();
                    current.message_key = "index.partial".into();
                    current.warning_count = current.warning_count.saturating_add(1);
                }
            }
            let _ = app.emit("index-progress", current.clone());
        }
    });
    true
}

fn run_index(
    database: &Database,
    status: &Arc<RwLock<IndexStatus>>,
    app: &AppHandle,
    force: bool,
) -> AppResult<()> {
    let roots = source_roots();
    let mut files = Vec::new();
    for root in &roots {
        let path_hash = stable_hash(&root.path.to_string_lossy());
        let available = root.path.is_dir();
        database.upsert_source(
            root.agent,
            &path_hash,
            available,
            if available { "indexing" } else { "not-found" },
        )?;
        if available {
            collect_jsonl_files(&root.path, root.agent, root.adapter, &mut files);
        }
    }
    let zcode_model_io_ids = files
        .iter()
        .filter(|file| file.adapter == SourceAdapter::ZCode)
        .filter_map(|file| {
            if file.path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
                return None;
            }
            file.path
                .file_stem()?
                .to_str()?
                .strip_prefix("model-io-")
                .map(ToString::to_string)
        })
        .collect::<HashSet<_>>();
    files.retain(|file| {
        if file.adapter != SourceAdapter::ZCode
            || file.path.extension().and_then(|value| value.to_str()) != Some("json")
        {
            return true;
        }
        file.path
            .file_stem()
            .and_then(|value| value.to_str())
            .is_none_or(|stem| !zcode_model_io_ids.contains(stem))
    });
    index_cursor_database(database, force)?;
    index_hermes_database(database, force)?;
    files.sort_by_key(|item| std::cmp::Reverse(item.modified));
    if let Ok(mut current) = status.write() {
        current.phase = "indexing".into();
        current.discovered_files = files.len() as u64;
        current.message_key = "index.indexing".into();
        let _ = app.emit("index-progress", current.clone());
    }

    for file in &files {
        match parse_source_file(database, file, force) {
            Ok(indexed) => {
                if let Ok(mut current) = status.write() {
                    current.processed_files = current.processed_files.saturating_add(1);
                    if indexed {
                        current.indexed_sessions = current.indexed_sessions.saturating_add(1);
                    }
                    if current.processed_files % 4 == 0
                        || current.processed_files == current.discovered_files
                    {
                        let _ = app.emit("index-progress", current.clone());
                    }
                }
            }
            Err(_) => {
                if let Ok(mut current) = status.write() {
                    current.processed_files = current.processed_files.saturating_add(1);
                    current.warning_count = current.warning_count.saturating_add(1);
                    let _ = app.emit("index-progress", current.clone());
                }
            }
        }
    }

    for agent in [
        AgentKind::ClaudeCode,
        AgentKind::Codex,
        AgentKind::KimiCode,
        AgentKind::Cursor,
        AgentKind::OpenClaw,
        AgentKind::Hermes,
        AgentKind::ZCode,
    ] {
        let agent_roots = roots
            .iter()
            .filter(|root| root.agent == agent)
            .map(|root| root.path.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        let available = agent_roots.iter().any(|root| Path::new(root).is_dir())
            || (agent == AgentKind::Cursor && cursor_database_path().is_file())
            || (agent == AgentKind::Hermes && hermes_database_path().is_file());
        let path_hash = stable_hash(&agent_roots.join("|"));
        database.upsert_source(
            agent,
            &path_hash,
            available,
            if available { "ready" } else { "not-found" },
        )?;
    }
    let total_sessions = database
        .sources()?
        .into_iter()
        .map(|source| source.session_count)
        .sum();
    if let Ok(mut current) = status.write() {
        current.indexed_sessions = total_sessions;
    }
    database.prune_evidence()?;
    Ok(())
}

fn parse_source_file(database: &Database, source: &SourceFile, force: bool) -> AppResult<bool> {
    let file_hash = stable_hash(&source.path.to_string_lossy());
    let cursor = database.load_cursor(&file_hash)?;
    if !force
        && cursor.as_ref().is_some_and(|cursor| {
            cursor.source_size == source.size
                && cursor.source_mtime == source.modified
                && cursor.state.agent == source.agent
                && cursor.state.parser_version == PARSER_VERSION
        })
    {
        return Ok(false);
    }

    let fallback_id = if source.adapter == SourceAdapter::Kimi {
        file_hash.clone()
    } else {
        source
            .path
            .file_stem()
            .and_then(|value| value.to_str())
            .map(ToString::to_string)
            .unwrap_or_else(|| file_hash.clone())
    };
    let can_resume = source.adapter != SourceAdapter::ZCode
        && !force
        && cursor.as_ref().is_some_and(|cursor| {
            cursor.source_size < source.size
                && cursor.byte_offset <= source.size
                && cursor.state.agent == source.agent
                && cursor.state.parser_version == PARSER_VERSION
        });
    let mut state = if can_resume {
        cursor
            .as_ref()
            .map(|cursor| cursor.state.clone())
            .unwrap_or_else(|| ParseState::new(source.agent, fallback_id.clone()))
    } else {
        ParseState::new(source.agent, fallback_id)
    };
    state.source_record_receipt_key = database.source_record_receipt_key();
    if !can_resume {
        state.replace_source_record_ids = true;
    }
    if can_resume
        && state.project_root.is_none()
        && matches!(
            source.adapter,
            SourceAdapter::Kimi | SourceAdapter::OpenClaw
        )
    {
        restore_transient_project_context(
            &source.path,
            cursor.as_ref().map_or(0, |cursor| cursor.byte_offset),
            &mut state,
        )?;
    }
    if source.adapter == SourceAdapter::Cursor {
        // Cursor's transcript stream has no dependable message timestamps. Its
        // filesystem modification time is an honest session-level anchor, which
        // makes the recovered activity visible in date-ranged dashboards.
        let anchor = timestamp_from_seconds(source.modified as f64);
        common::observe_timestamp(&mut state, anchor.as_deref(), true);
    }
    let prompt_structure_enabled = database
        .setting("vctiPromptStructure")?
        .is_none_or(|value| value == "true");
    common::set_prompt_structure_enabled(&mut state, prompt_structure_enabled);

    if source.adapter == SourceAdapter::ZCode {
        let mut content = Vec::new();
        File::open(&source.path)?.read_to_end(&mut content)?;
        if content.len() > MAX_RECORD_BYTES {
            state.malformed_records = state.malformed_records.saturating_add(1);
        } else if let Ok(record) = serde_json::from_slice::<Value>(&content) {
            zcode::parse_record(&mut state, &record);
        } else {
            for line in content.split(|byte| *byte == b'\n') {
                if line.is_empty() {
                    continue;
                }
                match serde_json::from_slice::<Value>(line) {
                    Ok(record) => zcode::parse_record(&mut state, &record),
                    Err(_) => state.malformed_records = state.malformed_records.saturating_add(1),
                }
            }
        }
        common::finalize_run(&mut state);
        let git_allowed = database
            .setting("gitReadAllowed")?
            .is_some_and(|value| value == "true");
        state.git_evidence = Some(if git_allowed {
            state.project_root.as_deref().map_or_else(
                || crate::models::GitEvidence {
                    available: false,
                    state: "project-unavailable".into(),
                    ..crate::models::GitEvidence::default()
                },
                |root| {
                    git_evidence::inspect(
                        root,
                        state.started_at.as_deref(),
                        state.ended_at.as_deref(),
                    )
                },
            )
        } else {
            crate::models::GitEvidence {
                available: false,
                state: "not-authorized".into(),
                ..crate::models::GitEvidence::default()
            }
        });
        database.persist_parse_state(
            &file_hash,
            source.size,
            source.modified,
            source.size,
            &state,
        )?;
        return Ok(true);
    }
    let start_offset = if can_resume {
        cursor.as_ref().map_or(0, |cursor| cursor.byte_offset)
    } else {
        0
    };

    let file = File::open(&source.path)?;
    let mut reader = BufReader::with_capacity(256 * 1024, file);
    reader.seek(SeekFrom::Start(start_offset))?;
    let mut safe_offset = start_offset;
    let mut buffer = Vec::with_capacity(64 * 1024);
    loop {
        buffer.clear();
        let record_start = reader.stream_position()?;
        let bytes = reader.read_until(b'\n', &mut buffer)?;
        if bytes == 0 {
            break;
        }
        if buffer.last().copied() != Some(b'\n') {
            safe_offset = record_start;
            break;
        }
        safe_offset = reader.stream_position()?;
        if buffer.len() > MAX_RECORD_BYTES {
            state.malformed_records = state.malformed_records.saturating_add(1);
            continue;
        }
        match serde_json::from_slice::<Value>(&buffer) {
            Ok(record) => {
                if can_resume
                    && matches!(
                        source.adapter,
                        SourceAdapter::Kimi | SourceAdapter::OpenClaw | SourceAdapter::ZCode
                    )
                    && database.history_record_id_exists(
                        &file_hash,
                        &common::source_record_receipt(&record, &state.source_record_receipt_key),
                    )?
                {
                    continue;
                }
                match source.adapter {
                    SourceAdapter::Codex => codex::parse_record(&mut state, &record),
                    SourceAdapter::Claude => claude::parse_record(&mut state, &record),
                    SourceAdapter::Cursor => cursor::parse_record(&mut state, &record),
                    SourceAdapter::Kimi => kimi::parse_record(&mut state, &record),
                    SourceAdapter::OpenClaw => openclaw::parse_record(&mut state, &record),
                    SourceAdapter::ZCode => zcode::parse_record(&mut state, &record),
                }
            }
            Err(_) => {
                state.malformed_records = state.malformed_records.saturating_add(1);
            }
        }
    }
    common::finalize_run(&mut state);
    let git_allowed = database
        .setting("gitReadAllowed")?
        .is_some_and(|value| value == "true");
    state.git_evidence = Some(if git_allowed {
        state.project_root.as_deref().map_or_else(
            || crate::models::GitEvidence {
                available: false,
                state: "project-unavailable".into(),
                ..crate::models::GitEvidence::default()
            },
            |root| {
                git_evidence::inspect(root, state.started_at.as_deref(), state.ended_at.as_deref())
            },
        )
    } else {
        crate::models::GitEvidence {
            available: false,
            state: "not-authorized".into(),
            ..crate::models::GitEvidence::default()
        }
    });
    database.persist_parse_state(
        &file_hash,
        source.size,
        source.modified,
        safe_offset,
        &state,
    )?;
    Ok(true)
}

fn restore_transient_project_context(
    path: &Path,
    byte_offset: u64,
    state: &mut ParseState,
) -> AppResult<()> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut buffer = Vec::with_capacity(4096);
    while reader.stream_position()? < byte_offset && state.project_root.is_none() {
        buffer.clear();
        let record_start = reader.stream_position()?;
        let bytes = reader.read_until(b'\n', &mut buffer)?;
        if bytes == 0 || reader.stream_position()? > byte_offset {
            break;
        }
        if buffer.len() > MAX_RECORD_BYTES {
            continue;
        }
        let Ok(record) = serde_json::from_slice::<Value>(&buffer) else {
            continue;
        };
        let context = record.get("meta").unwrap_or(&record);
        common::set_project(
            state,
            context
                .get("workspacePath")
                .or_else(|| context.get("cwd"))
                .or_else(|| context.get("workdir"))
                .and_then(Value::as_str),
        );
        if reader.stream_position()? <= record_start {
            break;
        }
    }
    Ok(())
}

fn collect_jsonl_files(
    root: &Path,
    agent: AgentKind,
    adapter: SourceAdapter,
    destination: &mut Vec<SourceFile>,
) {
    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let extension = entry.path().extension().and_then(|value| value.to_str());
        let is_zcode_json = adapter == SourceAdapter::ZCode && extension == Some("json");
        let is_jsonl = extension == Some("jsonl");
        if !is_jsonl && !is_zcode_json {
            continue;
        }
        let file_name = entry.file_name().to_string_lossy();
        if adapter == SourceAdapter::ZCode
            && ((is_zcode_json && file_name.ends_with(".deleted.json"))
                || (is_jsonl && !file_name.starts_with("model-io-")))
        {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let modified = metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|value| value.as_secs() as i64)
            .unwrap_or(0);
        destination.push(SourceFile {
            path: entry.path().to_path_buf(),
            agent,
            adapter,
            size: metadata.len(),
            modified,
        });
    }
}

fn source_roots() -> Vec<SourceRoot> {
    let mut roots = Vec::new();
    let Some(home) = dirs::home_dir() else {
        return roots;
    };

    let codex_home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".codex"));
    roots.push(SourceRoot {
        agent: AgentKind::Codex,
        path: codex_home.join("sessions"),
        adapter: SourceAdapter::Codex,
    });
    roots.push(SourceRoot {
        agent: AgentKind::OpenClaw,
        path: home.join(".openclaw/agents"),
        adapter: SourceAdapter::OpenClaw,
    });
    roots.push(SourceRoot {
        agent: AgentKind::OpenClaw,
        path: home.join(".openclaw/sessions"),
        adapter: SourceAdapter::OpenClaw,
    });
    roots.push(SourceRoot {
        agent: AgentKind::Codex,
        path: codex_home.join("archived_sessions"),
        adapter: SourceAdapter::Codex,
    });

    if let Some(configured) = std::env::var_os("CLAUDE_CONFIG_DIR") {
        for value in configured.to_string_lossy().split(',') {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                roots.push(SourceRoot {
                    agent: AgentKind::ClaudeCode,
                    path: PathBuf::from(trimmed).join("projects"),
                    adapter: SourceAdapter::Claude,
                });
            }
        }
    }
    roots.push(SourceRoot {
        agent: AgentKind::ClaudeCode,
        path: home.join(".config/claude/projects"),
        adapter: SourceAdapter::Claude,
    });
    roots.push(SourceRoot {
        agent: AgentKind::ClaudeCode,
        path: home.join(".claude/projects"),
        adapter: SourceAdapter::Claude,
    });
    roots.push(SourceRoot {
        agent: AgentKind::KimiCode,
        path: home.join(".kimi-code/sessions"),
        adapter: SourceAdapter::Kimi,
    });
    roots.push(SourceRoot {
        agent: AgentKind::Cursor,
        path: home.join(".cursor/projects"),
        adapter: SourceAdapter::Cursor,
    });
    let zcode_base = std::env::var_os("ZCODE_DATA_BASE_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.clone());
    let zcode_root = zcode_base.join(".zcode");
    roots.push(SourceRoot {
        agent: AgentKind::ZCode,
        path: zcode_root.join("v2/sessions"),
        adapter: SourceAdapter::ZCode,
    });
    for path in [zcode_root.join("cli/debug"), zcode_root.join("cli/rollout")] {
        roots.push(SourceRoot {
            agent: AgentKind::ZCode,
            path,
            adapter: SourceAdapter::ZCode,
        });
    }

    let claude_support = home.join("Library/Application Support/Claude");
    for parent in [
        claude_support.join("local-agent-mode-sessions"),
        claude_support.join("claude-code-sessions"),
    ] {
        if !parent.is_dir() {
            continue;
        }
        for entry in WalkDir::new(parent)
            .max_depth(6)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
        {
            if entry.file_type().is_dir() && entry.path().ends_with(Path::new(".claude/projects")) {
                roots.push(SourceRoot {
                    agent: AgentKind::ClaudeCode,
                    path: entry.path().to_path_buf(),
                    adapter: SourceAdapter::Claude,
                });
            }
        }
    }
    roots.sort_by(|left, right| left.path.cmp(&right.path));
    roots.dedup_by(|left, right| {
        left.agent == right.agent && left.path == right.path && left.adapter == right.adapter
    });
    roots
}

fn cursor_database_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".cursor/ai-tracking/ai-code-tracking.db")
}

fn hermes_database_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".hermes/state.db")
}

fn index_cursor_database(database: &Database, force: bool) -> AppResult<()> {
    let path = cursor_database_path();
    let available = path.is_file();
    database.upsert_source(
        AgentKind::Cursor,
        &stable_hash(&path.to_string_lossy()),
        available,
        if available { "indexing" } else { "not-found" },
    )?;
    if !available {
        return Ok(());
    }
    let connection =
        Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut statement = connection
        .prepare("SELECT conversationId, title, model, updatedAt FROM conversation_summaries")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let title: Option<String> = row.get(1)?;
        let model: Option<String> = row.get(2)?;
        let updated: i64 = row.get(3)?;
        let key = stable_hash(&format!("cursor:{id}"));
        if !force
            && database
                .load_cursor(&key)?
                .is_some_and(|cursor| cursor.state.parser_version == PARSER_VERSION)
        {
            continue;
        }
        let stamp = timestamp_from_millis(updated);
        let mut state = ParseState::new(AgentKind::Cursor, id);
        state.source_session_observed = true;
        common::observe_timestamp(&mut state, stamp.as_deref(), true);
        common::consider_title(&mut state, title.as_deref());
        common::set_model(&mut state, model.as_deref());
        common::finalize_run(&mut state);
        database.persist_parse_state(
            &key,
            updated.max(0) as u64,
            updated,
            updated.max(0) as u64,
            &state,
        )?;
    }
    Ok(())
}

fn index_hermes_database(database: &Database, force: bool) -> AppResult<()> {
    let path = hermes_database_path();
    let available = path.is_file();
    database.upsert_source(
        AgentKind::Hermes,
        &stable_hash(&path.to_string_lossy()),
        available,
        if available { "indexing" } else { "not-found" },
    )?;
    if !available {
        return Ok(());
    }
    let connection =
        Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut statement = connection.prepare("SELECT id, model, started_at, ended_at, message_count, tool_call_count, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens, estimated_cost_usd, title FROM sessions")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let key = stable_hash(&format!("hermes:{id}"));
        if !force
            && database
                .load_cursor(&key)?
                .is_some_and(|cursor| cursor.state.parser_version == PARSER_VERSION)
        {
            continue;
        }
        let started: f64 = row.get(2)?;
        let ended: Option<f64> = row.get(3)?;
        let stamp = timestamp_from_seconds(started);
        let mut state = ParseState::new(AgentKind::Hermes, id);
        state.source_session_observed = true;
        common::observe_timestamp(&mut state, stamp.as_deref(), true);
        if let Some(end) = ended {
            common::observe_timestamp(&mut state, timestamp_from_seconds(end).as_deref(), false);
        }
        let model: Option<String> = row.get(1)?;
        common::set_model(&mut state, model.as_deref());
        let title: Option<String> = row.get(12)?;
        common::consider_title(&mut state, title.as_deref());
        let usage = TokenUsage {
            input_tokens: row.get::<_, i64>(6)?.max(0) as u64,
            output_tokens: row.get::<_, i64>(7)?.max(0) as u64,
            cache_read_tokens: row.get::<_, i64>(8)?.max(0) as u64,
            cache_write_tokens: row.get::<_, i64>(9)?.max(0) as u64,
            cache_write_1h_tokens: 0,
            reasoning_tokens: row.get::<_, i64>(10)?.max(0) as u64,
        };
        common::record_usage(&mut state, &usage, stamp.as_deref(), model.as_deref());
        state.tool_calls = row.get::<_, i64>(5)?.max(0) as u64;
        state.human_interventions = row.get::<_, i64>(4)?.max(0) as u64;
        state.estimated_cost_usd = row.get::<_, Option<f64>>(11)?.unwrap_or(0.0).max(0.0);
        common::finalize_run(&mut state);
        database.persist_parse_state(&key, 1, started as i64, 1, &state)?;
    }
    Ok(())
}

fn timestamp_from_millis(value: i64) -> Option<String> {
    DateTime::<Utc>::from_timestamp_millis(value)
        .map(|time| time.to_rfc3339_opts(SecondsFormat::Millis, true))
}
fn timestamp_from_seconds(value: f64) -> Option<String> {
    DateTime::<Utc>::from_timestamp(
        value as i64,
        ((value.fract().max(0.0)) * 1_000_000_000.0) as u32,
    )
    .map(|time| time.to_rfc3339_opts(SecondsFormat::Millis, true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{IndexStatus, SessionListFilters, ShareRenderRequest};
    use serde_json::json;
    use std::io::Write;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    fn text_history_records(adapter: SourceAdapter, project_root: &str) -> Vec<Value> {
        let private_path = format!("{project_root}/src/lib.rs");
        match adapter {
            SourceAdapter::Kimi => {
                let prompt = json!({
                    "id":"prompt-1", "type":"turn.prompt",
                    "time":1_775_000_000_000_i64,
                    "sessionId":"kimi-text-session", "cwd":project_root,
                    "prompt":"Implement text history coverage"
                });
                let usage = json!({
                    "type":"usage.record",
                    "time":1_775_000_001_000_i64,
                    "model":"kimi-code/k3",
                    "usage":{"inputOther":60,"output":40,"inputCacheRead":0,"inputCacheCreation":0},
                    "sourceSpecificSecret":"sk-text-source-secret"
                });
                let edit = json!({
                    "id":"edit-record", "type":"context.append_loop_event",
                    "time":1_775_000_002_000_i64,
                    "event":{"id":"edit-1","type":"tool.call","name":"Edit",
                        "args":{"file_path":private_path,"old_string":"secret old","new_string":"secret new"}}
                });
                let verify = json!({
                    "id":"verify-record", "type":"context.append_loop_event",
                    "time":1_775_000_003_000_i64,
                    "event":{"id":"verify-1","type":"tool.call","name":"Bash",
                        "args":{"command":"cargo test --secret"}}
                });
                vec![
                    prompt.clone(),
                    prompt,
                    usage.clone(),
                    usage,
                    edit.clone(),
                    edit,
                    verify.clone(),
                    verify,
                    json!({"id":"missing-1","type":"metadata"}),
                ]
            }
            SourceAdapter::OpenClaw => {
                let message = json!({
                    "type":"message", "timestamp":"2026-04-01T00:00:00Z",
                    "sessionId":"openclaw-text-session", "cwd":project_root,
                    "model":"gpt-text", "role":"user",
                    "message":{"role":"user","content":"Implement text history coverage"},
                    "usage":{"input_tokens":60,"output_tokens":40},
                    "sourceSpecificSecret":"sk-text-source-secret"
                });
                let edit = json!({
                    "id":"edit-1", "type":"tool.call", "timestamp":"2026-04-01T00:00:01Z",
                    "sessionId":"openclaw-text-session", "name":"Edit",
                    "arguments":{"file_path":private_path,"old_string":"secret old","new_string":"secret new"}
                });
                let verify = json!({
                    "id":"verify-1", "type":"tool.call", "timestamp":"2026-04-01T00:00:02Z",
                    "sessionId":"openclaw-text-session", "name":"Bash",
                    "arguments":{"command":"cargo test --secret"}
                });
                vec![
                    message.clone(),
                    message,
                    edit.clone(),
                    edit,
                    verify.clone(),
                    verify,
                    json!({"id":"missing-1","type":"metadata"}),
                ]
            }
            SourceAdapter::ZCode => {
                let model_io = json!({
                    "type":"model_io",
                    "sessionId":"zcode-text-session", "startedAt":"2026-04-01T00:00:00Z",
                    "workspacePath":project_root, "model":{"modelId":"glm-text"},
                    "request":{"messages":[{"role":"user","content":"Implement text history coverage"}]},
                    "response":{
                        "text":"Implemented",
                        "usage":{"inputTokens":60,"outputTokens":40},
                        "toolCalls":[
                            {"id":"edit-1","name":"Edit","input":{"file_path":private_path,"old_string":"secret old","new_string":"secret new"}},
                            {"id":"verify-1","name":"Bash","input":{"command":"cargo test --secret"}}
                        ]
                    },
                    "sourceSpecificSecret":"sk-text-source-secret"
                });
                let completed = json!({
                    "id":"turn-1", "type":"turn.completed", "timestamp":"2026-04-01T00:00:03Z",
                    "payload":{"duration":3000}
                });
                vec![
                    model_io.clone(),
                    model_io,
                    completed.clone(),
                    completed,
                    json!({"id":"missing-1","type":"unknown"}),
                ]
            }
            _ => unreachable!("fixture is limited to text history adapters"),
        }
    }

    fn appended_text_history_records(adapter: SourceAdapter, project_root: &str) -> Vec<Value> {
        let appended_path = format!("{project_root}/src/append.rs");
        let ignored_path = format!("{project_root}/src/ignored.rs");
        match adapter {
            SourceAdapter::Kimi => [
                ("append-record", "append-tool", appended_path),
                ("ignored-record", "ignored-tool", ignored_path),
            ]
            .into_iter()
            .enumerate()
            .map(|(index, (record_id, tool_id, path))| {
                json!({
                    "id":record_id, "type":"context.append_loop_event",
                    "time":1_775_000_004_000_i64 + index as i64,
                    "event":{"id":tool_id,"type":"tool.call","name":"Edit",
                        "args":{"file_path":path,"new_string":"safe"}}
                })
            })
            .collect(),
            SourceAdapter::OpenClaw => [
                ("append-tool", appended_path),
                ("ignored-tool", ignored_path),
            ]
            .into_iter()
            .enumerate()
            .map(|(index, (record_id, path))| {
                json!({
                    "id":record_id, "type":"tool.call",
                    "timestamp":format!("2026-04-01T00:00:0{}Z", index + 4),
                    "sessionId":"openclaw-text-session", "name":"Edit",
                    "arguments":{"file_path":path,"new_string":"safe"}
                })
            })
            .collect(),
            SourceAdapter::ZCode => vec![json!({
                "id":"appended-model-io", "type":"model_io",
                "sessionId":"zcode-text-session", "startedAt":"2026-04-01T00:00:04Z",
                "model":{"modelId":"glm-text"},
                "response":{"toolCalls":[
                    {"id":"append-tool","name":"Edit","input":{"file_path":appended_path,"new_string":"safe"}},
                    {"id":"ignored-tool","name":"Edit","input":{"file_path":ignored_path,"new_string":"safe"}}
                ]}
            })],
            _ => unreachable!("fixture is limited to text history adapters"),
        }
    }

    fn untimed_usage_record(adapter: SourceAdapter) -> Value {
        match adapter {
            SourceAdapter::Kimi => json!({
                "type":"usage.record", "model":"kimi-code/k3",
                "usage":{"inputOther":60,"output":40}
            }),
            SourceAdapter::OpenClaw => json!({
                "type":"message", "model":"gpt-text", "role":"assistant",
                "usage":{"input_tokens":60,"output_tokens":40}
            }),
            SourceAdapter::ZCode => json!({
                "type":"model_io", "sessionId":"zcode-untimed", "model":{"modelId":"glm-text"},
                "response":{"usage":{"inputTokens":60,"outputTokens":40}}
            }),
            _ => unreachable!("fixture is limited to text history adapters"),
        }
    }

    #[test]
    fn text_history_sources_handle_normal_missing_corrupt_and_duplicate_records() {
        for (agent, adapter, expected_model) in [
            (AgentKind::KimiCode, SourceAdapter::Kimi, "kimi-code/k3"),
            (AgentKind::OpenClaw, SourceAdapter::OpenClaw, "gpt-text"),
            (AgentKind::ZCode, SourceAdapter::ZCode, "glm-text"),
        ] {
            let temporary = tempfile::tempdir().expect("temporary directory");
            let database = Database::open(
                temporary
                    .path()
                    .join(format!("{}-text.sqlite", agent.as_str())),
            )
            .expect("database should open");
            let project_root = temporary.path().join("private-project");
            std::fs::create_dir_all(project_root.join("src"))
                .expect("project fixture should exist");
            std::fs::write(project_root.join(".gitignore"), "src/ignored.rs\n")
                .expect("ignore fixture should write");
            let project_root = project_root.to_string_lossy().to_string();
            let source_path = temporary
                .path()
                .join(format!("{}-history.jsonl", agent.as_str()));
            let mut source_file = File::create(&source_path).expect("fixture should create");
            for record in text_history_records(adapter, &project_root) {
                writeln!(source_file, "{record}").expect("record should write");
            }
            writeln!(source_file, "{{not-json").expect("corrupt record should write");
            source_file.flush().expect("fixture should flush");
            let metadata = source_file
                .metadata()
                .expect("fixture metadata should load");
            let mut source = SourceFile {
                path: source_path,
                agent,
                adapter,
                size: metadata.len(),
                modified: 1,
            };

            assert!(parse_source_file(&database, &source, false).expect("source should parse"));
            let cursor = database
                .load_cursor(&stable_hash(&source.path.to_string_lossy()))
                .expect("cursor should load")
                .expect("cursor should exist");
            assert_eq!(cursor.state.malformed_records, 1);
            assert_eq!(cursor.state.tool_calls, 2);
            assert_eq!(cursor.state.usage.total(), 100);
            assert_eq!(
                cursor.state.primary_model().as_deref(),
                Some(expected_model)
            );
            assert_eq!(
                cursor
                    .state
                    .events
                    .iter()
                    .filter(|event| event.event_type == "tool")
                    .count(),
                2,
                "duplicate tool records must not create duplicate evidence"
            );

            let sessions = database
                .sessions(
                    "all",
                    SessionListFilters {
                        agent: Some(agent.as_str()),
                        ..SessionListFilters::default()
                    },
                    0,
                    10,
                )
                .expect("sessions should load");
            assert_eq!(sessions.total, 1);
            let detail = database
                .session_detail(&sessions.items[0].id)
                .expect("session detail should load");
            assert_eq!(detail.file_changes.len(), 1);
            assert_eq!(detail.file_changes[0].path, "src/lib.rs");
            assert_eq!(detail.summary.verification_state, "verified");
            let initial_offset = cursor.byte_offset;

            let mut source_file = std::fs::OpenOptions::new()
                .append(true)
                .open(&source.path)
                .expect("fixture should reopen for append");
            for record in appended_text_history_records(adapter, &project_root) {
                writeln!(source_file, "{record}").expect("appended record should write");
            }
            source_file.flush().expect("appended fixture should flush");
            let metadata = source_file
                .metadata()
                .expect("appended metadata should load");
            source.size = metadata.len();
            source.modified = 2;
            assert!(parse_source_file(&database, &source, false).expect("append should parse"));
            let appended_cursor = database
                .load_cursor(&stable_hash(&source.path.to_string_lossy()))
                .expect("appended cursor should load")
                .expect("appended cursor should exist");
            assert!(appended_cursor.byte_offset > initial_offset);
            assert_eq!(appended_cursor.state.usage.total(), 100);
            assert_eq!(appended_cursor.state.tool_calls, 4);

            let sessions = database
                .sessions(
                    "all",
                    SessionListFilters {
                        agent: Some(agent.as_str()),
                        ..SessionListFilters::default()
                    },
                    0,
                    10,
                )
                .expect("sessions should reload after append");
            let detail = database
                .session_detail(&sessions.items[0].id)
                .expect("session detail should reload after append");
            assert!(
                detail
                    .file_changes
                    .iter()
                    .any(|change| change.path == "src/append.rs")
            );
            assert!(detail.file_changes.iter().all(
                |change| change.path != "src/ignored.rs" && !change.path.contains("[external]")
            ));

            let overview = database
                .overview("all", IndexStatus::default())
                .expect("overview should include partial source");
            assert_eq!(overview.totals.usage.total(), 100);
            assert!(overview.agents.iter().any(|item| item.id == agent.as_str()));
            assert!(database.insights("all").expect("insights").sample_size >= 1);
            assert_eq!(database.vcti_profile("all").expect("vcti").session_count, 1);
            let share = crate::export::preview(
                &database,
                ShareRenderRequest {
                    template_id: "usage-overview".into(),
                    locale: "en-US".into(),
                    aspect_ratio: "1:1".into(),
                    theme: "light".into(),
                    range: "all".into(),
                    session_id: None,
                    compare_ids: Vec::new(),
                    title: String::new(),
                    summary: String::new(),
                    project_name: String::new(),
                    metrics: Vec::new(),
                    show_brand: true,
                    show_model: true,
                    show_cost: false,
                    show_project: false,
                    show_behavior_evidence: false,
                    privacy_reviewed: true,
                },
            )
            .expect("share preview should include partial source");
            assert!(share.can_export);

            let before = serde_json::to_value((
                &sessions,
                &detail,
                &overview.totals,
                &overview.daily,
                &overview.hourly,
                &overview.agents,
                &overview.models,
                &overview.tools,
            ))
            .expect("surface should serialize");
            let public_json = format!("{}{}", before, share.svg);
            for private in [
                "sk-text-source-secret",
                "cargo test --secret",
                &project_root,
                "secret old",
                "secret new",
            ] {
                assert!(!public_json.contains(private));
            }

            assert!(
                parse_source_file(&database, &source, true).expect("force reparse should work")
            );
            let sessions_after = database
                .sessions(
                    "all",
                    SessionListFilters {
                        agent: Some(agent.as_str()),
                        ..SessionListFilters::default()
                    },
                    0,
                    10,
                )
                .expect("sessions should reload");
            let detail_after = database
                .session_detail(&sessions_after.items[0].id)
                .expect("session detail should reload");
            let overview_after = database
                .overview("all", IndexStatus::default())
                .expect("overview should reload after reparse");
            assert_eq!(
                before,
                serde_json::to_value((
                    &sessions_after,
                    &detail_after,
                    &overview_after.totals,
                    &overview_after.daily,
                    &overview_after.hourly,
                    &overview_after.agents,
                    &overview_after.models,
                    &overview_after.tools,
                ))
                .expect("surface should serialize after reparse")
            );
        }
    }

    #[test]
    fn untimed_text_usage_stays_session_level_without_becoming_today() {
        for (agent, adapter) in [
            (AgentKind::KimiCode, SourceAdapter::Kimi),
            (AgentKind::OpenClaw, SourceAdapter::OpenClaw),
            (AgentKind::ZCode, SourceAdapter::ZCode),
        ] {
            let temporary = tempfile::tempdir().expect("temporary directory");
            let database = Database::open(temporary.path().join("untimed.sqlite"))
                .expect("database should open");
            let mut state = ParseState::new(agent, format!("{}-untimed", agent.as_str()));
            state.started_at = Some("2026-04-01T00:00:00Z".into());
            state.ended_at = state.started_at.clone();
            let record = untimed_usage_record(adapter);
            match adapter {
                SourceAdapter::Kimi => kimi::parse_record(&mut state, &record),
                SourceAdapter::OpenClaw => openclaw::parse_record(&mut state, &record),
                SourceAdapter::ZCode => zcode::parse_record(&mut state, &record),
                _ => unreachable!("fixture is limited to text history adapters"),
            }
            assert_eq!(state.usage.total(), 100);
            assert!(state.daily.is_empty());
            assert!(state.hourly.is_empty());
            let session_id = database
                .persist_parse_state(agent.as_str(), 1, 1, 1, &state)
                .expect("untimed session should persist");
            assert_eq!(
                database
                    .session_detail(&session_id)
                    .expect("untimed session should load by identity")
                    .summary
                    .usage
                    .total(),
                100
            );
            assert_eq!(
                database
                    .overview("today", IndexStatus::default())
                    .expect("today overview should load")
                    .totals
                    .usage
                    .total(),
                0
            );
        }
    }

    #[test]
    fn source_record_receipts_are_exact_transient_and_private() {
        let mut state = ParseState::new(AgentKind::KimiCode, "bounded-records".into());
        state.source_record_receipt_key = [7; 32];
        let private_left = json!({
            "type":"tool.call", "timestamp":"2026-04-01T00:00:00Z", "name":"Bash",
            "arguments":{"command":"cat alpha", "file_path":"/private/alpha.rs"},
            "prompt":"secret-one", "sourceSpecificSecret":"sk-secret-one"
        });
        let private_right = json!({
            "type":"tool.call", "timestamp":"2026-04-01T00:00:00Z", "name":"Bash",
            "arguments":{"command":"pwd omega", "file_path":"/another/omega.rs"},
            "prompt":"hidden-two", "sourceSpecificSecret":"sk-hidden-two"
        });
        let left_receipt =
            common::source_record_receipt(&private_left, &state.source_record_receipt_key);
        let right_receipt =
            common::source_record_receipt(&private_right, &state.source_record_receipt_key);
        assert_ne!(
            left_receipt, right_receipt,
            "keyed receipts must distinguish different private records without storing them"
        );
        for private in [
            "cat alpha",
            "/private/alpha.rs",
            "secret-one",
            "sk-secret-one",
            "tool.call",
        ] {
            assert!(!left_receipt.contains(private));
        }
        assert!(common::source_record_once(&mut state, &private_left).0);
        assert!(common::source_record_once(&mut state, &private_right).0);
        let first_native = json!({"id":"native-0","type":"metadata"});
        let native_receipt =
            common::source_record_receipt(&first_native, &state.source_record_receipt_key);
        assert!(!native_receipt.contains("native-0"));
        assert!(!native_receipt.contains("metadata"));
        assert!(common::source_record_once(&mut state, &first_native).0);
        for index in 1..=1_000 {
            let record = json!({"id":format!("native-{index}"),"type":"metadata"});
            assert!(common::source_record_once(&mut state, &record).0);
        }
        assert!(!common::source_record_once(&mut state, &first_native).0);

        for index in 0..1_000 {
            let record = json!({"type":"metadata","sequence":index,"secret":"sk-bounded-secret"});
            assert!(common::source_record_once(&mut state, &record).0);
        }
        assert_eq!(state.source_record_ids.len(), 2_003);
        let first_structural = json!({"type":"metadata","sequence":0,"secret":"sk-bounded-secret"});
        assert!(!common::source_record_once(&mut state, &first_structural).0);
        let recent = json!({"type":"metadata","sequence":999,"secret":"sk-bounded-secret"});
        assert!(!common::source_record_once(&mut state, &recent).0);
        let serialized = serde_json::to_string(&state).expect("state should serialize");
        assert!(serialized.len() < 16 * 1024);
        assert!(!serialized.contains("sk-bounded-secret"));
        assert!(!serialized.contains("native-0"));
    }

    #[test]
    fn text_record_receipts_deduplicate_after_long_cross_batch_replay_and_force() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let database_path = temporary.path().join("native-records.sqlite");
        let database = Database::open(database_path.clone()).expect("database should open");
        let receipt_key = database.source_record_receipt_key();
        let key_path = temporary
            .path()
            .join(".native-records.sqlite.source-record-key");
        assert_eq!(
            std::fs::read(&key_path)
                .expect("receipt key should read")
                .len(),
            32
        );
        #[cfg(unix)]
        assert_eq!(
            std::fs::metadata(&key_path)
                .expect("receipt key metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let source_path = temporary.path().join("kimi-history.jsonl");
        let usage = json!({
            "type":"usage.record", "time":1_775_000_000_000_i64,
            "model":"kimi-code/k3", "usage":{"inputOther":60,"output":40}
        });
        let mut source_file = File::create(&source_path).expect("fixture should create");
        writeln!(source_file, "{usage}").expect("usage should write");
        for index in 0..300 {
            writeln!(
                source_file,
                "{}",
                json!({"id":format!("metadata-{index}"),"type":"metadata"})
            )
            .expect("metadata should write");
        }
        source_file.flush().expect("fixture should flush");
        let metadata = source_file.metadata().expect("metadata should load");
        let mut source = SourceFile {
            path: source_path,
            agent: AgentKind::KimiCode,
            adapter: SourceAdapter::Kimi,
            size: metadata.len(),
            modified: 1,
        };
        assert!(parse_source_file(&database, &source, false).expect("source should parse"));

        let mut source_file = std::fs::OpenOptions::new()
            .append(true)
            .open(&source.path)
            .expect("fixture should reopen");
        writeln!(source_file, "{usage}").expect("old usage should replay");
        source_file.flush().expect("replay should flush");
        source.size = source_file.metadata().expect("metadata").len();
        source.modified = 2;
        assert!(parse_source_file(&database, &source, false).expect("replay should parse"));

        let file_hash = stable_hash(&source.path.to_string_lossy());
        let cursor = database
            .load_cursor(&file_hash)
            .expect("cursor should load")
            .expect("cursor should exist");
        assert_eq!(cursor.state.usage.total(), 100);
        assert!(cursor.state.source_record_ids.is_empty());
        let usage_receipt =
            common::source_record_receipt(&usage, &database.source_record_receipt_key());
        assert!(
            database
                .history_record_id_exists(&file_hash, &usage_receipt)
                .expect("usage receipt should load")
        );

        assert!(parse_source_file(&database, &source, true).expect("force reparse should work"));
        let force_cursor = database
            .load_cursor(&file_hash)
            .expect("force cursor should load")
            .expect("force cursor should exist");
        assert_eq!(force_cursor.state.usage.total(), 100);
        assert!(
            database
                .history_record_id_exists(&file_hash, &usage_receipt)
                .expect("force receipt should load")
        );

        database
            .clear_local_data()
            .expect("local data should clear");
        assert!(
            !database
                .history_record_id_exists(&file_hash, &usage_receipt)
                .expect("cleared receipt lookup should work")
        );
        let interrupted_temporary_key = temporary
            .path()
            .join(".native-records.sqlite.source-record-key.interrupted.tmp");
        std::fs::write(&interrupted_temporary_key, [9_u8; 32])
            .expect("interrupted key artifact should write");
        drop(database);
        assert_eq!(
            Database::open(database_path)
                .expect("database should reopen")
                .source_record_receipt_key(),
            receipt_key
        );
        assert!(
            !interrupted_temporary_key.exists(),
            "reopening should retain the published key and clean interrupted artifacts"
        );
    }

    #[test]
    fn leaves_an_incomplete_final_record_for_the_next_pass() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let database = Database::open(temporary.path().join("test.sqlite")).expect("database");
        let source_path = temporary.path().join("session.jsonl");
        let mut source_file = File::create(&source_path).expect("fixture file");
        writeln!(
            source_file,
            "{}",
            serde_json::json!({
                "type":"session_meta",
                "timestamp":"2026-07-19T00:00:00Z",
                "payload":{"id":"session-1"}
            })
        )
        .expect("complete record");
        write!(source_file, "{{\"type\":\"event_msg\"").expect("partial record");
        source_file.flush().expect("flush fixture");
        let metadata = source_file.metadata().expect("metadata");
        let source = SourceFile {
            path: source_path,
            agent: AgentKind::Codex,
            adapter: SourceAdapter::Codex,
            size: metadata.len(),
            modified: 1,
        };
        assert!(parse_source_file(&database, &source, false).expect("parse"));
        let cursor = database
            .load_cursor(&stable_hash(&source.path.to_string_lossy()))
            .expect("cursor query")
            .expect("cursor");
        assert!(cursor.byte_offset < source.size);
        assert_eq!(cursor.state.malformed_records, 0);
    }

    #[test]
    fn reclassifies_a_kimi_source_previously_indexed_as_claude() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let database = Database::open(temporary.path().join("test.sqlite")).expect("database");
        let source_path = temporary.path().join("wire.jsonl");
        let mut source_file = File::create(&source_path).expect("fixture file");
        writeln!(
            source_file,
            "{}",
            serde_json::json!({
                "type": "metadata",
                "created_at": 1_784_678_400_000_i64,
                "protocol_version": 1
            })
        )
        .expect("metadata record");
        writeln!(
            source_file,
            "{}",
            serde_json::json!({
                "type": "usage.record",
                "time": 1_784_678_401_000_i64,
                "model": "kimi-code/k3",
                "usage": {
                    "inputOther": 120,
                    "output": 30,
                    "inputCacheRead": 250,
                    "inputCacheCreation": 0
                }
            })
        )
        .expect("usage record");
        source_file.flush().expect("flush fixture");
        let metadata = source_file.metadata().expect("metadata");
        let file_hash = stable_hash(&source_path.to_string_lossy());

        let mut stale = ParseState::new(AgentKind::ClaudeCode, file_hash.clone());
        stale.started_at = Some("2026-07-22T00:00:00Z".into());
        stale.ended_at = stale.started_at.clone();
        stale.current_model = Some("kimi-code/k3".into());
        stale.usage.input_tokens = 999;
        database
            .persist_parse_state(&file_hash, metadata.len(), 1, metadata.len(), &stale)
            .expect("stale source should persist");

        let source = SourceFile {
            path: source_path,
            agent: AgentKind::KimiCode,
            adapter: SourceAdapter::Kimi,
            size: metadata.len(),
            modified: 1,
        };
        assert!(parse_source_file(&database, &source, false).expect("reparse mismatched source"));

        let kimi_sessions = database
            .sessions(
                "all",
                SessionListFilters {
                    agent: Some("kimi-code"),
                    ..SessionListFilters::default()
                },
                0,
                10,
            )
            .expect("kimi sessions");
        assert_eq!(kimi_sessions.total, 1);
        assert_eq!(
            kimi_sessions.items[0].model.as_deref(),
            Some("kimi-code/k3")
        );
        assert_eq!(kimi_sessions.items[0].usage.total(), 400);
        assert_eq!(
            database
                .sessions(
                    "all",
                    SessionListFilters {
                        agent: Some("claude-code"),
                        ..SessionListFilters::default()
                    },
                    0,
                    10,
                )
                .expect("claude sessions")
                .total,
            0
        );
        assert_eq!(
            database
                .load_cursor(&file_hash)
                .expect("cursor query")
                .expect("cursor")
                .state
                .agent,
            AgentKind::KimiCode
        );
    }
}
