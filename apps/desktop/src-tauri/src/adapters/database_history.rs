use crate::errors::{AppError, AppResult};
use crate::models::{AgentKind, TokenUsage};
use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
#[cfg(test)]
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::LazyLock;
use std::sync::{Mutex, MutexGuard};

const SNAPSHOT_DIRECTORY_PREFIX: &str = "vibemeter-database-history-";
const SNAPSHOT_LOCK_FILE: &str = ".active";
const MAX_DATABASE_TEXT_BYTES: usize = 1024 * 1024;
static SNAPSHOT_GUARD: Mutex<()> = Mutex::new(());
#[cfg(test)]
static SNAPSHOT_CREATIONS: LazyLock<Mutex<HashMap<PathBuf, u64>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Clone)]
pub struct DatabaseHistoryEvent {
    pub occurred_at: Option<String>,
    pub event_type: String,
    pub category: String,
    pub name: String,
    pub source_event_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DatabaseHistorySession {
    pub source_session_id: String,
    pub title: Option<String>,
    pub model: Option<String>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub usage: TokenUsage,
    pub estimated_cost_usd: Option<f64>,
    pub declared_tool_calls: u64,
    pub events: Vec<DatabaseHistoryEvent>,
    pub malformed_records: u64,
    pub unknown_records: u64,
    pub source_revision: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DatabaseHistoryReadSummary {
    pub partial: bool,
    pub completed: bool,
}

pub struct DatabaseHistoryReader {
    agent: AgentKind,
    snapshot: SourceDatabaseSnapshot,
}

impl DatabaseHistoryReader {
    pub fn open(agent: AgentKind, path: &Path) -> AppResult<Self> {
        if !matches!(agent, AgentKind::Cursor | AgentKind::Hermes) {
            return Err(AppError::InvalidRequest(
                "unsupported database history source".into(),
            ));
        }
        Ok(Self {
            agent,
            snapshot: SourceDatabaseSnapshot::create(path)?,
        })
    }

    pub fn read_each(
        &self,
        mut visit: impl FnMut(DatabaseHistorySession) -> bool,
    ) -> AppResult<DatabaseHistoryReadSummary> {
        let connection = self
            .snapshot
            .connection
            .as_ref()
            .expect("database snapshot connection should remain available");
        match self.agent {
            AgentKind::Cursor => read_cursor(connection, &mut visit),
            AgentKind::Hermes => read_hermes(connection, &mut visit),
            _ => unreachable!("reader validates the supported database agents"),
        }
    }

    pub fn normalized_stage_path(&self) -> PathBuf {
        self.snapshot._directory.path.join("normalized.sqlite")
    }
}

pub fn source_revision(path: &Path) -> AppResult<String> {
    if !path.is_file() {
        return Err(AppError::InvalidRequest(
            "database history source is unavailable".into(),
        ));
    }
    verify_readable_artifacts(path)?;
    reject_hot_rollback_journal(path)?;
    let mut revision = Vec::new();
    for stamp in source_artifact_stamps(path)? {
        let label = if stamp.path == path {
            "main"
        } else if stamp.path == sqlite_sidecar(path, "-wal") {
            "wal"
        } else {
            "journal"
        };
        let modified = stamp
            .modified
            .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|value| value.as_nanos())
            .unwrap_or_default();
        revision.push(format!("{label}:{}:{modified}", stamp.length));
    }
    Ok(revision.join("|"))
}

#[cfg(test)]
pub fn snapshot_creation_count(path: &Path) -> u64 {
    SNAPSHOT_CREATIONS
        .lock()
        .ok()
        .and_then(|counts| counts.get(path).copied())
        .unwrap_or_default()
}

pub fn clear_snapshot_artifacts() -> AppResult<()> {
    with_snapshot_exclusion(|| Ok(()))
}

pub fn with_snapshot_exclusion<T>(operation: impl FnOnce() -> AppResult<T>) -> AppResult<T> {
    let _guard = SNAPSHOT_GUARD
        .lock()
        .map_err(|_| AppError::InvalidRequest("database snapshot lock was poisoned".into()))?;
    cleanup_abandoned_snapshots()?;
    operation()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceArtifactStamp {
    path: PathBuf,
    length: u64,
    modified: Option<std::time::SystemTime>,
}

struct SourceDatabaseSnapshot {
    connection: Option<Connection>,
    _directory: SnapshotDirectory,
    _guard: MutexGuard<'static, ()>,
}

struct SnapshotDirectory {
    path: PathBuf,
    _lock: Option<File>,
}

impl Drop for SnapshotDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

impl SourceDatabaseSnapshot {
    fn create(source_path: &Path) -> AppResult<Self> {
        let guard = SNAPSHOT_GUARD
            .lock()
            .map_err(|_| AppError::InvalidRequest("database snapshot lock was poisoned".into()))?;
        cleanup_abandoned_snapshots()?;
        if !source_path.is_file() {
            return Err(AppError::InvalidRequest(
                "database history source is unavailable".into(),
            ));
        }
        reject_hot_rollback_journal(source_path)?;
        verify_readable_artifacts(source_path)?;
        let artifacts_before = source_artifact_stamps(source_path)?;
        #[cfg(test)]
        if let Ok(mut counts) = SNAPSHOT_CREATIONS.lock() {
            let count = counts.entry(source_path.to_path_buf()).or_default();
            *count = count.saturating_add(1);
        }
        let directory_path = std::env::temp_dir().join(format!(
            "{SNAPSHOT_DIRECTORY_PREFIX}{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&directory_path)?;
        let mut directory = SnapshotDirectory {
            path: directory_path,
            _lock: None,
        };
        #[cfg(unix)]
        std::fs::set_permissions(&directory.path, std::fs::Permissions::from_mode(0o700))?;
        let lock_path = directory.path.join(SNAPSHOT_LOCK_FILE);
        let mut lock_options = OpenOptions::new();
        lock_options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        lock_options.mode(0o600);
        let mut lock = lock_options.open(&lock_path)?;
        lock.try_lock().map_err(|_| {
            AppError::InvalidRequest("database snapshot lock could not be acquired".into())
        })?;
        lock.write_all(b"active\n")?;
        lock.sync_all()?;
        directory._lock = Some(lock);
        let staged_path = directory.path.join("source.sqlite");
        copy_read_only_artifact(source_path, &staged_path)?;
        let source_wal = sqlite_sidecar(source_path, "-wal");
        if source_wal.is_file() {
            copy_read_only_artifact(&source_wal, &sqlite_sidecar(&staged_path, "-wal"))?;
        }
        if source_artifact_stamps(source_path)? != artifacts_before {
            return Err(AppError::InvalidRequest(
                "database history source changed during snapshot".into(),
            ));
        }
        let connection = Connection::open_with_flags(
            &staged_path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        connection.busy_timeout(std::time::Duration::from_millis(500))?;
        connection.pragma_update(None, "query_only", true)?;
        let integrity =
            connection.query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))?;
        if integrity != "ok" {
            return Err(AppError::InvalidRequest(
                "database history snapshot failed integrity validation".into(),
            ));
        }
        Ok(Self {
            connection: Some(connection),
            _directory: directory,
            _guard: guard,
        })
    }
}

impl Drop for SourceDatabaseSnapshot {
    fn drop(&mut self) {
        drop(self.connection.take());
    }
}

fn copy_read_only_artifact(source: &Path, destination: &Path) -> AppResult<()> {
    let mut input = File::open(source)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut output = options.open(destination)?;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        output.write_all(&buffer[..count])?;
    }
    output.sync_all()?;
    Ok(())
}

fn cleanup_abandoned_snapshots() -> AppResult<()> {
    let temporary_root = std::env::temp_dir();
    let entries = match std::fs::read_dir(&temporary_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    for entry in entries {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if !file_type.is_dir()
            || !entry
                .file_name()
                .to_string_lossy()
                .starts_with(SNAPSHOT_DIRECTORY_PREFIX)
        {
            continue;
        }
        let directory = entry.path();
        let lock_path = directory.join(SNAPSHOT_LOCK_FILE);
        let abandoned = if lock_path.is_file() {
            let lock = OpenOptions::new().read(true).write(true).open(&lock_path)?;
            lock.try_lock().is_ok()
        } else {
            true
        };
        if abandoned {
            std::fs::remove_dir_all(directory)?;
        }
    }
    Ok(())
}

fn reject_hot_rollback_journal(path: &Path) -> AppResult<()> {
    let rollback_journal = sqlite_sidecar(path, "-journal");
    if !rollback_journal.is_file() || std::fs::metadata(&rollback_journal)?.len() <= 512 {
        return Ok(());
    }
    let mut file = File::open(rollback_journal)?;
    let mut header = [0_u8; 8];
    let count = file.read(&mut header)?;
    if count == header.len() && header.iter().any(|byte| *byte != 0) {
        return Err(AppError::InvalidRequest(
            "database history source has an active rollback journal".into(),
        ));
    }
    Ok(())
}

fn verify_readable_artifacts(path: &Path) -> AppResult<()> {
    File::open(path)?;
    let wal = sqlite_sidecar(path, "-wal");
    if wal.is_file() {
        File::open(wal)?;
    }
    Ok(())
}

fn source_artifact_stamps(path: &Path) -> AppResult<Vec<SourceArtifactStamp>> {
    let mut artifacts = Vec::new();
    for candidate in [
        path.to_path_buf(),
        sqlite_sidecar(path, "-wal"),
        sqlite_sidecar(path, "-journal"),
    ] {
        if !candidate.is_file() {
            continue;
        }
        let metadata = std::fs::metadata(&candidate)?;
        artifacts.push(SourceArtifactStamp {
            path: candidate,
            length: metadata.len(),
            modified: metadata.modified().ok(),
        });
    }
    Ok(artifacts)
}

fn sqlite_sidecar(path: &Path, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{}{suffix}", path.to_string_lossy()))
}

fn read_cursor(
    connection: &Connection,
    visit: &mut impl FnMut(DatabaseHistorySession) -> bool,
) -> AppResult<DatabaseHistoryReadSummary> {
    let summaries = table_columns(connection, "conversation_summaries")?
        .filter(|columns| columns.contains("conversationId"));
    let hashes = table_columns(connection, "ai_code_hashes")?
        .filter(|columns| columns.contains("hash") && columns.contains("conversationId"));
    if summaries.is_none() && hashes.is_none() {
        return Err(AppError::InvalidRequest(
            "unsupported Cursor history schema".into(),
        ));
    }
    let mut query_parts = Vec::new();
    if let Some(columns) = summaries.as_ref() {
        query_parts.push(format!(
            "SELECT CAST(\"conversationId\" AS TEXT), 0, {}, {}, {}, NULL, NULL, NULL
             FROM conversation_summaries
             WHERE \"conversationId\" IS NOT NULL AND \"conversationId\" <> ''",
            text_column(columns, "title"),
            text_column(columns, "model"),
            text_column(columns, "updatedAt"),
        ));
    }
    if let Some(columns) = hashes.as_ref() {
        query_parts.push(format!(
            "SELECT CAST(\"conversationId\" AS TEXT), 1, NULL, {}, NULL, {}, {}, {}
             FROM ai_code_hashes
             WHERE \"conversationId\" IS NOT NULL AND \"conversationId\" <> ''",
            text_column(columns, "model"),
            text_column(columns, "timestamp"),
            text_column(columns, "createdAt"),
            text_column(columns, "hash"),
        ));
    }
    let query = format!("{} ORDER BY 1, 2, 6, 7, 8", query_parts.join(" UNION ALL "));
    let mut partial = false;
    let mut current = None::<DatabaseHistorySession>;
    let mut statement = connection.prepare(&query)?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, Option<String>>(7)?,
        ))
    })?;
    for row in rows {
        let (session_id, row_kind, title, model, updated_at, timestamp, created_at, event_id) =
            row?;
        if current
            .as_ref()
            .is_some_and(|session| session.source_session_id != session_id)
        {
            let mut session = current.take().expect("current Cursor session should exist");
            session.events.sort_by(|left, right| {
                left.occurred_at
                    .cmp(&right.occurred_at)
                    .then_with(|| left.source_event_id.cmp(&right.source_event_id))
            });
            partial |= session.malformed_records > 0 || session.unknown_records > 0;
            if !visit(session) {
                return Ok(DatabaseHistoryReadSummary {
                    partial,
                    completed: false,
                });
            }
        }
        let session = current.get_or_insert_with(|| empty_session(session_id));
        if row_kind == 0 {
            session.title = title;
            session.model = safe_model(model.as_deref());
            session.ended_at = updated_at
                .as_deref()
                .and_then(|value| value.parse::<i64>().ok())
                .and_then(timestamp_from_millis);
            session.source_revision = updated_at;
            continue;
        }
        let Some(event_id) = event_id.filter(|value| !value.is_empty()) else {
            session.unknown_records = session.unknown_records.saturating_add(1);
            continue;
        };
        let timestamp = timestamp
            .or(created_at)
            .and_then(|value| value.parse::<i64>().ok())
            .filter(|value| *value > 0);
        if let Some(model) = safe_model(model.as_deref()) {
            session.model = Some(model);
        }
        if timestamp.is_some_and(|timestamp| {
            session
                .source_revision
                .as_deref()
                .and_then(|value| value.parse::<i64>().ok())
                .is_none_or(|current| timestamp > current)
        }) {
            session.source_revision = timestamp.map(|value| value.to_string());
        }
        if session.events.len() < crate::adapters::common::MAX_EVENTS_PER_SESSION {
            session.events.push(DatabaseHistoryEvent {
                occurred_at: timestamp.and_then(timestamp_from_millis),
                event_type: "activity".into(),
                category: "edit".into(),
                name: "code-change".into(),
                source_event_id: Some(event_id),
            });
        } else {
            session.unknown_records = session.unknown_records.saturating_add(1);
        }
    }
    if let Some(mut session) = current {
        session.events.sort_by(|left, right| {
            left.occurred_at
                .cmp(&right.occurred_at)
                .then_with(|| left.source_event_id.cmp(&right.source_event_id))
        });
        partial |= session.malformed_records > 0 || session.unknown_records > 0;
        if !visit(session) {
            return Ok(DatabaseHistoryReadSummary {
                partial,
                completed: false,
            });
        }
    }
    Ok(DatabaseHistoryReadSummary {
        partial,
        completed: true,
    })
}

