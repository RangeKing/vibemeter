use crate::adapters::{claude, codex, common, cursor, kimi, openclaw};
use crate::database::Database;
use crate::errors::AppResult;
use crate::git_evidence;
use crate::models::{AgentKind, IndexStatus, PARSER_VERSION, ParseState, TokenUsage};
use crate::privacy::stable_hash;
use chrono::Utc;
use chrono::{DateTime, SecondsFormat};
use rusqlite::Connection;
use serde_json::Value;
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
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
    let can_resume = !force
        && cursor.as_ref().is_some_and(|cursor| {
            cursor.source_size <= source.size
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
            Ok(record) => match source.adapter {
                SourceAdapter::Codex => codex::parse_record(&mut state, &record),
                SourceAdapter::Claude => claude::parse_record(&mut state, &record),
                SourceAdapter::Cursor => cursor::parse_record(&mut state, &record),
                SourceAdapter::Kimi => kimi::parse_record(&mut state, &record),
                SourceAdapter::OpenClaw => openclaw::parse_record(&mut state, &record),
            },
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
        if !entry.file_type().is_file()
            || entry.path().extension().and_then(|value| value.to_str()) != Some("jsonl")
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
        if !force && database.load_cursor(&key)?.is_some() {
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
        if !force && database.load_cursor(&key)?.is_some() {
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
    use crate::models::SessionListFilters;
    use std::io::Write;

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
