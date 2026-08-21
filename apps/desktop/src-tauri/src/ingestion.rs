use crate::adapters::{
    claude, codex, common, cursor, database_history, deepseek_harness, grok_build, kimi, openclaw,
    zcode,
};
use crate::database::Database;
use crate::errors::AppResult;
use crate::git_evidence;
use crate::models::{AgentKind, IndexStatus, PARSER_VERSION, ParseState, SessionContentPreview};
use crate::privacy::stable_hash;
use chrono::Utc;
use chrono::{DateTime, SecondsFormat};
use rusqlite::{Connection, params};
use serde_json::Value;
use std::collections::HashSet;
use std::fs::{self, File};
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
    DeepSeekHarness,
    GrokBuild,
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

#[derive(Debug)]
struct PreparedDatabaseHistorySession {
    source_file_hash: String,
    revision_size: u64,
    revision_time: i64,
    state: ParseState,
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
                    if current.warning_count > 0 {
                        current.phase = "partial".into();
                        current.message_key = "index.partial".into();
                    } else {
                        current.phase = "complete".into();
                        current.message_key = "index.complete".into();
                    }
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
    let cursor_database_ready = index_cursor_database(database, force)?;
    let hermes_database_ready = index_hermes_database(database, force)?;
    let mut unavailable_database_sources = HashSet::new();
    if cursor_database_path().is_file() && !cursor_database_ready {
        unavailable_database_sources.insert(AgentKind::Cursor);
    }
    if hermes_database_path().is_file() && !hermes_database_ready {
        unavailable_database_sources.insert(AgentKind::Hermes);
    }
    let partial_database_sources = database
        .sources()?
        .into_iter()
        .filter_map(
            |source| match (source.agent.as_str(), source.status.as_str()) {
                ("cursor", "partial") => Some(AgentKind::Cursor),
                ("hermes", "partial") => Some(AgentKind::Hermes),
                _ => None,
            },
        )
        .collect::<HashSet<_>>();
    if let Ok(mut current) = status.write() {
        current.warning_count = current.warning_count.saturating_add(
            unavailable_database_sources
                .len()
                .saturating_add(partial_database_sources.len()) as u64,
        );
    }
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
        AgentKind::DeepSeekHarness,
        AgentKind::GrokBuild,
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
        let file_history_available = agent_roots.iter().any(|root| Path::new(root).is_dir());
        let available = file_history_available
            || (agent == AgentKind::Cursor && cursor_database_path().is_file())
            || (agent == AgentKind::Hermes && hermes_database_path().is_file())
            || (agent == AgentKind::ZCode && Path::new("/Applications/ZCode.app").is_dir());
        let path_hash = stable_hash(&agent_roots.join("|"));
        database.upsert_source(
            agent,
            &path_hash,
            available,
            if unavailable_database_sources.contains(&agent) {
                if file_history_available {
                    "partial"
                } else {
                    "unavailable"
                }
            } else if partial_database_sources.contains(&agent) {
                "partial"
            } else if available {
                "ready"
            } else {
                "not-found"
            },
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
    let git_allowed = database
        .setting("gitReadAllowed")?
        .is_some_and(|value| value == "true");
    if !force
        && cursor.as_ref().is_some_and(|cursor| {
            cursor.source_size == source.size
                && cursor.source_mtime == source.modified
                && cursor.state.agent == source.agent
                && cursor.state.parser_version == PARSER_VERSION
                && cursor
                    .state
                    .git_evidence
                    .as_ref()
                    .is_some_and(|evidence| git_allowed != (evidence.state == "not-authorized"))
        })
    {
        return Ok(false);
    }

    let fallback_id = if source.adapter == SourceAdapter::Kimi {
        kimi_source_identity(&source.path).unwrap_or_else(|| file_hash.clone())
    } else if source.adapter == SourceAdapter::GrokBuild {
        grok_source_identity(&source.path).unwrap_or_else(|| file_hash.clone())
    } else {
        source
            .path
            .file_stem()
            .and_then(|value| value.to_str())
            .map(ToString::to_string)
            .unwrap_or_else(|| file_hash.clone())
    };
    let can_resume = !matches!(
        source.adapter,
        SourceAdapter::ZCode | SourceAdapter::DeepSeekHarness | SourceAdapter::GrokBuild
    ) && !force
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
    if source.adapter == SourceAdapter::Kimi && state.project_root.is_none() {
        restore_kimi_session_context(&source.path, &mut state);
    }
    if source.adapter == SourceAdapter::GrokBuild {
        grok_build::restore_session_context(&source.path, &mut state);
    }
    if !can_resume {
        state.replace_source_record_ids = true;
    }
    if can_resume
        && state.project_root.is_none()
        && matches!(
            source.adapter,
            SourceAdapter::Kimi | SourceAdapter::OpenClaw | SourceAdapter::GrokBuild
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

    if source.adapter == SourceAdapter::DeepSeekHarness {
        deepseek_harness::read_records(&source.path, |record| {
            deepseek_harness::parse_record(&mut state, record);
        })?;
        common::finalize_run(&mut state);
        persist_complete_state(database, &file_hash, source, &mut state)?;
        return Ok(true);
    }
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
                        SourceAdapter::Kimi
                            | SourceAdapter::OpenClaw
                            | SourceAdapter::ZCode
                            | SourceAdapter::GrokBuild
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
                    SourceAdapter::DeepSeekHarness => {
                        deepseek_harness::parse_record(&mut state, &record)
                    }
                    SourceAdapter::Kimi => kimi::parse_record(&mut state, &record),
                    SourceAdapter::GrokBuild => grok_build::parse_record(&mut state, &record),
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

fn persist_complete_state(
    database: &Database,
    file_hash: &str,
    source: &SourceFile,
    state: &mut ParseState,
) -> AppResult<()> {
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
    database.persist_parse_state(file_hash, source.size, source.modified, source.size, state)?;
    Ok(())
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

fn kimi_session_root(path: &Path) -> Option<&Path> {
    let agent_dir = path.parent()?;
    let agents_dir = agent_dir.parent()?;
    if agents_dir.file_name().and_then(|value| value.to_str()) != Some("agents") {
        return None;
    }
    agents_dir.parent()
}

fn kimi_source_identity(path: &Path) -> Option<String> {
    let session_root = kimi_session_root(path)?;
    let session = session_root
        .file_name()?
        .to_str()?
        .strip_prefix("session_")?;
    if session.is_empty()
        || !session
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
    {
        return None;
    }
    let agent = path.parent()?.file_name()?.to_str()?;
    Some(if agent == "main" {
        session.to_string()
    } else {
        format!("{session}:{agent}")
    })
}

fn restore_kimi_session_context(path: &Path, state: &mut ParseState) {
    let Some(session_root) = kimi_session_root(path) else {
        return;
    };
    let Ok(content) = fs::read(session_root.join("state.json")) else {
        return;
    };
    if content.len() > MAX_RECORD_BYTES {
        return;
    }
    let Ok(metadata) = serde_json::from_slice::<Value>(&content) else {
        return;
    };
    common::set_project(
        state,
        metadata
            .get("workDir")
            .or_else(|| metadata.get("workspacePath"))
            .or_else(|| metadata.get("cwd"))
            .and_then(Value::as_str),
    );
}

fn grok_source_identity(path: &Path) -> Option<String> {
    let summary = path.parent()?.join("summary.json");
    let content = fs::read(summary).ok()?;
    if content.len() > MAX_RECORD_BYTES {
        return None;
    }
    serde_json::from_slice::<Value>(&content)
        .ok()?
        .get("info")
        .and_then(|info| info.get("id"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
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
        let file_name = entry.file_name().to_string_lossy();
        if adapter == SourceAdapter::GrokBuild && file_name != "updates.jsonl" {
            continue;
        }
        let is_zcode_json = adapter == SourceAdapter::ZCode && extension == Some("json");
        let is_deepseek_log = adapter == SourceAdapter::DeepSeekHarness
            && entry
                .file_name()
                .to_str()
                .is_some_and(|name| name == "session.jsonl.zstd");
        let is_jsonl = extension == Some("jsonl");
        if !is_jsonl && !is_zcode_json && !is_deepseek_log {
            continue;
        }
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
        agent: AgentKind::DeepSeekHarness,
        path: home.join(".dsh/sessions"),
        adapter: SourceAdapter::DeepSeekHarness,
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
        agent: AgentKind::GrokBuild,
        path: home.join(".grok/sessions"),
        adapter: SourceAdapter::GrokBuild,
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

pub(crate) fn session_content_preview(
    database: &Database,
    session_id: &str,
) -> AppResult<SessionContentPreview> {
    let Some((agent_name, source_file_hash)) = database.session_source_locator(session_id)? else {
        return Ok(SessionContentPreview::default());
    };
    let Some(agent) = agent_kind_from_name(&agent_name) else {
        return Ok(SessionContentPreview::default());
    };
    for root in source_roots()
        .into_iter()
        .filter(|root| root.agent == agent && root.path.is_dir())
    {
        let mut files = Vec::new();
        collect_jsonl_files(&root.path, root.agent, root.adapter, &mut files);
        if let Some(source) = files
            .iter()
            .find(|source| stable_hash(&source.path.to_string_lossy()) == source_file_hash)
        {
            return read_content_preview_from_source(source, database.source_record_receipt_key());
        }
    }
    Ok(SessionContentPreview::default())
}

fn agent_kind_from_name(value: &str) -> Option<AgentKind> {
    match value {
        "claude-code" => Some(AgentKind::ClaudeCode),
        "codex" => Some(AgentKind::Codex),
        "deepseek-harness" => Some(AgentKind::DeepSeekHarness),
        "kimi-code" => Some(AgentKind::KimiCode),
        "grok-build" => Some(AgentKind::GrokBuild),
        "cursor" => Some(AgentKind::Cursor),
        "openclaw" => Some(AgentKind::OpenClaw),
        "hermes" => Some(AgentKind::Hermes),
        "zcode" => Some(AgentKind::ZCode),
        _ => None,
    }
}

fn read_content_preview_from_source(
    source: &SourceFile,
    receipt_key: [u8; 32],
) -> AppResult<SessionContentPreview> {
    let fallback_id = source
        .path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("session")
        .to_string();
    let mut state = ParseState::new(source.agent, fallback_id);
    state.source_record_receipt_key = receipt_key;
    {
        let mut parse_record = |record: &Value| match source.adapter {
            SourceAdapter::Codex => codex::parse_record(&mut state, record),
            SourceAdapter::Claude => claude::parse_record(&mut state, record),
            SourceAdapter::Cursor => cursor::parse_record(&mut state, record),
            SourceAdapter::DeepSeekHarness => deepseek_harness::parse_record(&mut state, record),
            SourceAdapter::Kimi => kimi::parse_record(&mut state, record),
            SourceAdapter::GrokBuild => grok_build::parse_record(&mut state, record),
            SourceAdapter::OpenClaw => openclaw::parse_record(&mut state, record),
            SourceAdapter::ZCode => zcode::parse_record(&mut state, record),
        };
        if source.adapter == SourceAdapter::DeepSeekHarness {
            deepseek_harness::read_records(&source.path, |record| parse_record(record))?;
        } else if source.adapter == SourceAdapter::ZCode
            && source.path.extension().and_then(|value| value.to_str()) == Some("json")
        {
            let mut content = Vec::new();
            File::open(&source.path)?.read_to_end(&mut content)?;
            if content.len() <= MAX_RECORD_BYTES
                && let Ok(record) = serde_json::from_slice::<Value>(&content)
            {
                parse_record(&record);
            }
        } else {
            let mut reader = BufReader::with_capacity(256 * 1024, File::open(&source.path)?);
            let mut buffer = Vec::with_capacity(64 * 1024);
            loop {
                buffer.clear();
                if reader.read_until(b'\n', &mut buffer)? == 0 {
                    break;
                }
                if buffer.len() > MAX_RECORD_BYTES {
                    continue;
                }
                if let Ok(record) = serde_json::from_slice::<Value>(&buffer) {
                    parse_record(&record);
                }
            }
        }
    }
    Ok(SessionContentPreview {
        prompt: state.prompt_excerpt,
        output: state.result_excerpt,
    })
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

fn index_cursor_database(database: &Database, force: bool) -> AppResult<bool> {
    index_database_source_at(database, AgentKind::Cursor, &cursor_database_path(), force)
}

fn index_hermes_database(database: &Database, force: bool) -> AppResult<bool> {
    index_database_source_at(database, AgentKind::Hermes, &hermes_database_path(), force)
}

fn index_database_source_at(
    database: &Database,
    agent: AgentKind,
    path: &Path,
    force: bool,
) -> AppResult<bool> {
    let available = path.is_file();
    let path_hash = stable_hash(&path.to_string_lossy());
    database.upsert_source(
        agent,
        &path_hash,
        available,
        if available { "indexing" } else { "not-found" },
    )?;
    if !available {
        database.set_source_warning(agent, &path_hash, "database-unavailable", false)?;
        database.set_source_warning(agent, &path_hash, "database-partial", false)?;
        return Ok(false);
    }
    let source_revision = match database_history::source_revision(path) {
        Ok(revision) => revision,
        Err(_) => {
            database.set_source_warning(agent, &path_hash, "database-partial", false)?;
            database.set_source_warning(agent, &path_hash, "database-unavailable", true)?;
            database.upsert_source(agent, &path_hash, true, "unavailable")?;
            return Ok(false);
        }
    };
    let source_cursor_key = database_source_cursor_key(database, agent, path);
    if !force && let Some(cached) = database.setting(&source_cursor_key)? {
        let mut fields = cached.splitn(3, '\n');
        let parser_matches = fields.next() == Some(PARSER_VERSION);
        let revision_matches = fields.next() == Some(source_revision.as_str());
        let cached_status = fields.next().unwrap_or("ready");
        if parser_matches && revision_matches {
            let partial = cached_status == "partial";
            database.set_source_warning(agent, &path_hash, "database-unavailable", false)?;
            database.set_source_warning(agent, &path_hash, "database-partial", partial)?;
            database.upsert_source(
                agent,
                &path_hash,
                true,
                if partial { "partial" } else { "ready" },
            )?;
            return Ok(true);
        }
    }
    let reader = match database_history::DatabaseHistoryReader::open(agent, path) {
        Ok(reader) => reader,
        Err(_) => {
            database.set_source_warning(agent, &path_hash, "database-partial", false)?;
            database.set_source_warning(agent, &path_hash, "database-unavailable", true)?;
            database.upsert_source(agent, &path_hash, true, "unavailable")?;
            return Ok(false);
        }
    };
    let mut source_read_failed = false;
    let pipeline_result = (|| -> AppResult<database_history::DatabaseHistoryReadSummary> {
        let mut stage = Connection::open(reader.normalized_stage_path())?;
        stage.execute_batch(
            "PRAGMA journal_mode=OFF;
             PRAGMA synchronous=OFF;
             CREATE TABLE normalized_sessions(
                ordinal INTEGER PRIMARY KEY AUTOINCREMENT,
                source_file_hash TEXT NOT NULL,
                revision_size INTEGER NOT NULL,
                revision_time INTEGER NOT NULL,
                state_json TEXT NOT NULL
             );",
        )?;
        let stage_transaction = stage.transaction()?;
        let mut staging_error = None;
        let read_summary = reader
            .read_each(|session| {
                let result = prepare_database_history_session(database, agent, path, &session)
                    .and_then(|prepared| {
                        stage_transaction.execute(
                            "INSERT INTO normalized_sessions(
                                source_file_hash,revision_size,revision_time,state_json
                             ) VALUES(?1,?2,?3,?4)",
                            params![
                                prepared.source_file_hash,
                                prepared.revision_size as i64,
                                prepared.revision_time,
                                serde_json::to_string(&prepared.state)?,
                            ],
                        )?;
                        Ok(())
                    });
                match result {
                    Ok(()) => true,
                    Err(error) => {
                        staging_error = Some(error);
                        false
                    }
                }
            })
            .inspect_err(|_| {
                source_read_failed = true;
            })?;
        if let Some(error) = staging_error {
            return Err(error);
        }
        if !read_summary.completed {
            return Err(crate::errors::AppError::InvalidRequest(
                "database history indexing stopped before completion".into(),
            ));
        }
        stage_transaction.commit()?;
        stage.pragma_update(None, "query_only", true)?;
        let mut statement = stage.prepare(
            "SELECT source_file_hash,revision_size,revision_time,state_json
             FROM normalized_sessions ORDER BY ordinal",
        )?;
        let mut rows = statement.query([])?;
        database.with_parse_state_batch(|transaction| {
            while let Some(row) = rows.next()? {
                let source_file_hash = row.get::<_, String>(0)?;
                let revision_size = row.get::<_, i64>(1)?.max(0) as u64;
                let revision_time = row.get::<_, i64>(2)?;
                let state = serde_json::from_str::<ParseState>(&row.get::<_, String>(3)?)?;
                if !force
                    && Database::cursor_matches_in_transaction(
                        transaction,
                        &source_file_hash,
                        revision_size,
                        revision_time,
                    )?
                {
                    continue;
                }
                Database::persist_parse_state_in_transaction(
                    transaction,
                    &source_file_hash,
                    revision_size,
                    revision_time,
                    revision_size,
                    &state,
                )?;
            }
            Ok(())
        })?;
        Ok(read_summary)
    })();
    let read_summary = match pipeline_result {
        Ok(summary) => summary,
        Err(error) => {
            database.set_source_warning(agent, &path_hash, "database-partial", false)?;
            database.set_source_warning(agent, &path_hash, "database-unavailable", true)?;
            database.upsert_source(agent, &path_hash, true, "unavailable")?;
            if source_read_failed {
                return Ok(false);
            }
            return Err(error);
        }
    };
    let partial = read_summary.partial;
    database.set_source_warning(agent, &path_hash, "database-unavailable", false)?;
    database.set_source_warning(agent, &path_hash, "database-partial", partial)?;
    let source_status = if partial { "partial" } else { "ready" };
    database.upsert_source(agent, &path_hash, true, source_status)?;
    if database_history::source_revision(path).is_ok_and(|current| current == source_revision) {
        database.set_setting(
            &source_cursor_key,
            &format!("{PARSER_VERSION}\n{source_revision}\n{source_status}"),
        )?;
    }
    Ok(true)
}

fn prepare_database_history_session(
    database: &Database,
    agent: AgentKind,
    path: &Path,
    session: &database_history::DatabaseHistorySession,
) -> AppResult<PreparedDatabaseHistorySession> {
    let identity = serde_json::json!({
        "type": "database-history-session",
        "agent": agent.as_str(),
        "sourcePath": path.to_string_lossy(),
        "sourceSession": session.source_session_id,
    });
    let key = common::source_record_receipt(&identity, &database.source_record_receipt_key());
    let (revision_size, revision_time) = database_session_revision(database, agent, path, session);
    let mut state = ParseState::new(
        agent,
        database_session_reference(database, agent, path, &session.source_session_id),
    );
    state.source_session_observed = true;
    state.started_at = session.started_at.clone();
    state.ended_at = session.ended_at.clone();
    common::set_observed_title(&mut state, session.title.as_deref());
    common::set_model(&mut state, session.model.as_deref());
    common::record_usage(&mut state, &session.usage, None, session.model.as_deref());
    if let Some(cost) = session.estimated_cost_usd {
        state.estimated_cost_usd = cost;
        state.cost_coverage_tokens = session.usage.total();
    }
    state.malformed_records = session.malformed_records;
    state.unknown_records = session.unknown_records;
    for event in &session.events {
        let source_event_id = database_event_reference(
            database,
            agent,
            path,
            &session.source_session_id,
            event.source_event_id.as_deref(),
        );
        match event.event_type.as_str() {
            "prompt" => common::observe_prompt_with_source(
                &mut state,
                event.occurred_at.as_deref(),
                source_event_id.as_deref(),
            ),
            "tool" => {
                common::observe_timestamp(&mut state, event.occurred_at.as_deref(), false);
                if event.occurred_at.is_none() {
                    state.event_count = state.event_count.saturating_add(1);
                }
                common::record_tool_with_source(
                    &mut state,
                    &event.name,
                    None,
                    event.occurred_at.as_deref(),
                    source_event_id.as_deref(),
                );
            }
            _ => {
                common::observe_timestamp(&mut state, event.occurred_at.as_deref(), false);
                if event.occurred_at.is_none() {
                    state.event_count = state.event_count.saturating_add(1);
                }
                common::record_event_with_source(
                    &mut state,
                    &event.event_type,
                    &event.category,
                    &event.name,
                    None,
                    event.occurred_at.as_deref(),
                    source_event_id.as_deref(),
                );
            }
        }
    }
    state.tool_calls = state.tool_calls.max(session.declared_tool_calls);
    common::finalize_run(&mut state);
    Ok(PreparedDatabaseHistorySession {
        source_file_hash: key,
        revision_size,
        revision_time,
        state,
    })
}

fn database_source_cursor_key(database: &Database, agent: AgentKind, path: &Path) -> String {
    let payload = serde_json::json!({
        "type": "database-history-source",
        "agent": agent.as_str(),
        "sourcePath": path.to_string_lossy(),
    });
    format!(
        "databaseHistoryRevision:{}",
        common::source_record_receipt(&payload, &database.source_record_receipt_key())
    )
}

fn database_session_reference(
    database: &Database,
    agent: AgentKind,
    path: &Path,
    source_session_id: &str,
) -> String {
    let payload = serde_json::json!({
        "type": "database-history-session-reference",
        "agent": agent.as_str(),
        "sourcePath": path.to_string_lossy(),
        "sourceSession": source_session_id,
    });
    common::source_record_receipt(&payload, &database.source_record_receipt_key())
}

fn database_event_reference(
    database: &Database,
    agent: AgentKind,
    path: &Path,
    source_session_id: &str,
    source_event_id: Option<&str>,
) -> Option<String> {
    let source_event_id = source_event_id?;
    let payload = serde_json::json!({
        "type": "database-history-event",
        "agent": agent.as_str(),
        "sourcePath": path.to_string_lossy(),
        "sourceSession": source_session_id,
        "sourceEvent": source_event_id,
    });
    Some(common::source_record_receipt(
        &payload,
        &database.source_record_receipt_key(),
    ))
}

fn database_session_revision(
    database: &Database,
    agent: AgentKind,
    path: &Path,
    session: &database_history::DatabaseHistorySession,
) -> (u64, i64) {
    let events = session
        .events
        .iter()
        .map(|event| {
            serde_json::json!({
                "occurredAt": event.occurred_at,
                "eventType": event.event_type,
                "category": event.category,
                "name": event.name,
                "sourceEventId": event.source_event_id,
            })
        })
        .collect::<Vec<_>>();
    let payload = serde_json::json!({
        "type": "database-history-revision",
        "agent": agent.as_str(),
        "sourcePath": path.to_string_lossy(),
        "sourceSession": session.source_session_id,
        "title": session.title,
        "model": session.model,
        "startedAt": session.started_at,
        "endedAt": session.ended_at,
        "usage": session.usage,
        "estimatedCostUsd": session.estimated_cost_usd,
        "declaredToolCalls": session.declared_tool_calls,
        "malformedRecords": session.malformed_records,
        "unknownRecords": session.unknown_records,
        "sourceRevision": session.source_revision,
        "events": events,
    });
    let receipt = common::source_record_receipt(&payload, &database.source_record_receipt_key());
    let digest = receipt.split_once(':').map_or("", |(_, digest)| digest);
    let first = digest.get(..15).unwrap_or("0");
    let second = digest.get(15..30).unwrap_or("0");
    (
        u64::from_str_radix(first, 16).unwrap_or(1).max(1),
        i64::from_str_radix(second, 16).unwrap_or_default(),
    )
}

fn timestamp_from_seconds(value: f64) -> Option<String> {
    if !value.is_finite() || value < 0.0 {
        return None;
    }
    DateTime::<Utc>::from_timestamp(value as i64, (value.fract() * 1_000_000_000.0) as u32)
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

    #[test]
    fn kimi_wire_uses_native_session_identity_and_state_project() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let session = temporary.path().join("session_native-id");
        let wire = session.join("agents/agent-0/wire.jsonl");
        fs::create_dir_all(wire.parent().expect("agent directory")).expect("agent directory");
        fs::write(&wire, "").expect("wire");
        fs::write(
            session.join("state.json"),
            serde_json::json!({"workDir":"/tmp/kimi-project"}).to_string(),
        )
        .expect("state");

        assert_eq!(
            kimi_source_identity(&wire).as_deref(),
            Some("native-id:agent-0")
        );
        let mut state = ParseState::new(AgentKind::KimiCode, "fallback".into());
        restore_kimi_session_context(&wire, &mut state);
        assert_eq!(state.project_label.as_deref(), Some("kimi-project"));
    }

    #[test]
    fn session_content_preview_reads_source_without_persisting_a_copy() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let path = temporary.path().join("cursor-session.jsonl");
        let mut file = File::create(&path).expect("preview source");
        writeln!(
            file,
            "{}",
            json!({"role":"user","message":{"content":"Add a system proxy toggle"}})
        )
        .expect("prompt");
        writeln!(
            file,
            "{}",
            json!({"role":"assistant","message":{"content":"The proxy toggle is ready"}})
        )
        .expect("output");
        file.flush().expect("flush preview source");
        let metadata = file.metadata().expect("preview metadata");
        let source = SourceFile {
            path,
            agent: AgentKind::Cursor,
            adapter: SourceAdapter::Cursor,
            size: metadata.len(),
            modified: 0,
        };

        let preview = read_content_preview_from_source(&source, [7; 32]).expect("preview");

        assert_eq!(preview.prompt.as_deref(), Some("Add a system proxy toggle"));
        assert_eq!(preview.output.as_deref(), Some("The proxy toggle is ready"));
    }

    #[test]
    fn unchanged_source_reindexes_when_git_permission_changes() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let database = Database::open(temporary.path().join("git-permission.sqlite"))
            .expect("database should open");
        let project_root = temporary.path().join("project");
        fs::create_dir_all(&project_root).expect("project directory");
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q"])
                .current_dir(&project_root)
                .status()
                .expect("git init")
                .success()
        );

        let source_path = temporary.path().join("model-io-git-session.jsonl");
        fs::write(
            &source_path,
            format!(
                "{}\n",
                json!({
                    "type":"model_io",
                    "sessionId":"git-session",
                    "querySource":"main_turn",
                    "startedAt":"2026-08-16T08:00:00Z",
                    "workspacePath":project_root,
                    "request":{"messages":[{"role":"user","content":"Inspect Git"}]}
                })
            ),
        )
        .expect("source fixture");
        let metadata = fs::metadata(&source_path).expect("source metadata");
        let source = SourceFile {
            path: source_path,
            agent: AgentKind::ZCode,
            adapter: SourceAdapter::ZCode,
            size: metadata.len(),
            modified: 1,
        };

        assert!(parse_source_file(&database, &source, false).expect("initial parse"));
        let session_id = database
            .sessions("all", SessionListFilters::default(), 0, 10)
            .expect("sessions")
            .items[0]
            .id
            .clone();
        assert_eq!(
            database
                .session_detail(&session_id)
                .expect("initial detail")
                .git_evidence
                .state,
            "not-authorized"
        );

        database
            .set_setting("gitReadAllowed", "true")
            .expect("enable Git trajectory");
        assert!(
            parse_source_file(&database, &source, false).expect("permission refresh should parse")
        );
        let refreshed = database
            .session_detail(&session_id)
            .expect("refreshed detail");
        assert!(refreshed.git_evidence.available);
        assert_eq!(refreshed.git_evidence.state, "available");
    }

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

    fn database_source_bytes(path: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        let sidecar = |suffix: &str| PathBuf::from(format!("{}{suffix}", path.to_string_lossy()));
        [path.to_path_buf(), sidecar("-wal"), sidecar("-shm")]
            .into_iter()
            .filter(|candidate| candidate.is_file())
            .map(|candidate| {
                let bytes =
                    std::fs::read(&candidate).expect("source database artifact should read");
                (candidate, bytes)
            })
            .collect()
    }

    #[test]
    fn cursor_database_history_is_read_only_idempotent_and_account_isolated() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let source_path = temporary.path().join("cursor-source.sqlite");
        let source = Connection::open(&source_path).expect("cursor source should open");
        source
            .execute_batch(
                "PRAGMA journal_mode=WAL;
                 PRAGMA wal_autocheckpoint=0;
                 CREATE TABLE conversation_summaries(
                    conversationId TEXT PRIMARY KEY, title TEXT, tldr TEXT,
                    model TEXT, updatedAt INTEGER NOT NULL
                 );
                 CREATE TABLE ai_code_hashes(
                    hash TEXT PRIMARY KEY, conversationId TEXT, timestamp INTEGER,
                    createdAt INTEGER NOT NULL, model TEXT, source TEXT
                 );
                 CREATE TABLE account_usage(
                    account_id TEXT, input_tokens INTEGER, output_tokens INTEGER, secret TEXT
                 );
                 INSERT INTO conversation_summaries VALUES
                    ('cursor-session-1','Cursor local work','private tldr must stay local',
                     'composer-test',1775000003000),
                    ('person@example.com','Cursor summary only','private summary must stay local',
                     'sk-super-secret-token',1775000004000);
                 INSERT INTO ai_code_hashes VALUES
                    ('edit-hash-1','cursor-session-1',1775000001000,1775000001000,'composer-test','private-source-a'),
                    ('edit-hash-2','cursor-session-1',1775000002000,1775000002000,'composer-test','private-source-b'),
                    ('orphan-hash',NULL,1775000002000,1775000002000,'composer-test','private-source-c');
                 INSERT INTO account_usage VALUES('account-elsewhere',999999,999999,'sk-account-secret');",
            )
            .expect("cursor fixture should write");
        let before = database_source_bytes(&source_path);
        let vibemeter_path = temporary.path().join("vibemeter.sqlite");
        let database = Database::open(vibemeter_path.clone()).expect("database should open");

        assert!(
            index_database_source_at(&database, AgentKind::Cursor, &source_path, false)
                .expect("cursor database should index")
        );
        assert_eq!(database_source_bytes(&source_path), before);
        let first = database
            .sessions(
                "all",
                SessionListFilters {
                    agent: Some("cursor"),
                    ..SessionListFilters::default()
                },
                0,
                100,
            )
            .expect("cursor sessions should load");
        assert_eq!(first.total, 2);
        assert!(first.items.iter().all(|session| session.usage.total() == 0));
        let cursor_work = first
            .items
            .iter()
            .find(|session| session.title == "Cursor local work")
            .expect("event-backed Cursor session should remain visible");
        let detail = database
            .session_detail(&cursor_work.id)
            .expect("cursor detail should load");
        assert_eq!(detail.phases.len(), 1);
        assert_eq!(
            detail
                .phases
                .iter()
                .map(|phase| phase.event_count)
                .sum::<u64>(),
            2
        );
        assert!(detail.phases.iter().all(|phase| phase.phase_key == "edit"));
        assert!(
            detail
                .phases
                .iter()
                .flat_map(|phase| &phase.events)
                .all(|event| event.event_type != "prompt")
        );
        let summary_only = first
            .items
            .iter()
            .find(|session| session.title == "Cursor summary only")
            .expect("summary-only Cursor session should remain visible");
        let summary_only_detail = database
            .session_detail(&summary_only.id)
            .expect("summary-only Cursor detail should load");
        assert!(
            summary_only_detail
                .phases
                .iter()
                .flat_map(|phase| &phase.events)
                .next()
                .is_none(),
            "summary metadata must not be promoted into an observed activity"
        );
        assert_eq!(summary_only.active_seconds, 0);
        assert_eq!(summary_only.model, None);
        let evidence = Connection::open_with_flags(
            &vibemeter_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .expect("persisted evidence should open read-only");
        let mut statement = evidence
            .prepare(
                "SELECT source_event_id FROM canonical_events
                 WHERE source='history-index' AND source_event_id IS NOT NULL
                   AND deleted_at IS NULL",
            )
            .expect("event identities should query");
        let identities = statement
            .query_map([], |row| row.get::<_, String>(0))
            .expect("event identities should load")
            .collect::<Result<Vec<_>, _>>()
            .expect("event identities should collect");
        assert_eq!(identities.len(), 2);
        assert!(identities.iter().all(|identity| {
            !identity.contains("edit-hash-1")
                && !identity.contains("edit-hash-2")
                && !identity.contains("person@example.com")
        }));
        let session_identities = evidence
            .prepare("SELECT source_session_id FROM sessions")
            .expect("session identities should query")
            .query_map([], |row| row.get::<_, String>(0))
            .expect("session identities should load")
            .collect::<Result<Vec<_>, _>>()
            .expect("session identities should collect");
        assert!(session_identities.iter().all(|identity| {
            identity.starts_with("keyed:")
                && !identity.contains("cursor-session-1")
                && !identity.contains("person@example.com")
        }));
        let overview = database
            .overview("all", IndexStatus::default())
            .expect("overview should load");
        assert_eq!(
            overview.totals.usage.total(),
            0,
            "Cursor account rows must never enter local-history totals"
        );
        assert_eq!(overview.behavior.prompt_count, 0);
        assert_eq!(
            database
                .vcti_profile("all")
                .expect("local VCTI should load")
                .session_count,
            2
        );
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
        .expect("local share preview should load");
        assert!(share.can_export);
        assert!(!share.svg.contains("account-elsewhere"));
        assert!(!share.svg.contains("sk-account-secret"));

        let snapshot_count = database_history::snapshot_creation_count(&source_path);
        assert!(
            index_database_source_at(&database, AgentKind::Cursor, &source_path, false)
                .expect("repeat cursor index should work")
        );
        assert_eq!(
            database_history::snapshot_creation_count(&source_path),
            snapshot_count,
            "unchanged database history must skip the full snapshot"
        );
        let repeated = database
            .sessions(
                "all",
                SessionListFilters {
                    agent: Some("cursor"),
                    ..SessionListFilters::default()
                },
                0,
                100,
            )
            .expect("cursor sessions should reload");
        assert_eq!(repeated.total, 2);
        let repeated_work = repeated
            .items
            .iter()
            .find(|session| session.title == "Cursor local work")
            .expect("repeated Cursor session should remain visible");
        assert_eq!(
            database
                .session_detail(&repeated_work.id)
                .expect("cursor detail should reload")
                .phases
                .len(),
            1
        );
        #[cfg(unix)]
        {
            let original_permissions = std::fs::metadata(&source_path)
                .expect("source permissions should load")
                .permissions();
            std::fs::set_permissions(&source_path, std::fs::Permissions::from_mode(0o000))
                .expect("source should become unreadable after its revision is cached");
            let revoked =
                index_database_source_at(&database, AgentKind::Cursor, &source_path, false);
            std::fs::set_permissions(&source_path, original_permissions)
                .expect("source permissions should restore");
            assert!(
                !revoked.expect("revoked read access should degrade safely"),
                "a cached revision must not hide revoked source access"
            );
            assert_eq!(
                database
                    .sources()
                    .expect("sources should load")
                    .into_iter()
                    .find(|source| source.agent == "cursor")
                    .expect("Cursor source status")
                    .status,
                "unavailable"
            );
            assert_eq!(
                database
                    .sessions(
                        "all",
                        SessionListFilters {
                            agent: Some("cursor"),
                            ..SessionListFilters::default()
                        },
                        0,
                        100,
                    )
                    .expect("existing Cursor sessions should remain")
                    .total,
                2
            );
        }
        assert_eq!(database_source_bytes(&source_path), before);
    }

    #[test]
    fn database_history_rejects_active_rollback_journals_and_never_reads_uncommitted_rows() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let source_path = temporary.path().join("cursor-delete-journal.sqlite");
        let source = Connection::open(&source_path).expect("cursor source should open");
        source
            .execute_batch(
                "PRAGMA journal_mode=DELETE;
                 CREATE TABLE conversation_summaries(
                    conversationId TEXT PRIMARY KEY, title TEXT, updatedAt INTEGER NOT NULL
                 );
                 INSERT INTO conversation_summaries VALUES(
                    'committed-session','Committed title',1775000000000
                 );",
            )
            .expect("committed fixture should write");
        source
            .execute_batch(
                "PRAGMA cache_size=10;
                 BEGIN IMMEDIATE;
                 UPDATE conversation_summaries
                 SET title='Uncommitted title' WHERE conversationId='committed-session';
                 INSERT INTO conversation_summaries VALUES(
                    'uncommitted-session','Uncommitted row',1775000001000
                 );
                 WITH RECURSIVE counter(value) AS(
                    SELECT 1 UNION ALL SELECT value + 1 FROM counter WHERE value < 200
                 )
                 INSERT INTO conversation_summaries(conversationId, title, updatedAt)
                 SELECT 'spill-' || value, hex(zeroblob(4096)), 1775000001000 + value
                 FROM counter;",
            )
            .expect("uncommitted fixture should write");
        let journal_path = PathBuf::from(format!("{}-journal", source_path.to_string_lossy()));
        assert!(
            std::fs::metadata(&journal_path)
                .expect("rollback journal should exist")
                .len()
                > 0
        );
        assert!(
            std::fs::read(&journal_path)
                .expect("rollback journal header should read")
                .iter()
                .take(8)
                .any(|byte| *byte != 0),
            "the fixture must force a hot journal header before testing rejection"
        );
        let database = Database::open(temporary.path().join("vibemeter.sqlite"))
            .expect("database should open");
        assert!(
            !index_database_source_at(&database, AgentKind::Cursor, &source_path, false)
                .expect("hot journal should degrade safely")
        );
        assert_eq!(
            database
                .sessions(
                    "all",
                    SessionListFilters {
                        agent: Some("cursor"),
                        ..SessionListFilters::default()
                    },
                    0,
                    100,
                )
                .expect("sessions should remain queryable")
                .total,
            0
        );
        source
            .execute_batch("ROLLBACK")
            .expect("uncommitted source write should roll back");
        assert!(
            index_database_source_at(&database, AgentKind::Cursor, &source_path, false)
                .expect("committed source should index after rollback")
        );
        let sessions = database
            .sessions(
                "all",
                SessionListFilters {
                    agent: Some("cursor"),
                    ..SessionListFilters::default()
                },
                0,
                100,
            )
            .expect("committed sessions should load");
        assert_eq!(sessions.total, 1);
        assert_eq!(sessions.items[0].title, "Committed title");
    }

    #[test]
    fn database_history_accepts_an_idle_persistent_rollback_journal() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let source_path = temporary.path().join("cursor-persist-journal.sqlite");
        let source = Connection::open(&source_path).expect("Cursor source should open");
        source
            .execute_batch(
                "PRAGMA journal_mode=PERSIST;
                 CREATE TABLE conversation_summaries(
                    conversationId TEXT PRIMARY KEY, title TEXT, updatedAt INTEGER NOT NULL
                 );
                 INSERT INTO conversation_summaries VALUES(
                    'persist-session','Committed persistent journal',1775000000000
                 );",
            )
            .expect("persistent journal fixture should write");
        let journal_path = PathBuf::from(format!("{}-journal", source_path.to_string_lossy()));
        assert!(
            std::fs::metadata(&journal_path)
                .expect("persistent journal should remain")
                .len()
                > 512
        );
        let database = Database::open(temporary.path().join("vibemeter.sqlite"))
            .expect("database should open");
        assert!(
            index_database_source_at(&database, AgentKind::Cursor, &source_path, false)
                .expect("idle persistent journal should be readable")
        );
        assert_eq!(
            database
                .sessions(
                    "all",
                    SessionListFilters {
                        agent: Some("cursor"),
                        ..SessionListFilters::default()
                    },
                    0,
                    100,
                )
                .expect("persistent-journal session should load")
                .total,
            1
        );
    }

    #[test]
    fn database_history_publishes_all_changed_sessions_atomically() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let source_path = temporary.path().join("cursor-atomic.sqlite");
        let source = Connection::open(&source_path).expect("Cursor source should open");
        source
            .execute_batch(
                "CREATE TABLE conversation_summaries(
                    conversationId TEXT PRIMARY KEY, title TEXT, updatedAt INTEGER NOT NULL
                 );
                 INSERT INTO conversation_summaries VALUES
                    ('a-session','First session',1775000000000),
                    ('z-session','Trigger failure',1775000001000);",
            )
            .expect("atomic source fixture should write");
        drop(source);
        let database_path = temporary.path().join("vibemeter.sqlite");
        let database = Database::open(database_path.clone()).expect("database should open");
        let fixture = Connection::open(database_path).expect("fixture database should open");
        fixture
            .execute_batch(
                "CREATE TRIGGER fail_database_history_fixture
                 BEFORE INSERT ON sessions
                 WHEN NEW.title='Trigger failure'
                 BEGIN
                    SELECT RAISE(ABORT, 'fixture persistence failure');
                 END;",
            )
            .expect("failure trigger should install");
        drop(fixture);
        assert!(
            index_database_source_at(&database, AgentKind::Cursor, &source_path, false).is_err(),
            "a persistence failure must abort the whole source generation"
        );
        assert_eq!(
            database
                .sessions(
                    "all",
                    SessionListFilters {
                        agent: Some("cursor"),
                        ..SessionListFilters::default()
                    },
                    0,
                    100,
                )
                .expect("rolled-back sessions should remain queryable")
                .total,
            0,
            "the first session must roll back with the failing second session"
        );
        assert_eq!(
            database
                .sources()
                .expect("sources should remain queryable after rollback")
                .into_iter()
                .find(|source| source.agent == "cursor")
                .expect("Cursor source status should remain visible")
                .status,
            "unavailable",
            "a failed publication must not leave the source stuck as indexing"
        );
    }

    #[test]
    fn clearing_local_data_removes_abandoned_database_snapshots_and_source_cursors() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let database = Database::open(temporary.path().join("vibemeter.sqlite"))
            .expect("database should open");
        database
            .set_setting("databaseHistoryRevision:keyed:test", "6.7\nrevision\nready")
            .expect("source cursor should write");
        let abandoned = std::env::temp_dir().join(format!(
            "vibemeter-database-history-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&abandoned).expect("abandoned snapshot should create");
        std::fs::write(abandoned.join("source.sqlite"), b"private source database")
            .expect("abandoned source snapshot should write");
        std::fs::write(abandoned.join(".active"), b"interrupted\n")
            .expect("abandoned lock marker should write");
        database
            .clear_local_data()
            .expect("local data should clear");
        assert!(!abandoned.exists());
        assert_eq!(
            database
                .setting("databaseHistoryRevision:keyed:test")
                .expect("source cursor should query"),
            None
        );
    }

    #[test]
    fn hermes_database_history_maps_observed_fields_and_degrades_without_data_loss() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let source_path = temporary.path().join("hermes-source.sqlite");
        let source = Connection::open(&source_path).expect("hermes source should open");
        source
            .execute_batch(
                "PRAGMA journal_mode=WAL;
                 PRAGMA wal_autocheckpoint=0;
                 CREATE TABLE sessions(
                    id TEXT PRIMARY KEY, model TEXT, started_at REAL, ended_at REAL,
                    message_count INTEGER, tool_call_count INTEGER,
                    input_tokens INTEGER, output_tokens INTEGER,
                    cache_read_tokens INTEGER, cache_write_tokens INTEGER,
                    reasoning_tokens INTEGER, estimated_cost_usd REAL, title TEXT
                 );
                 CREATE TABLE messages(
                    id INTEGER PRIMARY KEY, session_id TEXT, role TEXT, content TEXT,
                    tool_calls TEXT, timestamp REAL
                 );
                 CREATE TABLE account_usage(account_id TEXT, total_tokens INTEGER, secret TEXT);
                 INSERT INTO sessions VALUES(
                    'hermes-session-1','hermes-test',1775000000.0,1775000004.0,
                    3,1,60,40,5,3,2,0.25,'Hermes local work'
                 );
                 INSERT INTO messages VALUES
                    (1,'hermes-session-1','user','private prompt',NULL,1775000001.0),
                    (2,'hermes-session-1','assistant','private response',
                     '[{\"id\":\"tool-1\",\"type\":\"function\",\"function\":{\"name\":\"terminal\",\"arguments\":\"private command\"}}]',1775000002.0),
                    (3,'hermes-session-1','tool','private tool output',NULL,1775000003.0);
                 INSERT INTO account_usage VALUES('remote-account',999999,'sk-account-secret');",
            )
            .expect("hermes fixture should write");
        source
            .execute_batch("BEGIN EXCLUSIVE")
            .expect("source lock should start");
        let before = database_source_bytes(&source_path);
        let database = Database::open(temporary.path().join("vibemeter.sqlite"))
            .expect("database should open");

        assert!(
            index_database_source_at(&database, AgentKind::Hermes, &source_path, false)
                .expect("hermes database should index")
        );
        assert_eq!(database_source_bytes(&source_path), before);
        let sessions = database
            .sessions(
                "all",
                SessionListFilters {
                    agent: Some("hermes"),
                    ..SessionListFilters::default()
                },
                0,
                100,
            )
            .expect("hermes sessions should load");
        assert_eq!(sessions.total, 1);
        let summary = &sessions.items[0];
        assert_eq!(summary.usage.input_tokens, 60);
        assert_eq!(summary.usage.output_tokens, 40);
        assert_eq!(summary.usage.cache_read_tokens, 5);
        assert_eq!(summary.usage.cache_write_tokens, 3);
        assert_eq!(summary.usage.reasoning_tokens, 2);
        assert_eq!(summary.tool_calls, 1);
        let detail = database
            .session_detail(&summary.id)
            .expect("hermes detail should load");
        assert!(detail.tools.iter().any(|tool| tool.label == "other"));
        assert!(
            detail
                .phases
                .iter()
                .flat_map(|phase| &phase.events)
                .any(|event| event.name == "terminal")
        );
        assert!(
            detail
                .phases
                .iter()
                .any(|phase| phase.phase_key == "execute")
        );
        assert_eq!(
            detail
                .phases
                .iter()
                .flat_map(|phase| &phase.events)
                .filter(|event| event.event_type == "prompt.observed")
                .count(),
            1
        );
        source
            .execute_batch("ROLLBACK")
            .expect("source lock should end");

        let unsupported_path = temporary.path().join("unsupported-hermes.sqlite");
        let unsupported = Connection::open(&unsupported_path).expect("unsupported source");
        unsupported
            .execute_batch("CREATE TABLE unrelated(id INTEGER PRIMARY KEY, private TEXT);")
            .expect("unsupported fixture should write");
        drop(unsupported);
        assert!(
            !index_database_source_at(&database, AgentKind::Hermes, &unsupported_path, false,)
                .expect("unsupported schema should degrade safely")
        );
        assert_eq!(
            database
                .sources()
                .expect("sources should load")
                .into_iter()
                .find(|item| item.agent == "hermes")
                .expect("Hermes source status")
                .status,
            "unavailable"
        );
        assert_eq!(
            database
                .sessions(
                    "all",
                    SessionListFilters {
                        agent: Some("hermes"),
                        ..SessionListFilters::default()
                    },
                    0,
                    100,
                )
                .expect("existing Hermes data should remain")
                .total,
            1
        );
    }

    #[test]
    fn hermes_database_history_reports_partial_when_message_fields_are_unreadable() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let source_path = temporary.path().join("partial-hermes.sqlite");
        let source = Connection::open(&source_path).expect("Hermes source should open");
        source
            .execute_batch(
                "CREATE TABLE sessions(
                    id TEXT PRIMARY KEY, model TEXT, started_at REAL, ended_at REAL,
                    message_count INTEGER, input_tokens INTEGER, output_tokens INTEGER,
                    title TEXT
                 );
                 CREATE TABLE messages(id INTEGER PRIMARY KEY, session_id TEXT, content TEXT);
                 INSERT INTO sessions VALUES(
                    'partial-session','hermes-test',1775000000.0,1775000001.0,
                    1,4,2,'Partially readable Hermes session'
                 );
                 INSERT INTO messages VALUES(1,'partial-session','private prompt');",
            )
            .expect("partial Hermes fixture should write");
        drop(source);
        let database = Database::open(temporary.path().join("vibemeter.sqlite"))
            .expect("database should open");
        assert!(
            index_database_source_at(&database, AgentKind::Hermes, &source_path, false)
                .expect("partially readable Hermes source should retain session metadata")
        );
        let status = database
            .sources()
            .expect("sources should load")
            .into_iter()
            .find(|source| source.agent == "hermes")
            .expect("Hermes source status");
        assert_eq!(status.status, "partial");
        assert!(status.warning_count >= 1);
        let sessions = database
            .sessions(
                "all",
                SessionListFilters {
                    agent: Some("hermes"),
                    ..SessionListFilters::default()
                },
                0,
                100,
            )
            .expect("partial Hermes sessions should load");
        assert_eq!(sessions.total, 1);
        assert_eq!(sessions.items[0].usage.total(), 6);
        assert!(
            database
                .session_detail(&sessions.items[0].id)
                .expect("partial session detail should load")
                .phases
                .is_empty()
        );
    }

    #[test]
    fn hermes_database_history_rejects_oversized_tool_payloads_before_materializing_them() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let source_path = temporary.path().join("oversized-hermes.sqlite");
        let source = Connection::open(&source_path).expect("Hermes source should open");
        source
            .execute_batch(
                "CREATE TABLE sessions(
                    id TEXT PRIMARY KEY, model TEXT, started_at REAL, ended_at REAL,
                    message_count INTEGER, tool_call_count INTEGER, title TEXT
                 );
                 CREATE TABLE messages(
                    id INTEGER PRIMARY KEY, session_id TEXT, role TEXT,
                    tool_calls TEXT, timestamp REAL
                 );
                 INSERT INTO sessions VALUES(
                    'oversized-session','hermes-test',1775000000.0,1775000001.0,
                    1,1,'Oversized Hermes payload'
                 );
                 INSERT INTO messages
                 SELECT 1,'oversized-session','assistant',hex(zeroblob(524289)),1775000000.5;",
            )
            .expect("oversized Hermes fixture should write");
        drop(source);
        let database = Database::open(temporary.path().join("vibemeter.sqlite"))
            .expect("database should open");

        assert!(
            index_database_source_at(&database, AgentKind::Hermes, &source_path, false)
                .expect("oversized field should degrade without aborting the source")
        );
        let status = database
            .sources()
            .expect("sources should load")
            .into_iter()
            .find(|source| source.agent == "hermes")
            .expect("Hermes source status");
        assert_eq!(status.status, "partial");
        let sessions = database
            .sessions(
                "all",
                SessionListFilters {
                    agent: Some("hermes"),
                    ..SessionListFilters::default()
                },
                0,
                100,
            )
            .expect("bounded Hermes session should load");
        assert_eq!(sessions.total, 1);
        assert!(
            database
                .session_detail(&sessions.items[0].id)
                .expect("bounded Hermes detail should load")
                .phases
                .is_empty(),
            "an oversized private tool payload must not enter normalized evidence"
        );
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_database_history_reports_unavailable_without_local_rows() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let source_path = temporary.path().join("unreadable-cursor.sqlite");
        let source = Connection::open(&source_path).expect("source should open");
        source
            .execute_batch(
                "CREATE TABLE conversation_summaries(
                    conversationId TEXT PRIMARY KEY, updatedAt INTEGER NOT NULL
                 );
                 INSERT INTO conversation_summaries VALUES('session-1',1775000000000);",
            )
            .expect("source fixture should write");
        drop(source);
        let original_permissions = std::fs::metadata(&source_path)
            .expect("source metadata")
            .permissions();
        std::fs::set_permissions(&source_path, std::fs::Permissions::from_mode(0o000))
            .expect("source should become unreadable");
        let database = Database::open(temporary.path().join("vibemeter.sqlite"))
            .expect("database should open");
        let result = index_database_source_at(&database, AgentKind::Cursor, &source_path, false);
        std::fs::set_permissions(&source_path, original_permissions)
            .expect("source permissions should restore");
        assert!(!result.expect("permission failure should degrade safely"));
        let source_status = database
            .sources()
            .expect("sources should load")
            .into_iter()
            .find(|source| source.agent == "cursor")
            .expect("Cursor source status");
        assert!(source_status.available);
        assert_eq!(source_status.status, "unavailable");
        assert_eq!(source_status.warning_count, 1);
        assert_eq!(
            database
                .sessions(
                    "all",
                    SessionListFilters {
                        agent: Some("cursor"),
                        ..SessionListFilters::default()
                    },
                    0,
                    100,
                )
                .expect("sessions should remain queryable")
                .total,
            0
        );
    }

    #[test]
    #[ignore = "reads the current Mac's database history sources into an isolated database"]
    fn real_database_history_sources_remain_byte_identical() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let database = Database::open(temporary.path().join("vibemeter-audit.sqlite"))
            .expect("audit database should open");
        for (agent, path) in [
            (AgentKind::Cursor, cursor_database_path()),
            (AgentKind::Hermes, hermes_database_path()),
        ] {
            if !path.is_file() {
                continue;
            }
            let before = database_source_bytes(&path);
            assert!(
                index_database_source_at(&database, agent, &path, true)
                    .expect("real database source should index")
            );
            assert_eq!(database_source_bytes(&path), before);
            assert!(
                database
                    .sessions(
                        "all",
                        SessionListFilters {
                            agent: Some(agent.as_str()),
                            ..SessionListFilters::default()
                        },
                        0,
                        1,
                    )
                    .expect("audited sessions should load")
                    .total
                    > 0
            );
        }
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