fn read_hermes(
    connection: &Connection,
    visit: &mut impl FnMut(DatabaseHistorySession) -> bool,
) -> AppResult<DatabaseHistoryReadSummary> {
    let session_columns = table_columns(connection, "sessions")?
        .ok_or_else(|| AppError::InvalidRequest("unsupported Hermes history schema".into()))?;
    require_columns(&session_columns, &["id"])?;
    let message_columns = table_columns(connection, "messages")?;
    let messages_readable = message_columns.as_ref().is_some_and(|columns| {
        ["id", "session_id", "role"]
            .iter()
            .all(|column| columns.contains(*column))
    });
    let session_select = [
        "id",
        "model",
        "started_at",
        "ended_at",
        "message_count",
        "tool_call_count",
        "input_tokens",
        "output_tokens",
        "cache_read_tokens",
        "cache_write_tokens",
        "reasoning_tokens",
        "estimated_cost_usd",
        "actual_cost_usd",
        "cost_status",
        "title",
    ]
    .into_iter()
    .map(|column| qualified_text_column(&session_columns, "s", column))
    .collect::<Vec<_>>()
    .join(", ");
    let query = if let Some(columns) = message_columns.as_ref().filter(|_| messages_readable) {
        let bounded_tool_calls = if columns.contains("tool_calls") {
            format!(
                "CASE WHEN length(m.\"tool_calls\") <= {MAX_DATABASE_TEXT_BYTES}
                 THEN CAST(m.\"tool_calls\" AS TEXT) ELSE NULL END"
            )
        } else {
            "NULL".into()
        };
        let oversized_tool_calls = if columns.contains("tool_calls") {
            format!(
                "CASE WHEN length(m.\"tool_calls\") > {MAX_DATABASE_TEXT_BYTES} THEN 1 ELSE 0 END"
            )
        } else {
            "0".into()
        };
        format!(
            "SELECT {session_select}, {}, {}, {bounded_tool_calls}, {}, {oversized_tool_calls}
             FROM sessions s LEFT JOIN messages m ON m.\"session_id\"=s.\"id\"
             ORDER BY s.\"id\" ASC, {} ASC, {} ASC",
            qualified_text_column(columns, "m", "id"),
            qualified_text_column(columns, "m", "role"),
            qualified_text_column(columns, "m", "timestamp"),
            qualified_raw_column(columns, "m", "timestamp"),
            qualified_raw_column(columns, "m", "id"),
        )
    } else {
        format!(
            "SELECT {session_select}, NULL, NULL, NULL, NULL, 0
             FROM sessions s ORDER BY s.\"id\" ASC"
        )
    };
    let mut partial = false;
    let mut current = None::<DatabaseHistorySession>;
    let mut statement = connection.prepare(&query)?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, Option<String>>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, Option<String>>(8)?,
            row.get::<_, Option<String>>(9)?,
            row.get::<_, Option<String>>(10)?,
            row.get::<_, Option<String>>(11)?,
            row.get::<_, Option<String>>(12)?,
            row.get::<_, Option<String>>(13)?,
            row.get::<_, Option<String>>(14)?,
            row.get::<_, Option<String>>(15)?,
            row.get::<_, Option<String>>(16)?,
            row.get::<_, Option<String>>(17)?,
            row.get::<_, Option<String>>(18)?,
            row.get::<_, i64>(19)?,
        ))
    })?;
    for row in rows {
        let (
            id,
            model,
            started_at,
            ended_at,
            message_count,
            tool_call_count,
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_write_tokens,
            reasoning_tokens,
            estimated_cost_usd,
            actual_cost_usd,
            cost_status,
            title,
            message_id,
            role,
            tool_calls,
            message_timestamp,
            oversized_tool_calls,
        ) = row?;
        let Some(id) = id.filter(|value| !value.is_empty()) else {
            continue;
        };
        if current
            .as_ref()
            .is_some_and(|session| session.source_session_id != id)
        {
            let mut session = current.take().expect("current Hermes session should exist");
            session.events.sort_by(|left, right| {
                left.occurred_at
                    .cmp(&right.occurred_at)
                    .then_with(|| left.source_event_id.cmp(&right.source_event_id))
            });
            partial |= session.malformed_records > 0 || session.unknown_records > 0;
            if !visit(session) {
                return Ok(DatabaseHistoryReadSummary {
                    partial,
                    completed: false,
                });
            }
        }
        let session = current.get_or_insert_with(|| {
            let mut session = empty_session(id);
            session.title = title;
            session.model = safe_model(model.as_deref());
            session.started_at = parse_f64(started_at.as_deref()).and_then(timestamp_from_seconds);
            session.ended_at = parse_f64(ended_at.as_deref()).and_then(timestamp_from_seconds);
            session.usage = TokenUsage {
                input_tokens: parse_u64(input_tokens.as_deref()),
                output_tokens: parse_u64(output_tokens.as_deref()),
                cache_read_tokens: parse_u64(cache_read_tokens.as_deref()),
                cache_write_tokens: parse_u64(cache_write_tokens.as_deref()),
                cache_write_1h_tokens: 0,
                reasoning_tokens: parse_u64(reasoning_tokens.as_deref()),
            };
            session.estimated_cost_usd = match cost_status
                .as_deref()
                .map(str::trim)
                .map(str::to_ascii_lowercase)
                .as_deref()
            {
                Some("estimated") => parse_nonnegative_f64(estimated_cost_usd.as_deref()),
                Some("actual" | "complete" | "known") => parse_nonnegative_f64(
                    actual_cost_usd.as_deref().or(estimated_cost_usd.as_deref()),
                ),
                _ => None,
            };
            session.declared_tool_calls = parse_u64(tool_call_count.as_deref());
            session.source_revision = message_count;
            if !messages_readable {
                session.unknown_records = session.unknown_records.saturating_add(1);
            }
            session
        });
        if messages_readable {
            record_hermes_message(
                session,
                message_id,
                role,
                tool_calls,
                message_timestamp,
                oversized_tool_calls != 0,
            );
        }
    }
    if let Some(mut session) = current {
        session.events.sort_by(|left, right| {
            left.occurred_at
                .cmp(&right.occurred_at)
                .then_with(|| left.source_event_id.cmp(&right.source_event_id))
        });
        partial |= session.malformed_records > 0 || session.unknown_records > 0;
        if !visit(session) {
            return Ok(DatabaseHistoryReadSummary {
                partial,
                completed: false,
            });
        }
    }
    Ok(DatabaseHistoryReadSummary {
        partial,
        completed: true,
    })
}

fn record_hermes_message(
    session: &mut DatabaseHistorySession,
    message_id: Option<String>,
    role: Option<String>,
    tool_calls: Option<String>,
    timestamp: Option<String>,
    oversized_tool_calls: bool,
) {
    if message_id.is_none() && role.is_none() && tool_calls.is_none() && timestamp.is_none() {
        return;
    }
    if session.events.len() >= crate::adapters::common::MAX_EVENTS_PER_SESSION {
        session.unknown_records = session.unknown_records.saturating_add(1);
        return;
    }
    if oversized_tool_calls {
        session.malformed_records = session.malformed_records.saturating_add(1);
    }
    let occurred_at = parse_f64(timestamp.as_deref()).and_then(timestamp_from_seconds);
    let message_id = message_id.unwrap_or_else(|| {
        format!(
            "message-{}",
            session
                .events
                .len()
                .saturating_add(session.unknown_records as usize)
        )
    });
    match role.as_deref() {
        Some("user") => session.events.push(DatabaseHistoryEvent {
            occurred_at,
            event_type: "prompt".into(),
            category: "understand".into(),
            name: "user".into(),
            source_event_id: Some(message_id),
        }),
        Some("tool") => session.events.push(DatabaseHistoryEvent {
            occurred_at,
            event_type: "activity".into(),
            category: "execute".into(),
            name: "tool-result".into(),
            source_event_id: Some(message_id),
        }),
        Some("assistant") => {
            if let Some(tool_calls) = tool_calls {
                match serde_json::from_str::<Value>(&tool_calls) {
                    Ok(Value::Array(calls)) => {
                        for (index, call) in calls.iter().enumerate() {
                            if session.events.len()
                                >= crate::adapters::common::MAX_EVENTS_PER_SESSION
                            {
                                session.unknown_records = session.unknown_records.saturating_add(1);
                                break;
                            }
                            let name = call
                                .get("function")
                                .and_then(|value| value.get("name"))
                                .or_else(|| call.get("name"))
                                .and_then(Value::as_str)
                                .unwrap_or("tool")
                                .to_string();
                            let source_event_id = call
                                .get("id")
                                .and_then(Value::as_str)
                                .map(ToString::to_string)
                                .unwrap_or_else(|| format!("{message_id}:{index}"));
                            session.events.push(DatabaseHistoryEvent {
                                occurred_at: occurred_at.clone(),
                                event_type: "tool".into(),
                                category: "execute".into(),
                                name,
                                source_event_id: Some(source_event_id),
                            });
                        }
                    }
                    Ok(_) => session.unknown_records = session.unknown_records.saturating_add(1),
                    Err(_) => {
                        session.malformed_records = session.malformed_records.saturating_add(1)
                    }
                }
            }
        }
        _ => session.unknown_records = session.unknown_records.saturating_add(1),
    }
}

fn empty_session(source_session_id: String) -> DatabaseHistorySession {
    DatabaseHistorySession {
        source_session_id,
        title: None,
        model: None,
        started_at: None,
        ended_at: None,
        usage: TokenUsage::default(),
        estimated_cost_usd: None,
        declared_tool_calls: 0,
        events: Vec::new(),
        malformed_records: 0,
        unknown_records: 0,
        source_revision: None,
    }
}

fn table_columns(connection: &Connection, table: &str) -> AppResult<Option<HashSet<String>>> {
    let exists = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        [table],
        |row| row.get::<_, i64>(0),
    )? != 0;
    if !exists {
        return Ok(None);
    }
    let mut statement = connection.prepare("SELECT name FROM pragma_table_info(?1)")?;
    let columns = statement
        .query_map([table], |row| row.get::<_, String>(0))?
        .collect::<Result<HashSet<_>, _>>()?;
    Ok(Some(columns))
}

fn require_columns(columns: &HashSet<String>, required: &[&str]) -> AppResult<()> {
    if required.iter().all(|column| columns.contains(*column)) {
        Ok(())
    } else {
        Err(AppError::InvalidRequest(
            "database history schema is missing required fields".into(),
        ))
    }
}

fn text_column(columns: &HashSet<String>, name: &str) -> String {
    if columns.contains(name) {
        format!("CAST(\"{name}\" AS TEXT)")
    } else {
        "NULL".into()
    }
}

fn qualified_text_column(columns: &HashSet<String>, table: &str, name: &str) -> String {
    if columns.contains(name) {
        format!("CAST({table}.\"{name}\" AS TEXT)")
    } else {
        "NULL".into()
    }
}

fn qualified_raw_column(columns: &HashSet<String>, table: &str, name: &str) -> String {
    if columns.contains(name) {
        format!("{table}.\"{name}\"")
    } else {
        "NULL".into()
    }
}

fn parse_u64(value: Option<&str>) -> u64 {
    value
        .and_then(|value| value.parse::<i128>().ok())
        .unwrap_or_default()
        .max(0)
        .min(u64::MAX as i128) as u64
}

fn parse_f64(value: Option<&str>) -> Option<f64> {
    value.and_then(|value| value.parse::<f64>().ok())
}

fn parse_nonnegative_f64(value: Option<&str>) -> Option<f64> {
    parse_f64(value).filter(|value| value.is_finite() && *value >= 0.0)
}

fn safe_model(value: Option<&str>) -> Option<String> {
    let value = value.map(str::trim).filter(|value| {
        !value.is_empty()
            && value.len() <= 120
            && value.chars().all(|character| {
                character.is_ascii_alphanumeric()
                    || matches!(character, '-' | '_' | '.' | '/' | ':')
            })
    })?;
    let lower = value.to_ascii_lowercase();
    if value.starts_with(['/', '\\'])
        || value.contains("..")
        || value.contains("\\")
        || value.contains("://")
        || lower.starts_with("sk-")
        || lower.contains("api_key")
        || lower.contains("api-key")
        || lower.contains("authorization:")
        || crate::privacy::sanitize_title(value).as_deref() != Some(value)
    {
        return None;
    }
    Some(value.to_string())
}

fn timestamp_from_millis(value: i64) -> Option<String> {
    DateTime::<Utc>::from_timestamp_millis(value)
        .map(|time| time.to_rfc3339_opts(SecondsFormat::Millis, true))
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

    #[test]
    fn model_names_keep_public_provider_ids_and_reject_private_values() {
        assert_eq!(
            safe_model(Some("anthropic/claude-sonnet-4")),
            Some("anthropic/claude-sonnet-4".into())
        );
        assert_eq!(safe_model(Some("sk-super-secret-token")), None);
        assert_eq!(safe_model(Some("/Users/alice/private/model")), None);
        assert_eq!(safe_model(Some("person@example.com")), None);
    }
}
