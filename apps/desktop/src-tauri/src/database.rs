use crate::errors::{AppError, AppResult};
use crate::models::{
    AgentKind, BehaviorSignals, BehaviorSummary, CanonicalEvent, ComparisonItem, CoverageNotice,
    DailyUsagePoint, DistributionItem, EvidenceReference, FileChange, GitCommitEvidence,
    GitEvidence, GitFileStat, HourlyUsagePoint, IndexStatus, InsightItem, InsightStat,
    InsightsResponse, LiveActivityResponse, LiveConcurrencyLane, LiveHistoryItem, LiveSession,
    LiveTimelinePoint, NotchClearResult, NotchCompletedSession, ObservedLiveEvent,
    OverviewResponse, OverviewTotals, PARSER_VERSION, ParseState, PhraseAgentCount, PhraseCloud,
    PhraseCloudItem, PhraseCloudResponse, PhraseLegendItem, PhraseModelCount, PlaybookItem,
    ProcessPhase, ProjectControl, Provenance, SavePlaybookRequest, SessionDetail,
    SessionListFilters, SessionSummary, SessionsResponse, SkillUsageItem, SkillUsageSummary,
    SourceStatus, TaskSummary, TokenUsage, VctiProfile,
};
use crate::source_capabilities::{SourceLiveCapability, source_capabilities, source_capability};
use chrono::{DateTime, Duration, Local, SecondsFormat, Utc};
use rusqlite::{Connection, MAIN_DB, OpenFlags, OptionalExtension, Transaction, params};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

type RangeUsageActivity = (
    TokenUsage,
    Option<f64>,
    Vec<DailyUsagePoint>,
    Vec<HourlyUsagePoint>,
);

const MIGRATION_V1: &str = r#"
CREATE TABLE IF NOT EXISTS sources (
    id TEXT PRIMARY KEY,
    agent TEXT NOT NULL,
    path_hash TEXT NOT NULL,
    capability_level TEXT NOT NULL,
    available INTEGER NOT NULL DEFAULT 1,
    session_count INTEGER NOT NULL DEFAULT 0,
    warning_count INTEGER NOT NULL DEFAULT 0,
    last_indexed_at TEXT,
    status TEXT NOT NULL DEFAULT 'ready'
);

CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    source_session_id TEXT NOT NULL,
    agent TEXT NOT NULL,
    model TEXT,
    title TEXT,
    project_hash TEXT,
    started_at TEXT NOT NULL,
    ended_at TEXT,
    active_seconds INTEGER NOT NULL DEFAULT 0,
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0,
    cache_write_tokens INTEGER NOT NULL DEFAULT 0,
    cache_write_1h_tokens INTEGER NOT NULL DEFAULT 0,
    reasoning_tokens INTEGER NOT NULL DEFAULT 0,
    estimated_cost_usd REAL,
    cost_coverage_tokens INTEGER NOT NULL DEFAULT 0,
    tool_calls INTEGER NOT NULL DEFAULT 0,
    files_touched INTEGER NOT NULL DEFAULT 0,
    lines_added INTEGER NOT NULL DEFAULT 0,
    lines_deleted INTEGER NOT NULL DEFAULT 0,
    errors INTEGER NOT NULL DEFAULT 0,
    retries INTEGER NOT NULL DEFAULT 0,
    verification_events INTEGER NOT NULL DEFAULT 0,
    human_interventions INTEGER NOT NULL DEFAULT 0,
    subagent_count INTEGER NOT NULL DEFAULT 0,
    longest_uninterrupted_seconds INTEGER NOT NULL DEFAULT 0,
    event_count INTEGER NOT NULL DEFAULT 0,
    parser_version TEXT NOT NULL,
    source_file_hash TEXT NOT NULL UNIQUE,
    source_size INTEGER NOT NULL,
    source_mtime INTEGER NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS sessions_started_idx ON sessions(started_at DESC);
CREATE INDEX IF NOT EXISTS sessions_agent_idx ON sessions(agent, started_at DESC);
CREATE INDEX IF NOT EXISTS sessions_model_idx ON sessions(model, started_at DESC);

CREATE TABLE IF NOT EXISTS daily_usage (
    session_id TEXT NOT NULL,
    date TEXT NOT NULL,
    agent TEXT NOT NULL,
    model TEXT NOT NULL,
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0,
    cache_write_tokens INTEGER NOT NULL DEFAULT 0,
    cache_write_1h_tokens INTEGER NOT NULL DEFAULT 0,
    reasoning_tokens INTEGER NOT NULL DEFAULT 0,
    active_seconds INTEGER NOT NULL DEFAULT 0,
    events INTEGER NOT NULL DEFAULT 0,
    tool_calls INTEGER NOT NULL DEFAULT 0,
    errors INTEGER NOT NULL DEFAULT 0,
    verification_events INTEGER NOT NULL DEFAULT 0,
    estimated_cost_usd REAL,
    PRIMARY KEY(session_id, date, model),
    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS daily_usage_date_idx ON daily_usage(date, agent);

CREATE TABLE IF NOT EXISTS tool_usage (
    session_id TEXT NOT NULL,
    tool TEXT NOT NULL,
    count INTEGER NOT NULL,
    PRIMARY KEY(session_id, tool),
    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS session_files (
    session_id TEXT NOT NULL,
    file_hash TEXT NOT NULL,
    PRIMARY KEY(session_id, file_hash),
    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS ingestion_cursors (
    source_file_hash TEXT PRIMARY KEY,
    agent TEXT NOT NULL,
    source_size INTEGER NOT NULL,
    source_mtime INTEGER NOT NULL,
    byte_offset INTEGER NOT NULL,
    state_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS parser_warnings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_file_hash TEXT NOT NULL,
    agent TEXT NOT NULL,
    warning_code TEXT NOT NULL,
    count INTEGER NOT NULL DEFAULT 1,
    updated_at TEXT NOT NULL,
    UNIQUE(source_file_hash, warning_code)
);

CREATE TABLE IF NOT EXISTS app_settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS share_exports (
    id TEXT PRIMARY KEY,
    template_id TEXT NOT NULL,
    locale TEXT NOT NULL,
    aspect_ratio TEXT NOT NULL,
    format TEXT NOT NULL,
    model_hash TEXT NOT NULL,
    created_at TEXT NOT NULL
);

PRAGMA user_version = 1;
"#;

const MIGRATION_V2: &str = r#"
CREATE TABLE IF NOT EXISTS hourly_usage (
    session_id TEXT NOT NULL,
    hour TEXT NOT NULL,
    agent TEXT NOT NULL,
    model TEXT NOT NULL,
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0,
    cache_write_tokens INTEGER NOT NULL DEFAULT 0,
    cache_write_1h_tokens INTEGER NOT NULL DEFAULT 0,
    reasoning_tokens INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(session_id, hour, model),
    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS hourly_usage_hour_idx ON hourly_usage(hour, agent);
PRAGMA user_version = 2;
"#;

const MIGRATION_V3: &str = r#"
ALTER TABLE sessions ADD COLUMN project_label TEXT;
ALTER TABLE sessions ADD COLUMN prompt_excerpt TEXT;
ALTER TABLE sessions ADD COLUMN model_switches INTEGER NOT NULL DEFAULT 0;

CREATE TABLE events (
    session_id TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    occurred_at TEXT,
    event_type TEXT NOT NULL,
    category TEXT NOT NULL,
    name TEXT NOT NULL,
    success INTEGER,
    duration_ms INTEGER,
    provenance TEXT NOT NULL,
    PRIMARY KEY(session_id, sequence),
    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
);
CREATE INDEX events_session_time_idx ON events(session_id, occurred_at, sequence);
CREATE INDEX events_type_time_idx ON events(event_type, occurred_at);

CREATE TABLE file_changes (
    session_id TEXT NOT NULL,
    path TEXT NOT NULL,
    change_kind TEXT NOT NULL,
    lines_added INTEGER NOT NULL DEFAULT 0,
    lines_deleted INTEGER NOT NULL DEFAULT 0,
    modification_count INTEGER NOT NULL DEFAULT 0,
    first_observed_at TEXT,
    last_observed_at TEXT,
    final_state TEXT NOT NULL DEFAULT 'observed',
    PRIMARY KEY(session_id, path),
    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
);
CREATE INDEX file_changes_path_idx ON file_changes(path);

CREATE TABLE git_evidence (
    session_id TEXT PRIMARY KEY,
    available INTEGER NOT NULL DEFAULT 0,
    state TEXT NOT NULL,
    branch TEXT,
    inspected_at TEXT NOT NULL,
    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
);
CREATE TABLE git_commits (
    session_id TEXT NOT NULL,
    hash TEXT NOT NULL,
    subject TEXT NOT NULL,
    committed_at TEXT NOT NULL,
    PRIMARY KEY(session_id, hash),
    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
);
CREATE TABLE git_files (
    session_id TEXT NOT NULL,
    commit_hash TEXT NOT NULL,
    path TEXT NOT NULL,
    lines_added INTEGER NOT NULL DEFAULT 0,
    lines_deleted INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(session_id, commit_hash, path),
    FOREIGN KEY(session_id, commit_hash) REFERENCES git_commits(session_id, hash) ON DELETE CASCADE
);

CREATE TABLE tasks (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    project_label TEXT NOT NULL,
    status TEXT NOT NULL,
    confidence REAL NOT NULL,
    user_edited INTEGER NOT NULL DEFAULT 0,
    source_excluded INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX tasks_project_time_idx ON tasks(project_label, updated_at DESC);
CREATE TABLE task_sessions (
    task_id TEXT NOT NULL,
    session_id TEXT NOT NULL UNIQUE,
    position INTEGER NOT NULL DEFAULT 0,
    user_assigned INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(task_id, session_id),
    FOREIGN KEY(task_id) REFERENCES tasks(id) ON DELETE CASCADE,
    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE TABLE reviews (
    id TEXT PRIMARY KEY,
    review_type TEXT NOT NULL,
    target_id TEXT NOT NULL,
    locale TEXT NOT NULL,
    version INTEGER NOT NULL,
    status TEXT NOT NULL,
    title TEXT NOT NULL,
    outcome TEXT NOT NULL,
    what_happened TEXT NOT NULL,
    what_worked TEXT NOT NULL,
    friction TEXT NOT NULL,
    lessons TEXT NOT NULL,
    next_run TEXT NOT NULL,
    user_edited INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(review_type, target_id, locale, version)
);
CREATE INDEX reviews_target_idx ON reviews(review_type, target_id, locale, version DESC);
CREATE TABLE review_findings (
    review_id TEXT NOT NULL,
    id TEXT NOT NULL,
    rule_id TEXT NOT NULL,
    tier TEXT NOT NULL,
    title TEXT NOT NULL,
    detail TEXT NOT NULL,
    evidence_json TEXT NOT NULL,
    PRIMARY KEY(review_id, id),
    FOREIGN KEY(review_id) REFERENCES reviews(id) ON DELETE CASCADE
);

CREATE TABLE playbook_items (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    category TEXT NOT NULL,
    project_label TEXT,
    task_type TEXT,
    source_review_id TEXT,
    source_finding_id TEXT,
    source_excluded INTEGER NOT NULL DEFAULT 0,
    applied INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX playbook_search_idx ON playbook_items(category, project_label, updated_at DESC);

CREATE TABLE excluded_projects (
    project_hash TEXT PRIMARY KEY,
    project_label TEXT NOT NULL,
    excluded_at TEXT NOT NULL
);

PRAGMA user_version = 3;
"#;

const MIGRATION_V4: &str = r#"
ALTER TABLE reviews ADD COLUMN source_excluded INTEGER NOT NULL DEFAULT 0;
PRAGMA user_version = 4;
"#;

const MIGRATION_V5: &str = r#"
ALTER TABLE sessions ADD COLUMN result_excerpt TEXT;
ALTER TABLE tasks ADD COLUMN grouping_state TEXT NOT NULL DEFAULT 'separate';
ALTER TABLE tasks ADD COLUMN grouping_reason_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE tasks ADD COLUMN suggested_task_id TEXT;

DELETE FROM task_sessions WHERE user_assigned=0;
DELETE FROM tasks WHERE user_edited=0
  AND NOT EXISTS(SELECT 1 FROM task_sessions ts WHERE ts.task_id=tasks.id);

PRAGMA user_version = 5;
"#;

const MIGRATION_V6: &str = r#"
CREATE TABLE IF NOT EXISTS session_behavior (
    session_id TEXT PRIMARY KEY,
    behavior_json TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS vcti_profile_snapshots (
    period_end TEXT NOT NULL,
    algorithm_version TEXT NOT NULL,
    profile_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY(period_end, algorithm_version)
);

PRAGMA user_version = 6;
"#;

const MIGRATION_V7: &str = r#"
CREATE TABLE IF NOT EXISTS phrase_usage (
    session_id TEXT NOT NULL,
    date TEXT NOT NULL,
    role TEXT NOT NULL,
    agent TEXT NOT NULL,
    phrase TEXT NOT NULL,
    occurrences INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(session_id, date, role, agent, phrase),
    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS phrase_usage_range_idx
    ON phrase_usage(date, role, phrase);
CREATE INDEX IF NOT EXISTS phrase_usage_agent_idx
    ON phrase_usage(agent, date);

PRAGMA user_version = 7;
"#;

const MIGRATION_V8: &str = r#"
CREATE TABLE IF NOT EXISTS live_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    received_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    agent TEXT NOT NULL,
    source_session_id TEXT NOT NULL,
    event_name TEXT NOT NULL,
    project_label TEXT NOT NULL DEFAULT '',
    payload_json TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS live_events_expiry_idx ON live_events(expires_at);
CREATE INDEX IF NOT EXISTS live_events_session_idx
    ON live_events(agent, source_session_id, received_at);

CREATE TABLE IF NOT EXISTS live_session_metrics (
    agent TEXT NOT NULL,
    source_session_id TEXT NOT NULL,
    started_at TEXT NOT NULL,
    last_seen_at TEXT NOT NULL,
    event_count INTEGER NOT NULL DEFAULT 0,
    waiting_count INTEGER NOT NULL DEFAULT 0,
    error_count INTEGER NOT NULL DEFAULT 0,
    completion_count INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(agent, source_session_id)
);

PRAGMA user_version = 8;
"#;

const MIGRATION_V9: &str = r#"
ALTER TABLE sources ADD COLUMN selected INTEGER NOT NULL DEFAULT 1;
PRAGMA user_version = 9;
"#;

const MIGRATION_V10: &str = r#"
ALTER TABLE live_events ADD COLUMN status TEXT NOT NULL DEFAULT 'running';
CREATE INDEX IF NOT EXISTS live_events_status_idx ON live_events(status, received_at);
PRAGMA user_version = 10;
"#;

const MIGRATION_V11: &str = r#"
CREATE TABLE IF NOT EXISTS notch_session_history (
    id TEXT PRIMARY KEY,
    session_json TEXT NOT NULL,
    cycle_started_at TEXT NOT NULL,
    seen_at TEXT NOT NULL,
    completed_at TEXT,
    status TEXT NOT NULL,
    cleared_at TEXT
);
CREATE INDEX IF NOT EXISTS notch_session_history_completed_idx
    ON notch_session_history(status, completed_at DESC);
CREATE INDEX IF NOT EXISTS notch_session_history_seen_idx
    ON notch_session_history(status, seen_at);
PRAGMA user_version = 11;
"#;

const MIGRATION_V12: &str = r#"
ALTER TABLE notch_session_history ADD COLUMN jump_context_json TEXT;
PRAGMA user_version = 12;
"#;

const MIGRATION_V13: &str = r#"
CREATE TABLE IF NOT EXISTS skill_usage (
    session_id TEXT NOT NULL,
    skill TEXT NOT NULL,
    count INTEGER NOT NULL,
    PRIMARY KEY(session_id, skill),
    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS skill_usage_skill_idx ON skill_usage(skill);
PRAGMA user_version = 13;
"#;

const CANONICAL_EVENT_PROTOCOL_VERSION: &str = "1.0.0";
const CANONICAL_EVENT_SCHEMA_VERSION: i64 = 14;
const LIVE_NORMALIZER_VERSION: &str = "live-normalizer-1.0.0";
const DATABASE_SCHEMA_VERSION: i64 = 14;
const WAITING_REPLAY_WINDOW_SECONDS: i64 = 30;

const MIGRATION_V14: &str = r#"
BEGIN IMMEDIATE;
ALTER TABLE live_events ADD COLUMN canonical_event_id TEXT;

CREATE TABLE canonical_events (
    id TEXT PRIMARY KEY,
    source_event_id TEXT,
    event_fingerprint TEXT NOT NULL,
    dedup_key TEXT NOT NULL UNIQUE,
    protocol_version TEXT NOT NULL,
    schema_version INTEGER NOT NULL,
    algorithm_version TEXT NOT NULL,
    occurred_at TEXT NOT NULL,
    observed_at TEXT NOT NULL,
    source TEXT NOT NULL,
    agent TEXT NOT NULL,
    source_session_id TEXT NOT NULL,
    activity_cycle_id TEXT,
    work_unit_id TEXT,
    parent_session_id TEXT,
    relation_type TEXT,
    lifecycle_status TEXT NOT NULL,
    live_phase TEXT,
    event_type TEXT NOT NULL,
    source_event_name TEXT NOT NULL,
    process_phase TEXT,
    evidence_level TEXT NOT NULL,
    source_coverage TEXT NOT NULL,
    privacy_level TEXT NOT NULL,
    project_label TEXT NOT NULL DEFAULT '',
    deleted_at TEXT
);
CREATE INDEX canonical_events_occurred_idx
    ON canonical_events(occurred_at DESC);
CREATE INDEX canonical_events_status_idx
    ON canonical_events(lifecycle_status, occurred_at DESC);
CREATE INDEX canonical_events_session_idx
    ON canonical_events(agent, source_session_id, occurred_at DESC);
CREATE INDEX live_events_canonical_idx ON live_events(canonical_event_id);
PRAGMA user_version = 14;
COMMIT;
"#;

#[derive(Clone)]
pub struct Database {
    connection: Arc<Mutex<Connection>>,
}

#[derive(Debug)]
pub struct CursorRecord {
    pub source_size: u64,
    pub source_mtime: i64,
    pub byte_offset: u64,
    pub state: ParseState,
}

#[derive(Debug)]
struct CanonicalWaitingEvent {
    id: String,
    source_event_id: Option<String>,
    event_fingerprint: String,
    dedup_key: String,
    occurred_at: String,
    observed_at: String,
    agent: String,
    source_session_id: String,
    live_phase: String,
    source_event_name: String,
    project_label: String,
}

fn canonical_waiting_event(event: &ObservedLiveEvent) -> Option<CanonicalWaitingEvent> {
    let exact_source = source_capabilities().iter().any(|capability| {
        capability.agent == event.agent && capability.live_capability == SourceLiveCapability::Exact
    });
    if !exact_source || event.status != "waiting" {
        return None;
    }
    let occurred_at = DateTime::parse_from_rfc3339(&event.occurred_at)
        .ok()?
        .with_timezone(&Utc)
        .to_rfc3339_opts(SecondsFormat::AutoSi, true);
    let observed_at = DateTime::parse_from_rfc3339(&event.observed_at)
        .ok()?
        .with_timezone(&Utc)
        .to_rfc3339_opts(SecondsFormat::AutoSi, true);

    let source_event_name = match event.event_name.as_str() {
        "PermissionRequest" => "PermissionRequest",
        _ => "Waiting",
    };
    let live_phase = match event.phase.as_deref() {
        Some("needs-you") => "needs-you",
        _ => "needs-you",
    };
    let project_label = if event.project_label.contains(['/', '\\']) {
        format!(
            "private-{}",
            crate::privacy::stable_hash(&event.project_label)
        )
    } else {
        event.project_label.chars().take(80).collect()
    };
    let source_fingerprint = event.source_event_fingerprint.clone().unwrap_or_else(|| {
        crate::privacy::stable_hash(&format!(
            "{}|{}|{}|{}|{}|{}",
            event.agent,
            event.source_session_id,
            source_event_name,
            occurred_at,
            event.status,
            live_phase,
        ))
    });
    let event_fingerprint = crate::privacy::stable_hash(&format!(
        "{}|{}|{}|{}",
        event.agent, event.source_session_id, source_event_name, source_fingerprint,
    ));
    let dedup_identity = event
        .source_event_id
        .as_deref()
        .map(|source_event_id| {
            format!(
                "source|{}|{}|{}",
                event.agent, event.source_session_id, source_event_id
            )
        })
        .unwrap_or_else(|| format!("episode|{event_fingerprint}|{observed_at}"));
    let dedup_key = crate::privacy::stable_hash(&dedup_identity);

    Some(CanonicalWaitingEvent {
        id: format!("waiting-{dedup_key}"),
        source_event_id: event
            .source_event_id
            .as_deref()
            .map(crate::privacy::stable_hash),
        event_fingerprint,
        dedup_key,
        occurred_at,
        observed_at,
        agent: event.agent.clone(),
        source_session_id: crate::privacy::safe_opaque_identifier(&event.source_session_id),
        live_phase: live_phase.into(),
        source_event_name: source_event_name.into(),
        project_label,
    })
}

fn resolve_waiting_episode(
    transaction: &Transaction<'_>,
    event: &ObservedLiveEvent,
    canonical: &mut CanonicalWaitingEvent,
) -> AppResult<()> {
    if event.source_event_id.is_some() {
        return Ok(());
    }
    let latest = transaction
        .query_row(
            "SELECT le.id, le.status, le.received_at,
                    ce.id, ce.dedup_key, ce.event_fingerprint
             FROM live_events le
             LEFT JOIN canonical_events ce ON ce.id=le.canonical_event_id
             WHERE le.agent=?1 AND le.source_session_id=?2
             ORDER BY le.id DESC
             LIMIT 1",
            params![event.agent, event.source_session_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            },
        )
        .optional()?;

    if let Some((_, status, received_at, Some(id), Some(dedup_key), Some(fingerprint))) = &latest
        && status == "waiting"
        && fingerprint == &canonical.event_fingerprint
        && DateTime::parse_from_rfc3339(&canonical.observed_at)
            .ok()
            .zip(DateTime::parse_from_rfc3339(received_at).ok())
            .is_some_and(|(observed, previous)| {
                observed
                    .signed_duration_since(previous)
                    .num_seconds()
                    .unsigned_abs()
                    <= WAITING_REPLAY_WINDOW_SECONDS as u64
            })
    {
        canonical.id = id.clone();
        canonical.dedup_key = dedup_key.clone();
        return Ok(());
    }

    let previous_raw_id = latest.map(|value| value.0).unwrap_or_default();
    canonical.dedup_key = crate::privacy::stable_hash(&format!(
        "episode|{}|{}|{previous_raw_id}",
        canonical.event_fingerprint, canonical.observed_at,
    ));
    canonical.id = format!("waiting-{}", canonical.dedup_key);
    Ok(())
}

fn apply_schema_migrations(connection: &Connection, version: i64) -> AppResult<()> {
    for (target_version, migration) in [
        (1, MIGRATION_V1),
        (2, MIGRATION_V2),
        (3, MIGRATION_V3),
        (4, MIGRATION_V4),
        (5, MIGRATION_V5),
        (6, MIGRATION_V6),
        (7, MIGRATION_V7),
        (8, MIGRATION_V8),
        (9, MIGRATION_V9),
        (10, MIGRATION_V10),
        (11, MIGRATION_V11),
        (12, MIGRATION_V12),
        (13, MIGRATION_V13),
        (14, MIGRATION_V14),
    ] {
        if version < target_version {
            connection.execute_batch(migration)?;
        }
    }
    Ok(())
}

fn database_version(path: &Path) -> AppResult<i64> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(connection.query_row("PRAGMA user_version", [], |row| row.get(0))?)
}

fn sqlite_sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

fn migration_artifact(path: &Path, label: &str) -> PathBuf {
    path.with_extension(format!("sqlite.{label}"))
}

fn migration_paths(path: &Path) -> (PathBuf, PathBuf, PathBuf) {
    (
        migration_artifact(path, "schema-migrating"),
        migration_artifact(path, "schema-rollback"),
        migration_artifact(path, "schema-migration"),
    )
}

fn rollback_copy_path(rollback: &Path) -> PathBuf {
    sqlite_sidecar(rollback, "-copying")
}

fn remove_file_if_exists(path: &Path) -> AppResult<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn remove_database_artifact(path: &Path) -> AppResult<()> {
    remove_file_if_exists(&sqlite_sidecar(path, "-wal"))?;
    remove_file_if_exists(&sqlite_sidecar(path, "-shm"))?;
    remove_file_if_exists(path)
}

fn database_header_version(path: &Path) -> AppResult<i64> {
    let mut file = std::fs::File::open(path)?;
    let mut header = [0_u8; 64];
    file.read_exact(&mut header)?;
    if &header[..16] != b"SQLite format 3\0" {
        return Err(AppError::InvalidRequest(
            "database header is not a supported SQLite file".into(),
        ));
    }
    Ok(u32::from_be_bytes(
        header[60..64]
            .try_into()
            .map_err(|_| AppError::InvalidRequest("database header is incomplete".into()))?,
    ) as i64)
}

fn validate_v14_connection(connection: &Connection) -> AppResult<()> {
    let quick_check: String = connection.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    let canonical_columns: i64 = connection.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('canonical_events')
         WHERE name IN (
            'id', 'dedup_key', 'protocol_version', 'schema_version',
            'occurred_at', 'observed_at', 'evidence_level',
            'source_coverage', 'privacy_level'
         )",
        [],
        |row| row.get(0),
    )?;
    let live_link_columns: i64 = connection.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('live_events')
         WHERE name='canonical_event_id'",
        [],
        |row| row.get(0),
    )?;
    if quick_check != "ok"
        || version != DATABASE_SCHEMA_VERSION
        || canonical_columns != 9
        || live_link_columns != 1
    {
        return Err(AppError::InvalidRequest(
            "database migration did not pass version and schema verification".into(),
        ));
    }
    Ok(())
}

fn validate_v14_database(path: &Path) -> AppResult<()> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    validate_v14_connection(&connection)
}

fn validate_legacy_database(path: &Path) -> AppResult<()> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let quick_check: String = connection.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if quick_check != "ok" || version >= DATABASE_SCHEMA_VERSION {
        return Err(AppError::InvalidRequest(
            "database rollback artifact did not pass integrity and version verification".into(),
        ));
    }
    Ok(())
}

fn create_rollback_artifact(path: &Path, rollback: &Path) -> AppResult<()> {
    if std::fs::hard_link(path, rollback).is_ok() {
        return validate_legacy_database(rollback);
    }

    let copying = rollback_copy_path(rollback);
    let source_size = std::fs::metadata(path)?.len();
    std::fs::copy(path, &copying)?;
    std::fs::File::open(&copying)?.sync_all()?;
    if std::fs::metadata(&copying)?.len() != source_size {
        return Err(AppError::InvalidRequest(
            "database rollback copy is incomplete".into(),
        ));
    }
    validate_legacy_database(&copying)?;
    std::fs::rename(copying, rollback)?;
    Ok(())
}

fn archive_source_sidecars(path: &Path, rollback: &Path) -> AppResult<()> {
    for suffix in ["-wal", "-shm"] {
        let original = sqlite_sidecar(path, suffix);
        if original.exists() {
            std::fs::rename(original, sqlite_sidecar(rollback, suffix))?;
        }
    }
    Ok(())
}

fn restore_rollback_sidecars(path: &Path, rollback: &Path) -> AppResult<()> {
    for suffix in ["-wal", "-shm"] {
        let backup = sqlite_sidecar(rollback, suffix);
        if !backup.exists() {
            continue;
        }
        let original = sqlite_sidecar(path, suffix);
        remove_file_if_exists(&original)?;
        std::fs::rename(backup, original)?;
    }
    Ok(())
}

fn restore_database(path: &Path, rollback: &Path) -> AppResult<()> {
    if !rollback.exists() {
        return Err(AppError::InvalidRequest(
            "database rollback artifact is unavailable".into(),
        ));
    }
    validate_legacy_database(rollback)?;
    std::fs::rename(rollback, path)?;
    restore_rollback_sidecars(path, rollback)?;
    validate_legacy_database(path)
}

fn cleanup_migration_artifacts(staging: &Path, rollback: &Path, marker: &Path) -> AppResult<()> {
    remove_database_artifact(staging)?;
    remove_database_artifact(rollback)?;
    remove_database_artifact(&rollback_copy_path(rollback))?;
    remove_file_if_exists(marker)?;
    Ok(())
}

fn recover_schema_migration_state(path: &Path) -> AppResult<()> {
    let (staging, rollback, marker) = migration_paths(path);
    let installed = path.exists()
        && database_header_version(path).is_ok_and(|version| version == DATABASE_SCHEMA_VERSION);
    if installed {
        for suffix in ["-wal", "-shm"] {
            let original = sqlite_sidecar(path, suffix);
            if !original.exists() {
                continue;
            }
            let backup = sqlite_sidecar(&rollback, suffix);
            if rollback.exists() && !backup.exists() {
                std::fs::rename(&original, backup)?;
            } else {
                remove_file_if_exists(&original)?;
            }
        }
        if validate_v14_database(path).is_ok() {
            return cleanup_migration_artifacts(&staging, &rollback, &marker);
        }
    }

    let legacy_installed = path.exists()
        && database_header_version(path).is_ok_and(|version| version < DATABASE_SCHEMA_VERSION);
    if legacy_installed {
        restore_rollback_sidecars(path, &rollback)?;
        if validate_legacy_database(path).is_ok() {
            return cleanup_migration_artifacts(&staging, &rollback, &marker);
        }
    }

    if rollback.exists() {
        restore_database(path, &rollback)?;
    } else if !path.exists() {
        return Err(AppError::InvalidRequest(
            "interrupted database migration cannot be recovered".into(),
        ));
    }
    cleanup_migration_artifacts(&staging, &rollback, &marker)
}

fn recover_interrupted_schema_migration(path: &Path) -> AppResult<()> {
    let (_, _, marker) = migration_paths(path);
    if !marker.exists() {
        return Ok(());
    }
    let marker_lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&marker)?;
    marker_lock.try_lock().map_err(|_| {
        AppError::InvalidRequest("database migration is already in progress".into())
    })?;
    recover_schema_migration_state(path)
}

fn create_migration_marker(marker: &Path) -> AppResult<std::fs::File> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(marker)?;
    file.try_lock().map_err(|_| {
        AppError::InvalidRequest("database migration marker could not be locked".into())
    })?;
    file.write_all(b"v14\n")?;
    file.sync_all()?;
    Ok(file)
}

fn migrate_schema_on_copy(path: &Path) -> AppResult<()> {
    let (staging, rollback, marker) = migration_paths(path);
    if staging.exists() || rollback.exists() || marker.exists() {
        return Err(AppError::InvalidRequest(
            "database migration artifacts require recovery".into(),
        ));
    }
    let _marker_lock = create_migration_marker(&marker)?;
    let mut installed_validated = false;
    let migration_result = (|| -> AppResult<()> {
        let migration_lock = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        migration_lock.busy_timeout(std::time::Duration::from_secs(10))?;
        migration_lock.execute_batch("BEGIN IMMEDIATE")?;

        let source = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        source.busy_timeout(std::time::Duration::from_secs(10))?;
        source.backup(MAIN_DB, &staging, None)?;
        drop(source);

        let staged = Connection::open(&staging)?;
        staged.busy_timeout(std::time::Duration::from_secs(10))?;
        let version: i64 = staged.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        apply_schema_migrations(&staged, version)?;
        staged.pragma_update(None, "journal_mode", "DELETE")?;
        validate_v14_connection(&staged)?;
        drop(staged);

        create_rollback_artifact(path, &rollback)?;
        std::fs::rename(&staging, path)?;
        migration_lock.execute_batch("COMMIT")?;
        drop(migration_lock);
        archive_source_sidecars(path, &rollback)?;
        validate_v14_database(path)?;
        installed_validated = true;
        cleanup_migration_artifacts(&staging, &rollback, &marker)
    })();

    if migration_result.is_err() && !installed_validated {
        recover_schema_migration_state(path)?;
    }
    migration_result
}

impl Database {
    pub fn open(path: PathBuf) -> AppResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        recover_interrupted_schema_migration(&path)?;
        if path.is_file() && database_version(&path)? < DATABASE_SCHEMA_VERSION {
            migrate_schema_on_copy(&path)?;
        }
        let connection = Connection::open(path)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        apply_schema_migrations(&connection, version)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    fn connect(&self) -> AppResult<MutexGuard<'_, Connection>> {
        self.connection
            .lock()
            .map_err(|_| AppError::InvalidRequest("database connection lock was poisoned".into()))
    }

    pub fn load_cursor(&self, file_hash: &str) -> AppResult<Option<CursorRecord>> {
        let connection = self.connect()?;
        let row = connection
            .query_row(
                "SELECT source_size, source_mtime, byte_offset, state_json
                 FROM ingestion_cursors WHERE source_file_hash = ?1",
                params![file_hash],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()?;
        row.map(|(size, mtime, offset, state_json)| {
            Ok(CursorRecord {
                source_size: size.max(0) as u64,
                source_mtime: mtime,
                byte_offset: offset.max(0) as u64,
                state: serde_json::from_str(&state_json)?,
            })
        })
        .transpose()
    }

    pub fn persist_parse_state(
        &self,
        file_hash: &str,
        source_size: u64,
        source_mtime: i64,
        byte_offset: u64,
        state: &ParseState,
    ) -> AppResult<String> {
        let mut persisted_state = state.clone();
        crate::phrases::compact(&mut persisted_state);
        let state = &persisted_state;
        let mut connection = self.connect()?;
        let transaction = connection.transaction()?;
        let derived_session_id = session_database_id(state.agent, &state.source_session_id);
        // The source file is the durable identity. Older builds briefly indexed Kimi
        // wire logs through the Claude adapter, so changing the agent also changed the
        // derived session id and collided with the unique source_file_hash constraint.
        // Reuse the existing opaque id for that file and update it in place: child
        // evidence and user task/review links stay intact while the agent is corrected.
        let existing_file_session_id = transaction
            .query_row(
                "SELECT id FROM sessions WHERE source_file_hash=?1",
                params![file_hash],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let session_id = if let Some(existing) = existing_file_session_id {
            existing
        } else {
            let derived_owner = transaction
                .query_row(
                    "SELECT source_file_hash FROM sessions WHERE id=?1",
                    params![derived_session_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            if derived_owner
                .as_deref()
                .is_some_and(|owner| owner != file_hash)
            {
                source_file_session_database_id(state.agent, &state.source_session_id, file_hash)
            } else {
                derived_session_id
            }
        };
        let now = Utc::now().to_rfc3339();
        let excluded = state.project_hash.as_deref().is_some_and(|project_hash| {
            transaction
                .query_row(
                    "SELECT 1 FROM excluded_projects WHERE project_hash=?1",
                    params![project_hash],
                    |_| Ok(()),
                )
                .optional()
                .ok()
                .flatten()
                .is_some()
        });
        if excluded {
            transaction.execute("DELETE FROM sessions WHERE id=?1", params![session_id])?;
            transaction.execute(
                "INSERT INTO ingestion_cursors(
                    source_file_hash, agent, source_size, source_mtime, byte_offset,
                    state_json, updated_at
                 ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(source_file_hash) DO UPDATE SET
                    agent=excluded.agent, source_size=excluded.source_size,
                    source_mtime=excluded.source_mtime, byte_offset=excluded.byte_offset,
                    state_json=excluded.state_json, updated_at=excluded.updated_at",
                params![
                    file_hash,
                    state.agent.as_str(),
                    sql_i64(source_size),
                    source_mtime,
                    sql_i64(byte_offset),
                    serde_json::to_string(state)?,
                    now,
                ],
            )?;
            transaction.commit()?;
            return Ok(session_id);
        }
        let started_at = state
            .started_at
            .as_deref()
            .or(state.ended_at.as_deref())
            .unwrap_or(&now);
        let total_tokens = state.usage.total().max(1);
        let cost = if state.cost_coverage_tokens > 0 {
            Some(state.estimated_cost_usd)
        } else {
            None
        };

        transaction.execute(
            "INSERT INTO sessions (
                id, source_session_id, agent, model, title, project_hash,
                started_at, ended_at, active_seconds, input_tokens, output_tokens,
                cache_read_tokens, cache_write_tokens, cache_write_1h_tokens,
                reasoning_tokens, estimated_cost_usd, cost_coverage_tokens,
                tool_calls, files_touched, lines_added, lines_deleted, errors,
                retries, verification_events, human_interventions, subagent_count,
                longest_uninterrupted_seconds, event_count, parser_version,
                source_file_hash, source_size, source_mtime, updated_at
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24,
                ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33
             )
             ON CONFLICT(id) DO UPDATE SET
                source_session_id=excluded.source_session_id,
                agent=excluded.agent,
                model=excluded.model,
                title=excluded.title,
                project_hash=excluded.project_hash,
                started_at=excluded.started_at,
                ended_at=excluded.ended_at,
                active_seconds=excluded.active_seconds,
                input_tokens=excluded.input_tokens,
                output_tokens=excluded.output_tokens,
                cache_read_tokens=excluded.cache_read_tokens,
                cache_write_tokens=excluded.cache_write_tokens,
                cache_write_1h_tokens=excluded.cache_write_1h_tokens,
                reasoning_tokens=excluded.reasoning_tokens,
                estimated_cost_usd=excluded.estimated_cost_usd,
                cost_coverage_tokens=excluded.cost_coverage_tokens,
                tool_calls=excluded.tool_calls,
                files_touched=excluded.files_touched,
                lines_added=excluded.lines_added,
                lines_deleted=excluded.lines_deleted,
                errors=excluded.errors,
                retries=excluded.retries,
                verification_events=excluded.verification_events,
                human_interventions=excluded.human_interventions,
                subagent_count=excluded.subagent_count,
                longest_uninterrupted_seconds=excluded.longest_uninterrupted_seconds,
                event_count=excluded.event_count,
                parser_version=excluded.parser_version,
                source_file_hash=excluded.source_file_hash,
                source_size=excluded.source_size,
                source_mtime=excluded.source_mtime,
                updated_at=excluded.updated_at",
            params![
                session_id,
                state.source_session_id,
                state.agent.as_str(),
                state.primary_model(),
                state.title,
                state.project_hash,
                started_at,
                state.ended_at,
                sql_i64(state.active_seconds),
                sql_i64(state.usage.input_tokens),
                sql_i64(state.usage.output_tokens),
                sql_i64(state.usage.cache_read_tokens),
                sql_i64(state.usage.cache_write_tokens),
                sql_i64(state.usage.cache_write_1h_tokens),
                sql_i64(state.usage.reasoning_tokens),
                cost,
                sql_i64(state.cost_coverage_tokens.min(total_tokens)),
                sql_i64(state.tool_calls),
                sql_i64(state.touched_file_hashes.len() as u64),
                sql_i64(state.lines_added),
                sql_i64(state.lines_deleted),
                sql_i64(state.errors),
                sql_i64(state.retries),
                sql_i64(state.verification_events),
                sql_i64(state.human_interventions),
                sql_i64(state.subagent_count),
                sql_i64(state.longest_uninterrupted_seconds),
                sql_i64(state.event_count),
                state.parser_version,
                file_hash,
                sql_i64(source_size),
                source_mtime,
                now,
            ],
        )?;

        transaction.execute(
            "UPDATE sessions SET project_label=?1, prompt_excerpt=?2, result_excerpt=?3,
                model_switches=?4 WHERE id=?5",
            params![
                state.project_label,
                state.prompt_excerpt,
                state.result_excerpt,
                sql_i64(state.model_switches),
                session_id,
            ],
        )?;
        transaction.execute(
            "INSERT INTO session_behavior(session_id, behavior_json, updated_at)
             VALUES(?1, ?2, ?3)
             ON CONFLICT(session_id) DO UPDATE SET
                behavior_json=excluded.behavior_json,
                updated_at=excluded.updated_at",
            params![session_id, serde_json::to_string(&state.behavior)?, now],
        )?;

        replace_session_children(&transaction, &session_id, state)?;
        upsert_derived_task(&transaction, &session_id, state, &now)?;
        transaction.execute(
            "INSERT INTO ingestion_cursors (
                source_file_hash, agent, source_size, source_mtime, byte_offset,
                state_json, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(source_file_hash) DO UPDATE SET
                agent=excluded.agent,
                source_size=excluded.source_size,
                source_mtime=excluded.source_mtime,
                byte_offset=excluded.byte_offset,
                state_json=excluded.state_json,
                updated_at=excluded.updated_at",
            params![
                file_hash,
                state.agent.as_str(),
                sql_i64(source_size),
                source_mtime,
                sql_i64(byte_offset),
                serde_json::to_string(state)?,
                now,
            ],
        )?;
        upsert_warning_rows(&transaction, file_hash, state, &now)?;
        transaction.commit()?;
        Ok(session_id)
    }

    pub fn upsert_source(
        &self,
        agent: AgentKind,
        path_hash: &str,
        available: bool,
        status: &str,
    ) -> AppResult<()> {
        let connection = self.connect()?;
        let now = Utc::now().to_rfc3339();
        let capability_level = source_capability(agent).history_capability.as_str();
        connection.execute(
            "INSERT INTO sources (
                id, agent, path_hash, capability_level, available,
                session_count, warning_count, last_indexed_at, status
             ) VALUES (?1, ?2, ?3, ?4, ?5,
                (SELECT COUNT(*) FROM sessions WHERE agent=?2),
                (SELECT COALESCE(SUM(count), 0) FROM parser_warnings WHERE agent=?2),
                ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET
                path_hash=excluded.path_hash,
                capability_level=excluded.capability_level,
                available=excluded.available,
                session_count=(SELECT COUNT(*) FROM sessions WHERE agent=?2),
                warning_count=(SELECT COALESCE(SUM(count), 0) FROM parser_warnings WHERE agent=?2),
                last_indexed_at=excluded.last_indexed_at,
                status=excluded.status",
            params![
                agent.as_str(),
                agent.as_str(),
                path_hash,
                capability_level,
                available,
                now,
                status
            ],
        )?;
        Ok(())
    }

    pub fn setting(&self, key: &str) -> AppResult<Option<String>> {
        let connection = self.connect()?;
        Ok(connection
            .query_row(
                "SELECT value FROM app_settings WHERE key=?1",
                params![key],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> AppResult<()> {
        let connection = self.connect()?;
        connection.execute(
            "INSERT INTO app_settings(key, value, updated_at) VALUES(?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at",
            params![key, value, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn set_source_selected(&self, agent: &str, selected: bool) -> AppResult<()> {
        if !source_capabilities()
            .iter()
            .any(|capability| capability.agent == agent)
        {
            return Err(AppError::InvalidRequest("unknown data source".into()));
        }
        let connection = self.connect()?;
        let changed = connection.execute(
            "UPDATE sources SET selected=?1 WHERE agent=?2 AND available=1",
            params![selected, agent],
        )?;
        if changed == 0 {
            return Err(AppError::InvalidRequest(
                "data source is not available on this Mac".into(),
            ));
        }
        Ok(())
    }

    pub fn prune_evidence(&self) -> AppResult<()> {
        let connection = self.connect()?;
        let days = connection
            .query_row(
                "SELECT value FROM app_settings WHERE key='retentionDays'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(365);
        if days <= 0 {
            return Ok(());
        }
        let cutoff = (Utc::now() - Duration::days(days)).to_rfc3339();
        connection.execute(
            "DELETE FROM events WHERE (occurred_at IS NOT NULL AND occurred_at<?1)
             OR session_id IN(
                SELECT id FROM sessions WHERE COALESCE(ended_at,started_at)<?1
             )",
            params![cutoff],
        )?;
        connection.execute(
            "DELETE FROM file_changes WHERE session_id IN(
                SELECT id FROM sessions WHERE COALESCE(ended_at,started_at)<?1
             )",
            params![cutoff],
        )?;
        connection.execute(
            "DELETE FROM git_files WHERE session_id IN(
                SELECT id FROM sessions WHERE COALESCE(ended_at,started_at)<?1
             )",
            params![cutoff],
        )?;
        connection.execute(
            "DELETE FROM git_commits WHERE session_id IN(
                SELECT id FROM sessions WHERE COALESCE(ended_at,started_at)<?1
             )",
            params![cutoff],
        )?;
        connection.execute(
            "DELETE FROM git_evidence WHERE session_id IN(
                SELECT id FROM sessions WHERE COALESCE(ended_at,started_at)<?1
             )",
            params![cutoff],
        )?;
        connection.execute(
            "UPDATE sessions SET prompt_excerpt=NULL, result_excerpt=NULL
             WHERE COALESCE(ended_at,started_at)<?1",
            params![cutoff],
        )?;
        Ok(())
    }

    pub fn projects(&self) -> AppResult<Vec<ProjectControl>> {
        let connection = self.connect()?;
        let mut statement = connection.prepare(
            "SELECT s.project_hash, COALESCE(MAX(s.project_label), ''), COUNT(*),
                    EXISTS(SELECT 1 FROM excluded_projects ep WHERE ep.project_hash=s.project_hash)
             FROM sessions s WHERE s.project_hash IS NOT NULL
             GROUP BY s.project_hash ORDER BY 2",
        )?;
        let mut items = statement
            .query_map([], |row| {
                let project_hash: String = row.get(0)?;
                let label: String = row.get(1)?;
                Ok(ProjectControl {
                    project_hash: project_hash.clone(),
                    project_label: if label.is_empty() {
                        project_hash.chars().take(6).collect()
                    } else {
                        label
                    },
                    session_count: read_u64(row, 2)?,
                    excluded: row.get::<_, i64>(3)? != 0,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut excluded_statement = connection.prepare(
            "SELECT project_hash, project_label FROM excluded_projects ORDER BY project_label",
        )?;
        for item in excluded_statement
            .query_map([], |row| {
                Ok(ProjectControl {
                    project_hash: row.get(0)?,
                    project_label: row.get(1)?,
                    session_count: 0,
                    excluded: true,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?
        {
            if !items
                .iter()
                .any(|existing| existing.project_hash == item.project_hash)
            {
                items.push(item);
            }
        }
        Ok(items)
    }

    pub fn exclude_project(&self, project_hash: &str) -> AppResult<()> {
        let mut connection = self.connect()?;
        let transaction = connection.transaction()?;
        let project_label = transaction
            .query_row(
                "SELECT COALESCE(project_label, '') FROM sessions
                 WHERE project_hash=?1 ORDER BY started_at DESC LIMIT 1",
                params![project_hash],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .unwrap_or_else(|| project_hash.chars().take(6).collect());
        transaction.execute(
            "INSERT INTO excluded_projects(project_hash, project_label, excluded_at)
             VALUES(?1, ?2, ?3)
             ON CONFLICT(project_hash) DO UPDATE SET
                project_label=excluded.project_label, excluded_at=excluded.excluded_at",
            params![project_hash, project_label, Utc::now().to_rfc3339()],
        )?;
        let affected_reviews = "(
            (review_type='session' AND target_id IN(
                SELECT id FROM sessions WHERE project_hash=?1
            )) OR
            (review_type='daily' AND target_id IN(
                SELECT DISTINCT substr(started_at,1,10) FROM sessions WHERE project_hash=?1
            )) OR
            (review_type='weekly' AND EXISTS(
                SELECT 1 FROM sessions s WHERE s.project_hash=?1
                AND substr(s.started_at,1,10)>=reviews.target_id
                AND substr(s.started_at,1,10)<date(reviews.target_id,'+7 days')
            ))
        )";
        transaction.execute(
            &format!(
                "UPDATE playbook_items SET source_excluded=1, updated_at=?2
                WHERE source_review_id IN(SELECT id FROM reviews WHERE {affected_reviews})
                OR project_label=?3"
            ),
            params![project_hash, Utc::now().to_rfc3339(), project_label],
        )?;
        transaction.execute(
            &format!(
                "UPDATE reviews SET source_excluded=1, updated_at=?2
                WHERE user_edited=1 AND {affected_reviews}"
            ),
            params![project_hash, Utc::now().to_rfc3339()],
        )?;
        transaction.execute(
            &format!("DELETE FROM reviews WHERE user_edited=0 AND {affected_reviews}"),
            params![project_hash],
        )?;
        transaction.execute(
            "UPDATE tasks SET source_excluded=1, updated_at=?2
             WHERE project_label=?1 AND user_edited=1",
            params![project_label, Utc::now().to_rfc3339()],
        )?;
        transaction.execute(
            "DELETE FROM tasks WHERE project_label=?1 AND user_edited=0",
            params![project_label],
        )?;
        transaction.execute(
            "DELETE FROM sessions WHERE project_hash=?1",
            params![project_hash],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn include_project(&self, project_hash: &str) -> AppResult<()> {
        let connection = self.connect()?;
        connection.execute(
            "DELETE FROM excluded_projects WHERE project_hash=?1",
            params![project_hash],
        )?;
        Ok(())
    }

    pub fn clear_local_data(&self) -> AppResult<()> {
        let connection = self.connect()?;
        connection.execute_batch(
            "DELETE FROM reviews;
             DELETE FROM playbook_items;
             DELETE FROM tasks;
             DELETE FROM sessions;
             DELETE FROM ingestion_cursors;
             DELETE FROM parser_warnings;
             DELETE FROM sources;
             DELETE FROM share_exports;
             DELETE FROM vcti_profile_snapshots;
             DELETE FROM live_events;
             DELETE FROM canonical_events;
             DELETE FROM live_session_metrics;
             DELETE FROM excluded_projects;",
        )?;
        Ok(())
    }

    pub fn overview(&self, range: &str, index_status: IndexStatus) -> AppResult<OverviewResponse> {
        let connection = self.connect()?;
        let start_date = range_start(range);
        let start_timestamp = format!("{start_date}T00:00:00Z");
        let totals = query_overview_totals(&connection, &start_timestamp, &start_date)?;
        let daily = query_daily(&connection, &start_date)?;
        let hourly = query_hourly(&connection, &start_date)?;
        let agents = query_usage_distribution(&connection, "agent", "agent", &start_date)?;
        let models = query_usage_distribution(&connection, "model", "model", &start_date)?;
        let tools = query_tools(&connection, &start_timestamp)?;
        let skills = query_skills(&connection, &start_timestamp)?;
        let behavior = query_behavior_summary(&connection, &start_timestamp)?;
        let recent_sessions = query_session_rows(
            &connection,
            &start_timestamp,
            &SessionListFilters::default(),
            0,
            8,
        )?
        .0;
        let warning_count = connection.query_row(
            "SELECT COALESCE(SUM(count), 0) FROM parser_warnings
             WHERE NOT EXISTS(SELECT 1 FROM sources)
                OR agent IN (
                    SELECT agent FROM sources WHERE available=1 AND selected=1
                )",
            [],
            |row| read_u64(row, 0),
        )?;
        let mut coverage = Vec::new();
        if totals.cost_coverage < 0.999 && totals.usage.total() > 0 {
            coverage.push(CoverageNotice {
                id: "partial-cost".into(),
                level: "info".into(),
                message_key: "coverage.partialCost".into(),
                agent: None,
                value: Some(totals.cost_coverage),
            });
        }
        if warning_count > 0 {
            coverage.push(CoverageNotice {
                id: "parser-warnings".into(),
                level: "warning".into(),
                message_key: "coverage.parserWarnings".into(),
                agent: None,
                value: Some(warning_count as f64),
            });
        }
        Ok(OverviewResponse {
            range: range.into(),
            generated_at: Utc::now().to_rfc3339(),
            pricing_version: crate::pricing::PRICING_VERSION.into(),
            totals,
            daily,
            hourly,
            agents,
            models,
            tools,
            skills,
            behavior,
            recent_sessions,
            coverage,
            index_status,
        })
    }

    pub fn phrase_cloud(&self, range: &str) -> AppResult<PhraseCloudResponse> {
        let connection = self.connect()?;
        let start_date = range_start(range);
        let user = query_phrase_cloud(&connection, &start_date, "user")?;
        let agents = query_phrase_cloud(&connection, &start_date, "agent")?;
        let mut statement = connection.prepare(
            "SELECT agent, COALESCE(SUM(occurrences), 0)
             FROM phrase_usage
             WHERE date>=?1 AND role='agent'
             GROUP BY agent
             ORDER BY SUM(occurrences) DESC, agent ASC",
        )?;
        let legend = statement
            .query_map(params![start_date], |row| {
                Ok(PhraseLegendItem {
                    agent: row.get(0)?,
                    occurrences: read_u64(row, 1)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(PhraseCloudResponse {
            range: range.into(),
            generated_at: Utc::now().to_rfc3339(),
            user,
            agents,
            legend,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_live_event(
        &self,
        received_at: &str,
        expires_at: &str,
        agent: &str,
        source_session_id: &str,
        event_name: &str,
        project_label: &str,
        payload_json: &str,
        status: &str,
    ) -> AppResult<()> {
        self.record_observed_live_event(&ObservedLiveEvent {
            occurred_at: received_at.into(),
            observed_at: received_at.into(),
            expires_at: expires_at.into(),
            agent: agent.into(),
            source_session_id: source_session_id.into(),
            source_event_id: None,
            source_event_fingerprint: Some(crate::privacy::stable_hash(&format!(
                "{agent}|{source_session_id}|{event_name}|{status}"
            ))),
            event_name: event_name.into(),
            project_label: project_label.into(),
            payload_json: payload_json.into(),
            status: status.into(),
            phase: (status == "waiting").then(|| "needs-you".into()),
        })
    }

    pub fn record_observed_live_event(&self, event: &ObservedLiveEvent) -> AppResult<()> {
        let mut connection = self.connect()?;
        let transaction = connection.transaction()?;
        let mut canonical = canonical_waiting_event(event);
        if let Some(canonical) = &mut canonical {
            resolve_waiting_episode(&transaction, event, canonical)?;
        }
        let canonical_inserted = if let Some(canonical) = &canonical {
            transaction.execute(
                "INSERT INTO canonical_events(
                    id, source_event_id, event_fingerprint, dedup_key,
                    protocol_version, schema_version, algorithm_version,
                    occurred_at, observed_at, source, agent, source_session_id,
                    lifecycle_status, live_phase, event_type, source_event_name,
                    evidence_level, source_coverage, privacy_level, project_label
                 ) VALUES(
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                    ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20
                 ) ON CONFLICT(dedup_key) DO NOTHING",
                params![
                    canonical.id,
                    canonical.source_event_id,
                    canonical.event_fingerprint,
                    canonical.dedup_key,
                    CANONICAL_EVENT_PROTOCOL_VERSION,
                    CANONICAL_EVENT_SCHEMA_VERSION,
                    LIVE_NORMALIZER_VERSION,
                    canonical.occurred_at,
                    canonical.observed_at,
                    "live-hook",
                    canonical.agent,
                    canonical.source_session_id,
                    "waiting",
                    canonical.live_phase,
                    "attention.waiting",
                    canonical.source_event_name,
                    "observed",
                    "exact-lifecycle",
                    "normalized-local",
                    canonical.project_label,
                ],
            )? == 1
        } else {
            false
        };
        let visible_project_label = canonical
            .as_ref()
            .map(|value| value.project_label.as_str())
            .unwrap_or(&event.project_label);
        transaction.execute(
            "INSERT INTO live_events(
                received_at, expires_at, agent, source_session_id,
                event_name, project_label, payload_json, status, canonical_event_id
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                event.observed_at,
                event.expires_at,
                event.agent,
                event.source_session_id,
                event.event_name,
                visible_project_label,
                event.payload_json,
                event.status,
                canonical.as_ref().map(|value| value.id.as_str()),
            ],
        )?;
        if canonical.is_none() || canonical_inserted {
            transaction.execute(
                "INSERT INTO live_session_metrics(
                    agent, source_session_id, started_at, last_seen_at,
                    event_count, waiting_count, error_count, completion_count
                 ) VALUES(?1, ?2, ?3, ?3, 1, ?4, ?5, ?6)
                 ON CONFLICT(agent, source_session_id) DO UPDATE SET
                    last_seen_at=excluded.last_seen_at,
                    event_count=live_session_metrics.event_count+1,
                    waiting_count=live_session_metrics.waiting_count+excluded.waiting_count,
                    error_count=live_session_metrics.error_count+excluded.error_count,
                    completion_count=live_session_metrics.completion_count+excluded.completion_count",
                params![
                    event.agent,
                    event.source_session_id,
                    event.observed_at,
                    i64::from(event.status == "waiting"),
                    i64::from(event.status == "error"),
                    i64::from(event.status == "completed"),
                ],
            )?;
        }
        transaction.execute(
            "DELETE FROM live_events WHERE expires_at<?1",
            params![event.observed_at],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn mark_notch_sessions_seen(&self, sessions: &[LiveSession]) -> AppResult<()> {
        if sessions.is_empty() {
            return Ok(());
        }
        let now = Utc::now();
        let now_text = now.to_rfc3339();
        let mut connection = self.connect()?;
        let transaction = connection.transaction()?;
        prune_notch_session_history(&transaction, now)?;
        for session in sessions {
            let session_json = serde_json::to_string(session)?;
            let jump_context_json = session
                .jump_context
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?;
            transaction.execute(
                "INSERT INTO notch_session_history(
                    id, session_json, cycle_started_at, seen_at,
                    completed_at, status, cleared_at, jump_context_json
                 ) VALUES(?1, ?2, ?3, ?4, NULL, 'active', NULL, ?5)
                 ON CONFLICT(id) DO UPDATE SET
                    session_json=excluded.session_json,
                    jump_context_json=COALESCE(
                        excluded.jump_context_json,
                        notch_session_history.jump_context_json
                    ),
                    cycle_started_at=CASE
                        WHEN notch_session_history.status='completed'
                        THEN excluded.seen_at
                        ELSE notch_session_history.cycle_started_at
                    END,
                    seen_at=excluded.seen_at,
                    completed_at=CASE
                        WHEN notch_session_history.status='completed'
                        THEN NULL
                        ELSE notch_session_history.completed_at
                    END,
                    status='active',
                    cleared_at=NULL",
                params![
                    session.id,
                    session_json,
                    session.started_at,
                    now_text,
                    jump_context_json
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn live_conversation_titles(
        &self,
        sources: &[(String, String)],
    ) -> AppResult<HashMap<(String, String), String>> {
        if sources.is_empty() {
            return Ok(HashMap::new());
        }
        let connection = self.connect()?;
        let mut statement = connection.prepare(
            "SELECT title
             FROM sessions
             WHERE agent=?1
               AND source_session_id=?2
               AND title IS NOT NULL
               AND TRIM(title)<>''
             ORDER BY COALESCE(ended_at, started_at) DESC
             LIMIT 1",
        )?;
        let mut titles = HashMap::new();
        for (agent, source_session_id) in sources {
            let key = (agent.clone(), source_session_id.clone());
            if titles.contains_key(&key) {
                continue;
            }
            let raw = statement
                .query_row(params![agent, source_session_id], |row| {
                    row.get::<_, String>(0)
                })
                .optional()?;
            if let Some(title) = raw.and_then(|value| crate::privacy::sanitize_title(&value)) {
                titles.insert(key, title);
            }
        }
        Ok(titles)
    }

    pub fn complete_notch_session(&self, session: &LiveSession) -> AppResult<bool> {
        let now = Utc::now();
        let completed_at = session.updated_at.clone();
        let session_json = serde_json::to_string(session)?;
        let jump_context_json = session
            .jump_context
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let mut connection = self.connect()?;
        let transaction = connection.transaction()?;
        let updated = transaction.execute(
            "UPDATE notch_session_history
             SET session_json=?2,
                 seen_at=?3,
                 completed_at=?4,
                 status='completed',
                 cleared_at=NULL,
                 jump_context_json=COALESCE(?5, jump_context_json)
             WHERE id=?1 AND status='active'",
            params![
                session.id,
                session_json,
                now.to_rfc3339(),
                completed_at,
                jump_context_json
            ],
        )?;
        prune_notch_session_history(&transaction, now)?;
        transaction.commit()?;
        Ok(updated > 0)
    }

    pub fn notch_completed_sessions(&self) -> AppResult<Vec<NotchCompletedSession>> {
        let now = Utc::now();
        let mut connection = self.connect()?;
        let transaction = connection.transaction()?;
        prune_notch_session_history(&transaction, now)?;
        let rows = {
            let mut statement = transaction.prepare(
                "SELECT session_json, cycle_started_at, completed_at, jump_context_json
                 FROM notch_session_history
                 WHERE status='completed'
                   AND cleared_at IS NULL
                   AND completed_at IS NOT NULL
                 ORDER BY completed_at DESC, id ASC
                 LIMIT 10",
            )?;
            statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        transaction.commit()?;
        rows.into_iter()
            .map(
                |(session_json, cycle_started_at, completed_at, jump_context_json)| {
                    let mut session: LiveSession = serde_json::from_str(&session_json)?;
                    session.jump_context = jump_context_json
                        .as_deref()
                        .map(serde_json::from_str)
                        .transpose()?;
                    Ok(NotchCompletedSession {
                        session,
                        cycle_started_at,
                        completed_at,
                    })
                },
            )
            .collect()
    }

    pub fn notch_completed_session(&self, id: &str) -> AppResult<Option<NotchCompletedSession>> {
        Ok(self
            .notch_completed_sessions()?
            .into_iter()
            .find(|completed| completed.session.id == id))
    }

    pub fn delete_notch_completed_session(&self, id: &str) -> AppResult<bool> {
        let connection = self.connect()?;
        Ok(connection.execute(
            "DELETE FROM notch_session_history WHERE id=?1 AND status='completed'",
            params![id],
        )? > 0)
    }

    pub fn clear_notch_completed_sessions(&self) -> AppResult<NotchClearResult> {
        let now = Utc::now();
        let token = now.to_rfc3339();
        let mut connection = self.connect()?;
        let transaction = connection.transaction()?;
        prune_notch_session_history(&transaction, now)?;
        let count = transaction.execute(
            "UPDATE notch_session_history
             SET cleared_at=?1
             WHERE status='completed' AND cleared_at IS NULL",
            params![token],
        )?;
        transaction.commit()?;
        Ok(NotchClearResult {
            token,
            count: count as u64,
        })
    }

    pub fn undo_clear_notch_completed_sessions(&self, token: &str) -> AppResult<u64> {
        let cleared_at = DateTime::parse_from_rfc3339(token)
            .map_err(|_| AppError::InvalidRequest("invalid Notch clear token".into()))?
            .with_timezone(&Utc);
        if Utc::now().signed_duration_since(cleared_at) > Duration::seconds(5) {
            return Ok(0);
        }
        let connection = self.connect()?;
        let restored = connection.execute(
            "UPDATE notch_session_history
             SET cleared_at=NULL
             WHERE status='completed' AND cleared_at=?1",
            params![token],
        )?;
        Ok(restored as u64)
    }

    pub fn purge_expired_live_events(&self) -> AppResult<u64> {
        let connection = self.connect()?;
        let removed = connection.execute(
            "DELETE FROM live_events WHERE expires_at<?1",
            params![Utc::now().to_rfc3339()],
        )?;
        Ok(removed as u64)
    }

    pub fn purge_misattributed_cursor_live_events(&self) -> AppResult<u64> {
        let mut connection = self.connect()?;
        let transaction = connection.transaction()?;
        let cursor_marker = "\"cursor_version\"";
        transaction.execute(
            "DELETE FROM live_session_metrics
             WHERE agent='claude-code'
               AND source_session_id IN (
                   SELECT DISTINCT source_session_id
                   FROM live_events
                   WHERE agent='claude-code'
                     AND instr(payload_json, ?1)>0
               )",
            params![cursor_marker],
        )?;
        transaction.execute(
            "DELETE FROM canonical_events
             WHERE id IN (
                SELECT canonical_event_id
                FROM live_events
                WHERE agent='claude-code'
                  AND instr(payload_json, ?1)>0
                  AND canonical_event_id IS NOT NULL
             )",
            params![cursor_marker],
        )?;
        let removed = transaction.execute(
            "DELETE FROM live_events
             WHERE agent='claude-code'
               AND instr(payload_json, ?1)>0",
            params![cursor_marker],
        )?;
        transaction.commit()?;
        Ok(removed as u64)
    }

    pub fn purge_codex_memory_live_events(&self) -> AppResult<u64> {
        let mut connection = self.connect()?;
        let transaction = connection.transaction()?;
        let memory_marker = "/.codex/memories";
        transaction.execute(
            "DELETE FROM live_session_metrics
             WHERE agent='codex'
               AND source_session_id IN (
                   SELECT DISTINCT source_session_id
                   FROM live_events
                   WHERE agent='codex'
                     AND project_label='memories'
                     AND instr(payload_json, ?1)>0
               )",
            params![memory_marker],
        )?;
        transaction.execute(
            "DELETE FROM canonical_events
             WHERE id IN (
                SELECT canonical_event_id
                FROM live_events
                WHERE agent='codex'
                  AND project_label='memories'
                  AND instr(payload_json, ?1)>0
                  AND canonical_event_id IS NOT NULL
             )",
            params![memory_marker],
        )?;
        let removed = transaction.execute(
            "DELETE FROM live_events
             WHERE agent='codex'
               AND project_label='memories'
               AND instr(payload_json, ?1)>0",
            params![memory_marker],
        )?;
        transaction.commit()?;
        Ok(removed as u64)
    }

    pub fn purge_known_live_validation_events(&self) -> AppResult<u64> {
        let mut connection = self.connect()?;
        let transaction = connection.transaction()?;
        let session_ids = [
            "claude-expanded-check",
            "codex-expanded-check",
            "vibemeter-direct-check",
            "vibemeter-visual-check",
        ];
        transaction.execute(
            "DELETE FROM live_session_metrics
             WHERE source_session_id IN (?1, ?2, ?3, ?4)",
            params![
                session_ids[0],
                session_ids[1],
                session_ids[2],
                session_ids[3]
            ],
        )?;
        transaction.execute(
            "DELETE FROM canonical_events
             WHERE id IN (
                SELECT canonical_event_id
                FROM live_events
                WHERE source_session_id IN (?1, ?2, ?3, ?4)
                  AND canonical_event_id IS NOT NULL
             )",
            params![
                session_ids[0],
                session_ids[1],
                session_ids[2],
                session_ids[3]
            ],
        )?;
        let removed = transaction.execute(
            "DELETE FROM live_events
             WHERE source_session_id IN (?1, ?2, ?3, ?4)",
            params![
                session_ids[0],
                session_ids[1],
                session_ids[2],
                session_ids[3]
            ],
        )?;
        transaction.commit()?;
        Ok(removed as u64)
    }

    pub fn live_activity(&self) -> AppResult<LiveActivityResponse> {
        let connection = self.connect()?;
        let now = Utc::now();
        let period_start = Local::now()
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .map(|value| {
                value
                    .and_local_timezone(Local)
                    .single()
                    .map(|local| local.with_timezone(&Utc).to_rfc3339())
                    .unwrap_or_else(|| format!("{}T00:00:00Z", Local::now().date_naive()))
            })
            .unwrap_or_else(|| format!("{}T00:00:00Z", Local::now().date_naive()));
        let history_start = (now - Duration::days(7)).to_rfc3339();
        let mut timeline_statement = connection.prepare(
            "WITH activity_events AS (
                SELECT id, occurred_at, observed_at, agent, project_label,
                       source_event_name AS event_name,
                       lifecycle_status AS status, source_session_id
                FROM canonical_events
                WHERE deleted_at IS NULL
                UNION ALL
                SELECT printf('legacy:%d:%s:%s', id, agent, source_session_id),
                       received_at, NULL, agent, project_label, event_name,
                       COALESCE(NULLIF(status, ''), 'running'), source_session_id
                FROM live_events
                WHERE canonical_event_id IS NULL
             )
             SELECT id, occurred_at, observed_at, agent, project_label, event_name,
                    status, source_session_id
             FROM activity_events
             WHERE occurred_at>=?1
             ORDER BY occurred_at DESC, id ASC
             LIMIT 200",
        )?;
        let timeline = timeline_statement
            .query_map(params![period_start], |row| {
                Ok(LiveTimelinePoint {
                    id: row.get(0)?,
                    occurred_at: row.get(1)?,
                    observed_at: row.get(2)?,
                    agent: row.get(3)?,
                    project_label: row.get(4)?,
                    event_name: row.get(5)?,
                    status: row.get(6)?,
                    source_session_id: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(timeline_statement);

        let mut history_statement = connection.prepare(
            "WITH activity_events AS (
                SELECT id, occurred_at, observed_at, agent, project_label,
                       source_event_name AS event_name,
                       lifecycle_status AS status, source_session_id
                FROM canonical_events
                WHERE deleted_at IS NULL
                UNION ALL
                SELECT printf('legacy:%d:%s:%s', id, agent, source_session_id),
                       received_at, NULL, agent, project_label, event_name,
                       COALESCE(NULLIF(status, ''), 'running'), source_session_id
                FROM live_events
                WHERE canonical_event_id IS NULL
             )
             SELECT le.id, le.occurred_at, le.observed_at, le.agent, le.project_label,
                    le.event_name, le.status, le.source_session_id,
                    (
                        SELECT s.id
                        FROM sessions s
                        WHERE s.agent=le.agent
                          AND s.source_session_id=le.source_session_id
                        ORDER BY COALESCE(s.ended_at, s.started_at) DESC
                        LIMIT 1
                    )
             FROM activity_events le
             WHERE le.occurred_at>=?1
               AND (
                    le.status IN ('waiting', 'error')
                    OR le.event_name='PermissionRequest'
               )
             ORDER BY le.occurred_at DESC, le.id ASC
             LIMIT 60",
        )?;
        let history = history_statement
            .query_map(params![history_start], |row| {
                let id: String = row.get(0)?;
                let agent: String = row.get(3)?;
                let event_name: String = row.get(5)?;
                let mut status: String = row.get(6)?;
                if status != "waiting" && status != "error" && event_name == "PermissionRequest" {
                    status = "waiting".into();
                }
                let source_session_id: String = row.get(7)?;
                Ok(LiveHistoryItem {
                    id: format!("hist:{id}"),
                    occurred_at: row.get(1)?,
                    observed_at: row.get(2)?,
                    agent,
                    project_label: row.get(4)?,
                    status,
                    event_name,
                    source_session_id,
                    session_id: row.get(8)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(history_statement);

        let mut lane_statement = connection.prepare(
            "SELECT agent,
                    COUNT(*) AS sessions,
                    SUM(waiting_count) AS waiting,
                    SUM(error_count) AS errors,
                    SUM(completion_count) AS completions
             FROM live_session_metrics
             WHERE last_seen_at>=?1
             GROUP BY agent
             ORDER BY agent",
        )?;
        let mut lanes = lane_statement
            .query_map(params![period_start], |row| {
                Ok(LiveConcurrencyLane {
                    agent: row.get(0)?,
                    session_count: read_u64(row, 1)?,
                    waiting_count: read_u64(row, 2)?,
                    error_count: read_u64(row, 3)?,
                    running_count: 0,
                    completed_count: read_u64(row, 4)?,
                    projects: Vec::new(),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(lane_statement);

        for lane in &mut lanes {
            let mut project_statement = connection.prepare(
                "SELECT DISTINCT project_label
                 FROM live_events
                 WHERE agent=?1 AND received_at>=?2 AND project_label!=''
                 ORDER BY project_label
                 LIMIT 6",
            )?;
            lane.projects = project_statement
                .query_map(params![lane.agent, period_start], |row| row.get(0))?
                .collect::<Result<Vec<_>, _>>()?;
        }

        Ok(LiveActivityResponse {
            generated_at: now.to_rfc3339(),
            period_start,
            timeline,
            history,
            concurrency: lanes,
        })
    }

    pub fn vcti_profile(&self, range: &str) -> AppResult<VctiProfile> {
        let connection = self.connect()?;
        let now = Utc::now();
        let window_days = crate::vcti::window_days_for_range(range);
        let start_timestamp = format!("{}T00:00:00Z", range_start(range));
        let behavior = query_behavior_summary(&connection, &start_timestamp)?;
        let mut statement = connection.prepare(
            "SELECT
                s.id, s.started_at, s.agent, s.model, s.active_seconds,
                s.input_tokens+s.output_tokens+s.cache_read_tokens+
                    s.cache_write_tokens+s.cache_write_1h_tokens+s.reasoning_tokens,
                s.cache_read_tokens, s.tool_calls, s.files_touched,
                s.lines_added+s.lines_deleted, s.errors, s.verification_events,
                s.human_interventions, s.subagent_count, s.model_switches,
                s.longest_uninterrupted_seconds,
                EXISTS(SELECT 1 FROM git_commits gc WHERE gc.session_id=s.id),
                sb.behavior_json,
                COALESCE(SUM(CASE WHEN lower(tu.tool)='git-review' THEN tu.count ELSE 0 END),0),
                COALESCE(SUM(CASE WHEN lower(tu.tool)='test' THEN tu.count ELSE 0 END),0),
                COALESCE(SUM(CASE WHEN lower(tu.tool)='build' THEN tu.count ELSE 0 END),0),
                COALESCE(SUM(CASE WHEN lower(tu.tool)='lint' THEN tu.count ELSE 0 END),0),
                COALESCE(SUM(CASE WHEN lower(tu.tool)='typecheck' THEN tu.count ELSE 0 END),0),
                COALESCE(SUM(CASE WHEN lower(tu.tool) IN ('read','read_file','file-read')
                    THEN tu.count ELSE 0 END),0),
                COALESCE(SUM(CASE WHEN lower(tu.tool) IN ('search','grep','rg','web_search')
                    THEN tu.count ELSE 0 END),0),
                COALESCE(SUM(CASE WHEN lower(tu.tool) IN ('edit','write','apply_patch','file-write')
                    THEN tu.count ELSE 0 END),0),
                COALESCE(SUM(CASE WHEN lower(tu.tool) IN ('shell','bash','exec','exec_command')
                    THEN tu.count ELSE 0 END),0),
                COALESCE(lm.waiting_count,0),
                COALESCE(lm.error_count,0),
                COALESCE(lm.completion_count,0)
             FROM sessions s
             LEFT JOIN session_behavior sb ON sb.session_id=s.id
             LEFT JOIN tool_usage tu ON tu.session_id=s.id
             LEFT JOIN live_session_metrics lm
                ON lm.agent=s.agent AND lm.source_session_id=s.source_session_id
             WHERE s.started_at>=?1
             GROUP BY s.id
             ORDER BY s.started_at",
        )?;
        let records = statement
            .query_map(params![start_timestamp], |row| {
                let behavior_json = row.get::<_, Option<String>>(17)?;
                let mut behavior: BehaviorSignals = behavior_json
                    .as_deref()
                    .and_then(|json| serde_json::from_str(json).ok())
                    .unwrap_or_default();
                let live_completion = read_u64(row, 29)?;
                if live_completion > 0 {
                    behavior.task_completions = behavior.task_completions.max(1);
                }
                Ok(crate::vcti::SessionBehaviorRecord {
                    id: row.get(0)?,
                    started_at: row.get(1)?,
                    agent: row.get(2)?,
                    model: row.get(3)?,
                    active_seconds: read_u64(row, 4)?,
                    total_tokens: read_u64(row, 5)?,
                    cache_read_tokens: read_u64(row, 6)?,
                    tool_calls: read_u64(row, 7)?,
                    files_touched: read_u64(row, 8)?,
                    lines_changed: read_u64(row, 9)?,
                    errors: read_u64(row, 10)?.max(read_u64(row, 28)?),
                    verification_events: read_u64(row, 11)?,
                    human_interventions: read_u64(row, 12)?.max(read_u64(row, 27)?),
                    subagent_count: read_u64(row, 13)?,
                    model_switches: read_u64(row, 14)?,
                    longest_uninterrupted_seconds: read_u64(row, 15)?,
                    has_commit: row.get::<_, i64>(16)? != 0,
                    behavior,
                    git_review_events: read_u64(row, 18)?,
                    test_events: read_u64(row, 19)?,
                    build_events: read_u64(row, 20)?,
                    lint_events: read_u64(row, 21)?,
                    typecheck_events: read_u64(row, 22)?,
                    read_events: read_u64(row, 23)?,
                    search_events: read_u64(row, 24)?,
                    edit_events: read_u64(row, 25)?,
                    shell_events: read_u64(row, 26)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        let available_agents = connection.query_row(
            "SELECT COUNT(DISTINCT agent) FROM sources WHERE available=1",
            [],
            |row| read_u64(row, 0),
        )?;
        let structure_analysis_enabled = connection
            .query_row(
                "SELECT value FROM app_settings WHERE key='vctiPromptStructure'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .is_none_or(|value| value == "true");
        let git_evidence_enabled = connection
            .query_row(
                "SELECT value FROM app_settings WHERE key='gitReadAllowed'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .is_some_and(|value| value == "true");
        let profile = crate::vcti::calculate(
            &records,
            behavior,
            available_agents,
            structure_analysis_enabled,
            git_evidence_enabled,
            now,
            window_days,
        );
        if window_days == 90 {
            connection.execute(
                "INSERT INTO vcti_profile_snapshots(
                    period_end, algorithm_version, profile_json, created_at
                 ) VALUES(?1, ?2, ?3, ?4)
                 ON CONFLICT(period_end, algorithm_version) DO UPDATE SET
                    profile_json=excluded.profile_json,
                    created_at=excluded.created_at",
                params![
                    profile.period_end,
                    profile.algorithm_version,
                    serde_json::to_string(&profile)?,
                    Utc::now().to_rfc3339()
                ],
            )?;
        }
        Ok(profile)
    }

    pub fn tasks(&self, range: &str) -> AppResult<Vec<TaskSummary>> {
        let connection = self.connect()?;
        query_tasks(
            &connection,
            &format!("{}T00:00:00Z", range_start(range)),
            200,
        )
    }

    pub fn merge_tasks(&self, task_ids: &[String], title: Option<&str>) -> AppResult<String> {
        if task_ids.len() < 2 {
            return Err(AppError::InvalidRequest(
                "at least two tasks are required".into(),
            ));
        }
        let mut connection = self.connect()?;
        let transaction = connection.transaction()?;
        let target = task_ids[0].clone();
        for source in task_ids.iter().skip(1) {
            let mut statement = transaction.prepare(
                "SELECT session_id FROM task_sessions WHERE task_id=?1 ORDER BY position",
            )?;
            let sessions = statement
                .query_map(params![source], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            drop(statement);
            for session_id in sessions {
                transaction.execute(
                    "DELETE FROM task_sessions WHERE session_id=?1",
                    params![session_id],
                )?;
                let position: i64 = transaction.query_row(
                    "SELECT COUNT(*) FROM task_sessions WHERE task_id=?1",
                    params![target],
                    |row| row.get(0),
                )?;
                transaction.execute(
                    "INSERT INTO task_sessions(task_id, session_id, position, user_assigned)
                     VALUES(?1, ?2, ?3, 1)",
                    params![target, session_id, position],
                )?;
            }
            transaction.execute("DELETE FROM tasks WHERE id=?1", params![source])?;
        }
        transaction.execute(
            "UPDATE tasks SET
                title=CASE WHEN ?2<>'' THEN ?2 ELSE title END,
                confidence=1.0, user_edited=1, grouping_state='manual',
                grouping_reason_json='[\"task.grouping.manual\"]',
                suggested_task_id=NULL, updated_at=?3
             WHERE id=?1",
            params![target, title.unwrap_or(""), Utc::now().to_rfc3339()],
        )?;
        transaction.commit()?;
        Ok(target)
    }

    pub fn split_session(&self, session_id: &str) -> AppResult<String> {
        let mut connection = self.connect()?;
        let transaction = connection.transaction()?;
        let session = transaction
            .query_row(
                "SELECT COALESCE(title,''), COALESCE(project_label,''),
                        CASE WHEN verification_events>0 THEN 'verified'
                             WHEN files_touched>0 THEN 'changed'
                             WHEN errors>0 THEN 'blocked' ELSE 'unverified' END
                 FROM sessions WHERE id=?1",
                params![session_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| AppError::InvalidRequest("session not found".into()))?;
        let task_id = crate::privacy::stable_hash(&format!("user-task:{session_id}"));
        let now = Utc::now().to_rfc3339();
        transaction.execute(
            "INSERT INTO tasks(
                id, title, project_label, status, confidence, user_edited,
                source_excluded, grouping_state, grouping_reason_json,
                created_at, updated_at
             ) VALUES(?1, ?2, ?3, ?4, 1.0, 1, 0, 'manual',
                '[\"task.grouping.manual\"]', ?5, ?5)
             ON CONFLICT(id) DO UPDATE SET updated_at=excluded.updated_at",
            params![task_id, session.0, session.1, session.2, now],
        )?;
        transaction.execute(
            "DELETE FROM task_sessions WHERE session_id=?1",
            params![session_id],
        )?;
        transaction.execute(
            "INSERT INTO task_sessions(task_id, session_id, position, user_assigned)
             VALUES(?1, ?2, 0, 1)",
            params![task_id, session_id],
        )?;
        transaction.commit()?;
        Ok(task_id)
    }

    pub fn playbook_items(&self, search: Option<&str>) -> AppResult<Vec<PlaybookItem>> {
        let connection = self.connect()?;
        let search = search.unwrap_or("").trim();
        let pattern = format!("%{search}%");
        let mut statement = connection.prepare(
            "SELECT id, title, body, category, project_label, task_type,
                    source_review_id, source_finding_id, source_excluded,
                    applied, created_at, updated_at
             FROM playbook_items
             WHERE ?1='' OR title LIKE ?2 OR body LIKE ?2 OR category LIKE ?2
             ORDER BY applied, updated_at DESC",
        )?;
        Ok(statement
            .query_map(params![search, pattern], playbook_from_row)?
            .collect::<Result<Vec<_>, _>>()?)
    }

    pub fn save_playbook_item(&self, request: &SavePlaybookRequest) -> AppResult<PlaybookItem> {
        let connection = self.connect()?;
        let id = request
            .id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let created_at = connection
            .query_row(
                "SELECT created_at FROM playbook_items WHERE id=?1",
                params![id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .unwrap_or_else(|| Utc::now().to_rfc3339());
        let updated_at = Utc::now().to_rfc3339();
        connection.execute(
            "INSERT INTO playbook_items(
                id, title, body, category, project_label, task_type,
                source_review_id, source_finding_id, source_excluded,
                applied, created_at, updated_at
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?9, ?10, ?11)
             ON CONFLICT(id) DO UPDATE SET
                title=excluded.title, body=excluded.body, category=excluded.category,
                project_label=excluded.project_label, task_type=excluded.task_type,
                source_review_id=excluded.source_review_id,
                source_finding_id=excluded.source_finding_id,
                applied=excluded.applied, updated_at=excluded.updated_at",
            params![
                id,
                request.title,
                request.body,
                request.category,
                request.project_label,
                request.task_type,
                request.source_review_id,
                request.source_finding_id,
                i64::from(request.applied),
                created_at,
                updated_at,
            ],
        )?;
        Ok(PlaybookItem {
            id,
            title: request.title.clone(),
            body: request.body.clone(),
            category: request.category.clone(),
            project_label: request.project_label.clone(),
            task_type: request.task_type.clone(),
            source_review_id: request.source_review_id.clone(),
            source_finding_id: request.source_finding_id.clone(),
            source_excluded: false,
            applied: request.applied,
            created_at,
            updated_at,
        })
    }

    pub fn delete_playbook_item(&self, id: &str) -> AppResult<()> {
        let connection = self.connect()?;
        connection.execute("DELETE FROM playbook_items WHERE id=?1", params![id])?;
        Ok(())
    }

    pub fn insights(&self, range: &str) -> AppResult<InsightsResponse> {
        let connection = self.connect()?;
        let start_timestamp = format!("{}T00:00:00Z", range_start(range));
        let sample_size = connection.query_row(
            "SELECT COUNT(*) FROM sessions WHERE started_at>=?1",
            params![start_timestamp],
            |row| read_u64(row, 0),
        )?;

        let avg_duration = if sample_size > 0 {
            connection
                .query_row(
                    "SELECT AVG(active_seconds) FROM sessions WHERE started_at>=?1",
                    params![start_timestamp],
                    |row| row.get::<_, Option<f64>>(0),
                )?
                .unwrap_or(0.0)
        } else {
            0.0
        };
        let verification_rate = if sample_size > 0 {
            connection
                .query_row(
                    "SELECT AVG(CASE WHEN files_touched>0 AND verification_events>0 THEN 1.0
                                    WHEN files_touched>0 THEN 0.0 ELSE NULL END)
                     FROM sessions WHERE started_at>=?1",
                    params![start_timestamp],
                    |row| row.get::<_, Option<f64>>(0),
                )?
                .unwrap_or(0.0)
        } else {
            0.0
        };
        let files_touched = connection.query_row(
            "SELECT COUNT(DISTINCT fc.path)
             FROM file_changes fc JOIN sessions s ON s.id=fc.session_id
             WHERE s.started_at>=?1",
            params![start_timestamp],
            |row| read_u64(row, 0),
        )?;
        let top_agent = connection
            .query_row(
                "SELECT agent, COUNT(*) FROM sessions WHERE started_at>=?1
                 GROUP BY agent ORDER BY 2 DESC LIMIT 1",
                params![start_timestamp],
                |row| Ok((row.get::<_, String>(0)?, read_u64(row, 1)?)),
            )
            .optional()?;
        let busiest_hour = connection
            .query_row(
                "SELECT CAST(strftime('%H', started_at, 'localtime') AS INTEGER) AS hour, COUNT(*)
                 FROM sessions WHERE started_at>=?1
                 GROUP BY hour ORDER BY 2 DESC, hour ASC LIMIT 1",
                params![start_timestamp],
                |row| Ok((read_u64(row, 0)?, read_u64(row, 1)?)),
            )
            .optional()?;

        let mut stats = vec![
            InsightStat {
                id: "sessions".into(),
                label_key: "metrics.sessions".into(),
                value: sample_size as f64,
                format: "number".into(),
                text_value: None,
            },
            InsightStat {
                id: "avg-duration".into(),
                label_key: "insights.avgDuration.title".into(),
                value: avg_duration,
                format: "duration".into(),
                text_value: None,
            },
            InsightStat {
                id: "verification-rate".into(),
                label_key: "insights.verificationRate.title".into(),
                value: verification_rate,
                format: "percent".into(),
                text_value: None,
            },
            InsightStat {
                id: "files-touched".into(),
                label_key: "insights.filesTouched.title".into(),
                value: files_touched as f64,
                format: "number".into(),
                text_value: None,
            },
        ];
        if let Some((agent, _)) = &top_agent {
            stats.push(InsightStat {
                id: "top-agent".into(),
                label_key: "insights.topAgent.title".into(),
                value: 0.0,
                format: "text".into(),
                text_value: Some(agent.clone()),
            });
        }
        if let Some((hour, _)) = busiest_hour {
            stats.push(InsightStat {
                id: "active-hours".into(),
                label_key: "insights.activeHours.title".into(),
                value: hour as f64,
                format: "text".into(),
                text_value: Some(format!("{hour:02}:00")),
            });
        }
        if sample_size > 0 {
            let (total_tokens, active_days, lines_changed, tool_calls, longest_focus) = connection
                .query_row(
                    "SELECT
                        COALESCE(SUM(input_tokens+output_tokens+cache_read_tokens+cache_write_tokens+cache_write_1h_tokens+reasoning_tokens),0),
                        COUNT(DISTINCT substr(started_at,1,10)),
                        COALESCE(SUM(lines_added+lines_deleted),0),
                        COALESCE(SUM(tool_calls),0),
                        COALESCE(MAX(longest_uninterrupted_seconds),0)
                     FROM sessions WHERE started_at>=?1",
                    params![start_timestamp],
                    |row| {
                        Ok((
                            read_u64(row, 0)?,
                            read_u64(row, 1)?,
                            read_u64(row, 2)?,
                            read_u64(row, 3)?,
                            read_u64(row, 4)?,
                        ))
                    },
                )?;
            stats.push(InsightStat {
                id: "total-tokens".into(),
                label_key: "metrics.tokens".into(),
                value: total_tokens as f64,
                format: "number".into(),
                text_value: None,
            });
            stats.push(InsightStat {
                id: "active-days".into(),
                label_key: "metrics.activeDays".into(),
                value: active_days as f64,
                format: "number".into(),
                text_value: None,
            });
            stats.push(InsightStat {
                id: "lines-changed".into(),
                label_key: "metrics.lines".into(),
                value: lines_changed as f64,
                format: "number".into(),
                text_value: None,
            });
            stats.push(InsightStat {
                id: "tool-calls".into(),
                label_key: "metrics.tools".into(),
                value: tool_calls as f64,
                format: "number".into(),
                text_value: None,
            });
            stats.push(InsightStat {
                id: "longest-focus".into(),
                label_key: "insights.longestFocus.title".into(),
                value: longest_focus as f64,
                format: "duration".into(),
                text_value: None,
            });
        }

        let mut items = Vec::new();
        // Insufficient sample stays on the baseline banner only — never occupies a card slot.
        let mut churn_statement = connection.prepare(
            "SELECT fc.path, COUNT(DISTINCT fc.session_id), SUM(fc.modification_count)
             FROM file_changes fc JOIN sessions s ON s.id=fc.session_id
             WHERE s.started_at>=?1
             GROUP BY fc.path HAVING COUNT(DISTINCT fc.session_id)>=2
             ORDER BY 2 DESC, 3 DESC LIMIT 12",
        )?;
        let churn_candidates = churn_statement
            .query_map(params![start_timestamp], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    read_u64(row, 1)?,
                    read_u64(row, 2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        if let Some((path, sessions, edits)) = churn_candidates
            .into_iter()
            .find(|(path, _, _)| !is_routine_edit_path(path))
        {
            let target_session_id = connection
                .query_row(
                    "SELECT fc.session_id
                     FROM file_changes fc JOIN sessions s ON s.id=fc.session_id
                     WHERE fc.path=?1 AND s.started_at>=?2
                     ORDER BY s.started_at DESC LIMIT 1",
                    params![&path, &start_timestamp],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            items.push(InsightItem {
                id: "high-churn-file".into(),
                tier: "fact".into(),
                title_key: "insights.churn.title".into(),
                detail_key: "insights.churn.detail".into(),
                value: Some(sessions as f64),
                target_session_id,
                sample_size,
                trend: None,
                evidence: vec![EvidenceReference {
                    kind: "file".into(),
                    id: crate::privacy::stable_hash(&path),
                    label: format!("{path} · {edits}"),
                }],
                promotable: false,
            });
        }
        if sample_size > 0 {
            let verification_gap = connection.query_row(
                "SELECT AVG(CASE WHEN files_touched>0 AND verification_events=0 THEN 1.0 ELSE 0.0 END)
                 FROM sessions WHERE started_at>=?1",
                params![start_timestamp],
                |row| row.get::<_, Option<f64>>(0),
            )?;
            if verification_gap.is_some_and(|value| value >= 0.25) {
                items.push(InsightItem {
                    id: "verification-gap".into(),
                    tier: "inference".into(),
                    title_key: "insights.verificationGap.title".into(),
                    detail_key: "insights.verificationGap.detail".into(),
                    value: verification_gap,
                    target_session_id: None,
                    sample_size,
                    trend: None,
                    evidence: Vec::new(),
                    promotable: true,
                });
            }
        }
        if let Some((hour, count)) = busiest_hour
            && sample_size > 0
        {
            items.push(InsightItem {
                id: "peak-hour".into(),
                tier: "fact".into(),
                title_key: "insights.peakHour.title".into(),
                detail_key: "insights.peakHour.detail".into(),
                value: None,
                target_session_id: None,
                sample_size,
                trend: None,
                evidence: vec![EvidenceReference {
                    kind: "sessions".into(),
                    id: "peak-hour".into(),
                    label: format!("{hour:02}:00 · {count}"),
                }],
                promotable: false,
            });
        }
        let mut comparison = query_comparison(&connection, &start_timestamp, "agent")?;
        comparison.extend(query_comparison(&connection, &start_timestamp, "model")?);
        let behavior = query_behavior_summary(&connection, &start_timestamp)?;
        Ok(InsightsResponse {
            items,
            comparison,
            minimum_sample_size: 20,
            sample_size,
            stats,
            behavior,
        })
    }

    pub fn sessions(
        &self,
        range: &str,
        filters: SessionListFilters<'_>,
        page: u64,
        page_size: u64,
    ) -> AppResult<SessionsResponse> {
        let connection = self.connect()?;
        let start_timestamp = format!("{}T00:00:00Z", range_start(range));
        let page_size = page_size.clamp(1, 100);
        let (items, total) =
            query_session_rows(&connection, &start_timestamp, &filters, page, page_size)?;
        let (models, projects) =
            query_session_facets(&connection, &start_timestamp, filters.agent)?;
        Ok(SessionsResponse {
            items,
            total,
            page,
            page_size,
            models,
            projects,
        })
    }

    pub fn session_detail(&self, id: &str) -> AppResult<SessionDetail> {
        let connection = self.connect()?;
        let summary = connection
            .query_row(
                "SELECT id, agent, model, title, COALESCE(project_label, project_hash), started_at, ended_at,
                    active_seconds, input_tokens, output_tokens, cache_read_tokens,
                    cache_write_tokens, cache_write_1h_tokens, reasoning_tokens,
                    estimated_cost_usd, cost_coverage_tokens, tool_calls, files_touched,
                    lines_added, lines_deleted, errors, retries, verification_events,
                    longest_uninterrupted_seconds, subagent_count,
                    EXISTS(SELECT 1 FROM git_commits gc WHERE gc.session_id=sessions.id)
                 FROM sessions WHERE id=?1",
                params![id],
                session_from_row,
            )
            .optional()?
            .ok_or_else(|| AppError::InvalidRequest("session not found".into()))?;

        let mut tool_statement = connection.prepare(
            "SELECT tool, count FROM tool_usage WHERE session_id=?1 ORDER BY count DESC, tool",
        )?;
        let tools = tool_statement
            .query_map(params![id], |row| {
                Ok(DistributionItem {
                    id: row.get(0)?,
                    label: row.get(0)?,
                    value: read_u64(row, 1)? as f64,
                    secondary_value: None,
                    provenance: Provenance::Observed,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let daily = query_daily_for_session(&connection, id)?;
        let mut warnings = Vec::new();
        if summary.cost_coverage < 0.999 && summary.usage.total() > 0 {
            warnings.push(CoverageNotice {
                id: "session-partial-cost".into(),
                level: "info".into(),
                message_key: "coverage.partialCost".into(),
                agent: Some(summary.agent.clone()),
                value: Some(summary.cost_coverage),
            });
        }
        let events = query_events(&connection, id)?;
        let phases = derive_process_phases(events);
        let file_changes = query_file_changes(&connection, id)?;
        let git_evidence = query_git_evidence(&connection, id)?;
        let task = query_task_for_session(&connection, id)?;
        let capabilities = capabilities_for_agent(&summary.agent);
        Ok(SessionDetail {
            summary,
            tools,
            daily,
            warnings,
            task,
            phases,
            file_changes,
            git_evidence,
            capabilities,
        })
    }

    pub fn comparison(&self, range: &str) -> AppResult<Vec<ComparisonItem>> {
        let connection = self.connect()?;
        let start_timestamp = format!("{}T00:00:00Z", range_start(range));
        let mut items = query_comparison(&connection, &start_timestamp, "agent")?;
        items.extend(query_comparison(&connection, &start_timestamp, "model")?);
        Ok(items)
    }

    pub fn sources(&self) -> AppResult<Vec<SourceStatus>> {
        let connection = self.connect()?;
        let mut statement = connection.prepare(
            "SELECT agent, available, capability_level, session_count, last_indexed_at,
                    status, warning_count, selected, path_hash
             FROM sources ORDER BY CASE agent WHEN 'claude-code' THEN 0 ELSE 1 END",
        )?;
        let mut items = statement
            .query_map([], |row| {
                let path_hash: String = row.get(8)?;
                Ok(SourceStatus {
                    agent: row.get(0)?,
                    available: row.get::<_, i64>(1)? != 0,
                    capability_level: row.get(2)?,
                    live_capability: "none".into(),
                    parser_version: PARSER_VERSION.into(),
                    session_count: read_u64(row, 3)?,
                    last_indexed_at: row.get(4)?,
                    status: row.get(5)?,
                    warning_count: read_u64(row, 6)?,
                    selected: row.get::<_, i64>(7)? != 0,
                    path_label: path_hash.chars().take(6).collect(),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut observed = items
            .drain(..)
            .map(|item| (item.agent.clone(), item))
            .collect::<HashMap<_, _>>();
        let mut ordered = source_capabilities()
            .iter()
            .map(|capability| {
                let mut item = observed
                    .remove(&capability.agent)
                    .unwrap_or_else(|| SourceStatus {
                        agent: capability.agent.clone(),
                        available: false,
                        selected: false,
                        capability_level: capability.history_capability.as_str().into(),
                        live_capability: capability.live_capability.as_str().into(),
                        parser_version: PARSER_VERSION.into(),
                        session_count: 0,
                        last_indexed_at: None,
                        status: "not-found".into(),
                        warning_count: 0,
                        path_label: String::new(),
                    });
                item.capability_level = capability.history_capability.as_str().into();
                item.live_capability = capability.live_capability.as_str().into();
                item.parser_version = PARSER_VERSION.into();
                item
            })
            .collect::<Vec<_>>();
        let mut unknown = observed.into_values().collect::<Vec<_>>();
        unknown.sort_by(|left, right| left.agent.cmp(&right.agent));
        ordered.extend(unknown);
        Ok(ordered)
    }

    pub fn range_usage_and_activity(&self, range: &str) -> AppResult<RangeUsageActivity> {
        let connection = self.connect()?;
        let start_date = range_start(range);
        let start_timestamp = format!("{start_date}T00:00:00Z");
        let totals = query_overview_totals(&connection, &start_timestamp, &start_date)?;
        let hourly = if range == "today" {
            query_hourly(&connection, &start_date)?
        } else {
            Vec::new()
        };
        Ok((
            totals.usage,
            totals.estimated_cost_usd,
            query_daily(&connection, &start_date)?,
            hourly,
        ))
    }

    pub fn record_export(
        &self,
        id: &str,
        template_id: &str,
        locale: &str,
        aspect_ratio: &str,
        format: &str,
        model_hash: &str,
    ) -> AppResult<()> {
        let connection = self.connect()?;
        connection.execute(
            "INSERT INTO share_exports(id, template_id, locale, aspect_ratio, format, model_hash, created_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id,
                template_id,
                locale,
                aspect_ratio,
                format,
                model_hash,
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }
}

fn prune_notch_session_history(transaction: &Transaction<'_>, now: DateTime<Utc>) -> AppResult<()> {
    let cutoff = (now - Duration::hours(24)).to_rfc3339();
    transaction.execute(
        "DELETE FROM notch_session_history
         WHERE (status='completed' AND completed_at<?1)
            OR (status='active' AND seen_at<?1)",
        params![cutoff],
    )?;
    transaction.execute(
        "DELETE FROM notch_session_history
         WHERE status='completed'
           AND cleared_at IS NULL
           AND id NOT IN (
               SELECT id
               FROM notch_session_history
               WHERE status='completed' AND cleared_at IS NULL
               ORDER BY completed_at DESC, id ASC
               LIMIT 10
           )",
        [],
    )?;
    Ok(())
}

fn replace_session_children(
    transaction: &Transaction<'_>,
    session_id: &str,
    state: &ParseState,
) -> AppResult<()> {
    transaction.execute(
        "DELETE FROM daily_usage WHERE session_id=?1",
        params![session_id],
    )?;
    transaction.execute(
        "DELETE FROM hourly_usage WHERE session_id=?1",
        params![session_id],
    )?;
    transaction.execute(
        "DELETE FROM tool_usage WHERE session_id=?1",
        params![session_id],
    )?;
    transaction.execute(
        "DELETE FROM skill_usage WHERE session_id=?1",
        params![session_id],
    )?;
    transaction.execute(
        "DELETE FROM session_files WHERE session_id=?1",
        params![session_id],
    )?;
    transaction.execute(
        "DELETE FROM events WHERE session_id=?1",
        params![session_id],
    )?;
    transaction.execute(
        "DELETE FROM phrase_usage WHERE session_id=?1",
        params![session_id],
    )?;
    transaction.execute(
        "DELETE FROM file_changes WHERE session_id=?1",
        params![session_id],
    )?;
    transaction.execute(
        "DELETE FROM git_files WHERE session_id=?1",
        params![session_id],
    )?;
    transaction.execute(
        "DELETE FROM git_commits WHERE session_id=?1",
        params![session_id],
    )?;
    transaction.execute(
        "DELETE FROM git_evidence WHERE session_id=?1",
        params![session_id],
    )?;

    let model = state.primary_model().unwrap_or_else(|| "unknown".into());
    for (date, aggregate) in &state.daily {
        transaction.execute(
            "INSERT INTO daily_usage(
                session_id, date, agent, model, input_tokens, output_tokens,
                cache_read_tokens, cache_write_tokens, cache_write_1h_tokens,
                reasoning_tokens, active_seconds, events, tool_calls, errors,
                verification_events, estimated_cost_usd
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                session_id,
                date,
                state.agent.as_str(),
                model,
                sql_i64(aggregate.usage.input_tokens),
                sql_i64(aggregate.usage.output_tokens),
                sql_i64(aggregate.usage.cache_read_tokens),
                sql_i64(aggregate.usage.cache_write_tokens),
                sql_i64(aggregate.usage.cache_write_1h_tokens),
                sql_i64(aggregate.usage.reasoning_tokens),
                sql_i64(aggregate.active_seconds),
                sql_i64(aggregate.events),
                sql_i64(aggregate.tool_calls),
                sql_i64(aggregate.errors),
                sql_i64(aggregate.verification_events),
                aggregate.estimated_cost_usd,
            ],
        )?;
    }
    for (hour, usage) in &state.hourly {
        transaction.execute(
            "INSERT INTO hourly_usage(
                session_id, hour, agent, model, input_tokens, output_tokens,
                cache_read_tokens, cache_write_tokens, cache_write_1h_tokens,
                reasoning_tokens
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                session_id,
                hour,
                state.agent.as_str(),
                model,
                sql_i64(usage.input_tokens),
                sql_i64(usage.output_tokens),
                sql_i64(usage.cache_read_tokens),
                sql_i64(usage.cache_write_tokens),
                sql_i64(usage.cache_write_1h_tokens),
                sql_i64(usage.reasoning_tokens),
            ],
        )?;
    }
    for (tool, count) in &state.tool_counts {
        transaction.execute(
            "INSERT INTO tool_usage(session_id, tool, count) VALUES(?1, ?2, ?3)",
            params![session_id, tool, sql_i64(*count)],
        )?;
    }
    for (skill, count) in &state.skill_counts {
        transaction.execute(
            "INSERT INTO skill_usage(session_id, skill, count) VALUES(?1, ?2, ?3)",
            params![session_id, skill, sql_i64(*count)],
        )?;
    }
    for file_hash in &state.touched_file_hashes {
        transaction.execute(
            "INSERT INTO session_files(session_id, file_hash) VALUES(?1, ?2)",
            params![session_id, file_hash],
        )?;
    }
    for event in &state.events {
        transaction.execute(
            "INSERT INTO events(
                session_id, sequence, occurred_at, event_type, category, name,
                success, duration_ms, provenance
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                session_id,
                sql_i64(event.sequence),
                event.occurred_at,
                event.event_type,
                event.category,
                event.name,
                event.success.map(i64::from),
                event.duration_ms.map(sql_i64),
                event.provenance,
            ],
        )?;
    }
    for phrase in state.phrase_counts.values() {
        transaction.execute(
            "INSERT INTO phrase_usage(
                session_id, date, role, agent, phrase, occurrences
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                session_id,
                phrase.date,
                phrase.role,
                state.agent.as_str(),
                phrase.phrase,
                sql_i64(phrase.occurrences),
            ],
        )?;
    }
    for change in state.file_changes.values() {
        transaction.execute(
            "INSERT INTO file_changes(
                session_id, path, change_kind, lines_added, lines_deleted,
                modification_count, first_observed_at, last_observed_at, final_state
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'observed')",
            params![
                session_id,
                change.path,
                change.change_kind,
                sql_i64(change.lines_added),
                sql_i64(change.lines_deleted),
                sql_i64(change.modification_count),
                change.first_observed_at,
                change.last_observed_at,
            ],
        )?;
    }
    if let Some(git) = &state.git_evidence {
        transaction.execute(
            "INSERT INTO git_evidence(session_id, available, state, branch, inspected_at)
             VALUES(?1, ?2, ?3, ?4, ?5)",
            params![
                session_id,
                i64::from(git.available),
                git.state,
                git.branch,
                Utc::now().to_rfc3339(),
            ],
        )?;
        for commit in &git.commits {
            transaction.execute(
                "INSERT INTO git_commits(session_id, hash, subject, committed_at)
                 VALUES(?1, ?2, ?3, ?4)",
                params![session_id, commit.hash, commit.subject, commit.committed_at],
            )?;
            for file in &commit.files {
                transaction.execute(
                    "INSERT INTO git_files(session_id, commit_hash, path, lines_added, lines_deleted)
                     VALUES(?1, ?2, ?3, ?4, ?5)",
                    params![
                        session_id,
                        commit.hash,
                        file.path,
                        sql_i64(file.lines_added),
                        sql_i64(file.lines_deleted),
                    ],
                )?;
            }
        }
    }
    Ok(())
}

fn upsert_derived_task(
    transaction: &Transaction<'_>,
    session_id: &str,
    state: &ParseState,
    now: &str,
) -> AppResult<()> {
    let existing = transaction
        .query_row(
            "SELECT task_id FROM task_sessions WHERE session_id=?1",
            params![session_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let status = derive_task_status(state);
    let title = state
        .title
        .clone()
        .or_else(|| state.project_label.clone())
        .unwrap_or_default();
    if let Some(task_id) = existing {
        transaction.execute(
            "UPDATE tasks SET
                title=CASE WHEN user_edited=0 AND ?1<>'' THEN ?1 ELSE title END,
                status=?2, updated_at=?3
             WHERE id=?4",
            params![title, status, now, task_id],
        )?;
        return Ok(());
    }

    let project_label = state.project_label.clone().unwrap_or_default();
    let branch = state
        .git_evidence
        .as_ref()
        .and_then(|git| git.branch.as_deref())
        .unwrap_or("");
    let match_result = find_task_match(transaction, state, &title, &project_label, branch)?;
    let auto_candidate = match_result
        .as_ref()
        .filter(|candidate| candidate.grouping_state == "auto");
    let task_id = auto_candidate.map_or_else(
        || crate::privacy::stable_hash(&format!("task:{session_id}")),
        |candidate| candidate.task_id.clone(),
    );
    let confidence = match_result
        .as_ref()
        .map_or(0.92, |candidate| candidate.score);
    let grouping_state = auto_candidate.map_or_else(
        || {
            if match_result.is_some() {
                "suggested"
            } else {
                "separate"
            }
        },
        |_| "auto",
    );
    let grouping_reasons = match_result
        .as_ref()
        .map(|candidate| candidate.reason_keys.clone())
        .unwrap_or_default();
    let suggested_task_id = match_result
        .as_ref()
        .filter(|candidate| candidate.grouping_state == "suggested")
        .map(|candidate| candidate.task_id.clone());
    transaction.execute(
        "INSERT INTO tasks(
            id, title, project_label, status, confidence, user_edited,
            source_excluded, grouping_state, grouping_reason_json,
            suggested_task_id, created_at, updated_at
         ) VALUES(?1, ?2, ?3, ?4, ?5, 0, 0, ?6, ?7, ?8, ?9, ?9)
         ON CONFLICT(id) DO UPDATE SET
            title=CASE WHEN user_edited=0 AND title='' AND excluded.title<>'' THEN excluded.title ELSE title END,
            status=excluded.status,
            confidence=MAX(confidence, excluded.confidence),
            grouping_state=CASE WHEN user_edited=0 THEN excluded.grouping_state ELSE grouping_state END,
            grouping_reason_json=CASE WHEN user_edited=0 THEN excluded.grouping_reason_json ELSE grouping_reason_json END,
            suggested_task_id=CASE WHEN user_edited=0 THEN excluded.suggested_task_id ELSE suggested_task_id END,
            updated_at=excluded.updated_at",
        params![
            task_id,
            title,
            project_label,
            status,
            confidence,
            grouping_state,
            serde_json::to_string(&grouping_reasons)?,
            suggested_task_id,
            now,
        ],
    )?;
    let position: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM task_sessions WHERE task_id=?1",
        params![task_id],
        |row| row.get(0),
    )?;
    transaction.execute(
        "INSERT INTO task_sessions(task_id, session_id, position, user_assigned)
         VALUES(?1, ?2, ?3, 0)",
        params![task_id, session_id, position],
    )?;
    Ok(())
}

#[derive(Debug)]
struct TaskMatch {
    task_id: String,
    score: f64,
    grouping_state: &'static str,
    reason_keys: Vec<String>,
}

fn find_task_match(
    transaction: &Transaction<'_>,
    state: &ParseState,
    title: &str,
    project_label: &str,
    branch: &str,
) -> AppResult<Option<TaskMatch>> {
    let objective = state
        .prompt_excerpt
        .as_deref()
        .or(state.title.as_deref())
        .unwrap_or("")
        .trim();
    if project_label.is_empty()
        || objective.is_empty()
        || objective.eq_ignore_ascii_case(project_label)
        || semantic_ngrams(objective).len() < 8
    {
        return Ok(None);
    }
    let current_text = format!("{title} {}", state.prompt_excerpt.as_deref().unwrap_or(""));
    let current_paths = state.file_changes.keys().cloned().collect::<HashSet<_>>();
    let mut statement = transaction.prepare(
        "SELECT t.id, t.title, MAX(COALESCE(s.ended_at,s.started_at)),
            COALESCE((SELECT s2.prompt_excerpt FROM sessions s2
                JOIN task_sessions ts2 ON ts2.session_id=s2.id
                WHERE ts2.task_id=t.id ORDER BY s2.started_at DESC LIMIT 1),''),
            COALESCE((SELECT ge.branch FROM git_evidence ge
                JOIN sessions s3 ON s3.id=ge.session_id
                JOIN task_sessions ts3 ON ts3.session_id=s3.id
                WHERE ts3.task_id=t.id ORDER BY s3.started_at DESC LIMIT 1),'')
         FROM tasks t
         JOIN task_sessions ts ON ts.task_id=t.id
         JOIN sessions s ON s.id=ts.session_id
         WHERE t.project_label=?1 AND t.user_edited=0
         GROUP BY t.id, t.title
         ORDER BY 3 DESC LIMIT 80",
    )?;
    let candidates = statement
        .query_map(params![project_label], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);

    let mut best: Option<TaskMatch> = None;
    for (task_id, candidate_title, last_end, candidate_prompt, candidate_branch) in candidates {
        let semantic = semantic_similarity(
            &current_text,
            &format!("{candidate_title} {candidate_prompt}"),
        );
        if semantic < 0.24 {
            continue;
        }
        let candidate_paths = if current_paths.is_empty() {
            HashSet::new()
        } else {
            let mut paths = transaction.prepare(
                "SELECT DISTINCT fc.path FROM file_changes fc
                 JOIN task_sessions ts ON ts.session_id=fc.session_id
                 WHERE ts.task_id=?1",
            )?;
            paths
                .query_map(params![task_id], |row| row.get::<_, String>(0))?
                .collect::<Result<HashSet<_>, _>>()?
        };
        let file_overlap = overlap_ratio(&current_paths, &candidate_paths);
        let branch_match = !branch.is_empty() && branch == candidate_branch;
        let time_affinity = time_affinity(state.started_at.as_deref(), &last_end);
        let score = semantic * 0.72
            + file_overlap * 0.14
            + if branch_match { 0.05 } else { 0.0 }
            + time_affinity * 0.09;
        let grouping_state = if semantic >= 0.72 || (semantic >= 0.50 && score >= 0.56) {
            "auto"
        } else if semantic >= 0.32 && score >= 0.34 {
            "suggested"
        } else {
            continue;
        };
        let mut reason_keys = vec!["task.grouping.semantic".into()];
        if file_overlap > 0.0 {
            reason_keys.push("task.grouping.files".into());
        }
        if branch_match {
            reason_keys.push("task.grouping.branch".into());
        }
        if time_affinity >= 0.45 {
            reason_keys.push("task.grouping.time".into());
        }
        let candidate = TaskMatch {
            task_id,
            score: score.clamp(0.0, 1.0),
            grouping_state,
            reason_keys,
        };
        if best.as_ref().is_none_or(|current| {
            candidate.score > current.score
                || (candidate.score == current.score
                    && candidate.grouping_state == "auto"
                    && current.grouping_state != "auto")
        }) {
            best = Some(candidate);
        }
    }
    Ok(best)
}

fn semantic_similarity(left: &str, right: &str) -> f64 {
    let left = semantic_ngrams(left);
    let right = semantic_ngrams(right);
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let shared = left.intersection(&right).count() as f64;
    2.0 * shared / (left.len() + right.len()) as f64
}

fn semantic_ngrams(value: &str) -> HashSet<String> {
    let mut normalized = value.to_lowercase();
    for boilerplate in [
        "[path]",
        "workspace",
        "read agent.md first",
        "agent.md",
        "role:",
        "task:",
        "first read",
        "工作目录",
        "首先阅读",
        "先阅读",
        "根据 agent.md",
    ] {
        normalized = normalized.replace(boilerplate, " ");
    }
    let characters = normalized
        .chars()
        .filter(|character| character.is_alphanumeric())
        .take(900)
        .collect::<Vec<_>>();
    if characters.len() < 3 {
        return characters
            .into_iter()
            .map(|character| character.to_string())
            .collect();
    }
    characters
        .windows(3)
        .map(|window| window.iter().collect::<String>())
        .collect()
}

fn overlap_ratio(left: &HashSet<String>, right: &HashSet<String>) -> f64 {
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    left.intersection(right).count() as f64 / left.len().min(right.len()) as f64
}

fn time_affinity(start: Option<&str>, end: &str) -> f64 {
    let Some((start, end)) = start
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .zip(DateTime::parse_from_rfc3339(end).ok())
    else {
        return 0.0;
    };
    let hours = start.signed_duration_since(end).num_hours().unsigned_abs();
    match hours {
        0..=6 => 1.0,
        7..=48 => 0.8,
        49..=168 => 0.45,
        169..=720 => 0.15,
        _ => 0.0,
    }
}

fn derive_task_status(state: &ParseState) -> &'static str {
    if state
        .git_evidence
        .as_ref()
        .is_some_and(|git| !git.commits.is_empty())
        || state.verification_events > 0
    {
        "verified"
    } else if !state.file_changes.is_empty() || state.lines_added > 0 || state.lines_deleted > 0 {
        "changed"
    } else if state.errors > 0 {
        "blocked"
    } else {
        "unverified"
    }
}

fn upsert_warning_rows(
    transaction: &Transaction<'_>,
    file_hash: &str,
    state: &ParseState,
    now: &str,
) -> AppResult<()> {
    for (code, count) in [
        ("malformed-record", state.malformed_records),
        ("unknown-record", state.unknown_records),
    ] {
        if count == 0 {
            continue;
        }
        transaction.execute(
            "INSERT INTO parser_warnings(source_file_hash, agent, warning_code, count, updated_at)
             VALUES(?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(source_file_hash, warning_code) DO UPDATE SET
                count=excluded.count, updated_at=excluded.updated_at",
            params![file_hash, state.agent.as_str(), code, sql_i64(count), now],
        )?;
    }
    Ok(())
}

fn session_database_id(agent: AgentKind, source_session_id: &str) -> String {
    crate::privacy::stable_hash(&format!("{}:{source_session_id}", agent.as_str()))
}

fn source_file_session_database_id(
    agent: AgentKind,
    source_session_id: &str,
    source_file_hash: &str,
) -> String {
    crate::privacy::stable_hash(&format!(
        "{}:{source_session_id}:{source_file_hash}",
        agent.as_str()
    ))
}

fn range_start(range: &str) -> String {
    let days = match range {
        "today" => 0,
        "7d" => 6,
        "30d" => 29,
        "90d" => 89,
        "180d" => 179,
        "year" => 364,
        "all" => 3650,
        _ => 29,
    };
    (Local::now().date_naive() - Duration::days(days))
        .format("%Y-%m-%d")
        .to_string()
}

fn query_phrase_cloud(
    connection: &Connection,
    start_date: &str,
    role: &str,
) -> AppResult<PhraseCloud> {
    #[derive(Default)]
    struct PendingPhrase {
        occurrences: u64,
        session_count: u64,
        session_ids: HashSet<String>,
        agents: BTreeMap<String, (u64, u64)>,
        models: BTreeMap<String, (u64, u64)>,
    }

    let sample_sessions = connection.query_row(
        "SELECT COUNT(DISTINCT session_id)
         FROM phrase_usage WHERE date>=?1 AND role=?2",
        params![start_date, role],
        |row| read_u64(row, 0),
    )?;
    let mut statement = connection.prepare(
        "SELECT p.phrase, p.agent, COALESCE(s.model, ''),
                COALESCE(SUM(p.occurrences), 0),
                COUNT(DISTINCT p.session_id),
                GROUP_CONCAT(DISTINCT p.session_id)
         FROM phrase_usage p
         JOIN sessions s ON s.id=p.session_id
         WHERE p.date>=?1 AND p.role=?2
         GROUP BY p.phrase, p.agent, COALESCE(s.model, '')
         ORDER BY p.phrase, p.agent, COALESCE(s.model, '')",
    )?;
    let rows = statement
        .query_map(params![start_date, role], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                read_u64(row, 3)?,
                read_u64(row, 4)?,
                row.get::<_, Option<String>>(5)?.unwrap_or_default(),
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut grouped = HashMap::<String, PendingPhrase>::new();
    for (phrase, agent, model, occurrences, sessions, session_ids) in rows {
        let item = grouped.entry(phrase).or_default();
        item.occurrences = item.occurrences.saturating_add(occurrences);
        for session_id in session_ids.split(',').filter(|value| !value.is_empty()) {
            item.session_ids.insert(session_id.to_string());
        }
        let agent_counts = item.agents.entry(agent).or_default();
        agent_counts.0 = agent_counts.0.saturating_add(occurrences);
        agent_counts.1 = agent_counts.1.saturating_add(sessions);
        if !model.is_empty() {
            let model_counts = item.models.entry(model).or_default();
            model_counts.0 = model_counts.0.saturating_add(occurrences);
            model_counts.1 = model_counts.1.saturating_add(sessions);
        }
    }
    for item in grouped.values_mut() {
        item.session_count = item.session_ids.len() as u64;
    }

    let mut scored = grouped
        .into_iter()
        .filter_map(|(phrase, item)| {
            if item.session_count < 2 || item.occurrences < 2 {
                return None;
            }
            let voice_factor = phrase_voice_factor(&phrase)?;
            let visible_length = phrase
                .chars()
                .filter(|character| !character.is_whitespace())
                .count();
            let length_factor = 1.0 + visible_length.min(12) as f64 * 0.16;
            let frame_factor = if phrase.contains('…') { 1.22 } else { 1.0 };
            let score = (item.occurrences as f64).ln_1p()
                * (1.0 + (item.session_count as f64).ln())
                * length_factor
                * frame_factor
                * voice_factor;
            Some((phrase, item, score))
        })
        .collect::<Vec<_>>();
    scored.sort_by(|left, right| {
        right
            .2
            .total_cmp(&left.2)
            .then_with(|| right.1.session_count.cmp(&left.1.session_count))
            .then_with(|| right.1.occurrences.cmp(&left.1.occurrences))
            .then_with(|| left.0.cmp(&right.0))
    });
    let remainder = if scored.len() > 128 {
        scored.split_off(128)
    } else {
        Vec::new()
    };
    let mut selected = HashSet::<usize>::new();
    let mut visited = vec![false; scored.len()];
    for root in 0..scored.len() {
        if visited[root] {
            continue;
        }
        visited[root] = true;
        let mut component = vec![root];
        let mut cursor = 0;
        while cursor < component.len() {
            let current = component[cursor];
            cursor += 1;
            for candidate in 0..scored.len() {
                if visited[candidate]
                    || !nested_phrase_evidence_is_redundant(
                        &scored[current].0,
                        &scored[current].1.session_ids,
                        scored[current].1.occurrences,
                        &scored[candidate].0,
                        &scored[candidate].1.session_ids,
                        scored[candidate].1.occurrences,
                    )
                {
                    continue;
                }
                visited[candidate] = true;
                component.push(candidate);
            }
        }
        let representative = component
            .into_iter()
            .max_by(|left, right| {
                phrase_is_complete(&scored[*left].0)
                    .cmp(&phrase_is_complete(&scored[*right].0))
                    .then_with(|| {
                        phrase_dedup_units(&scored[*left].0)
                            .len()
                            .cmp(&phrase_dedup_units(&scored[*right].0).len())
                    })
                    .then_with(|| scored[*left].2.total_cmp(&scored[*right].2))
                    .then_with(|| {
                        scored[*left]
                            .1
                            .occurrences
                            .cmp(&scored[*right].1.occurrences)
                    })
                    .then_with(|| scored[*right].0.cmp(&scored[*left].0))
            })
            .expect("phrase component always has a representative");
        selected.insert(representative);
    }
    scored = scored
        .into_iter()
        .enumerate()
        .filter_map(|(index, item)| selected.contains(&index).then_some(item))
        .collect();
    if scored.len() < 8 {
        for candidate in remainder {
            let redundant = scored.iter().any(|retained| {
                nested_phrase_evidence_is_redundant(
                    &candidate.0,
                    &candidate.1.session_ids,
                    candidate.1.occurrences,
                    &retained.0,
                    &retained.1.session_ids,
                    retained.1.occurrences,
                )
            });
            if !redundant {
                scored.push(candidate);
            }
            if scored.len() >= 8 {
                break;
            }
        }
    }
    scored.sort_by(|left, right| {
        right
            .2
            .total_cmp(&left.2)
            .then_with(|| right.1.session_count.cmp(&left.1.session_count))
            .then_with(|| right.1.occurrences.cmp(&left.1.occurrences))
            .then_with(|| left.0.cmp(&right.0))
    });
    scored.truncate(8);

    let minimum = scored
        .iter()
        .map(|item| (item.1.occurrences as f64).ln_1p())
        .reduce(f64::min)
        .unwrap_or(0.0);
    let maximum = scored
        .iter()
        .map(|item| (item.1.occurrences as f64).ln_1p())
        .reduce(f64::max)
        .unwrap_or(0.0);
    let items = scored
        .into_iter()
        .map(|(phrase, item, _score)| {
            let frequency = (item.occurrences as f64).ln_1p();
            let weight = if maximum > minimum {
                0.36 + ((frequency - minimum) / (maximum - minimum)) * 0.64
            } else {
                0.68
            };
            let agents = item
                .agents
                .into_iter()
                .map(|(agent, (occurrences, session_count))| PhraseAgentCount {
                    agent,
                    occurrences,
                    session_count,
                })
                .collect::<Vec<_>>();
            let models = item
                .models
                .into_iter()
                .map(|(model, (occurrences, session_count))| PhraseModelCount {
                    model,
                    occurrences,
                    session_count,
                })
                .collect::<Vec<_>>();
            let dominant_agent = if role == "agent" {
                agents
                    .iter()
                    .max_by(|left, right| {
                        left.occurrences
                            .cmp(&right.occurrences)
                            .then_with(|| right.agent.cmp(&left.agent))
                    })
                    .map(|item| item.agent.clone())
            } else {
                None
            };
            let dominant_model = if role == "agent" {
                models
                    .iter()
                    .max_by(|left, right| {
                        left.occurrences
                            .cmp(&right.occurrences)
                            .then_with(|| right.model.cmp(&left.model))
                    })
                    .map(|item| item.model.clone())
            } else {
                None
            };
            PhraseCloudItem {
                phrase,
                occurrences: item.occurrences,
                session_count: item.session_count,
                weight,
                dominant_agent,
                dominant_model,
                agents,
                models,
            }
        })
        .collect::<Vec<_>>();
    Ok(PhraseCloud {
        status: if sample_sessions >= 2 && !items.is_empty() {
            "ready".into()
        } else {
            "insufficient-data".into()
        },
        sample_sessions,
        items,
    })
}

fn nested_phrase_evidence_is_redundant(
    left_phrase: &str,
    left_sessions: &HashSet<String>,
    left_occurrences: u64,
    right_phrase: &str,
    right_sessions: &HashSet<String>,
    right_occurrences: u64,
) -> bool {
    let left_units = phrase_dedup_units(left_phrase);
    let right_units = phrase_dedup_units(right_phrase);
    let nested = contiguous_units_contain(&left_units, &right_units)
        || contiguous_units_contain(&right_units, &left_units);
    if !nested || left_units == right_units {
        return false;
    }

    let smaller_sessions = left_sessions.len().min(right_sessions.len());
    if smaller_sessions < 2 {
        return false;
    }
    let intersection = left_sessions.intersection(right_sessions).count();
    let union = left_sessions.union(right_sessions).count();
    let sessions_substantially_overlap =
        intersection * 10 >= smaller_sessions * 9 && intersection * 100 >= union * 82;
    let smaller_occurrences = left_occurrences.min(right_occurrences);
    let larger_occurrences = left_occurrences.max(right_occurrences);
    let occurrences_are_comparable = larger_occurrences > 0
        && smaller_occurrences.saturating_mul(4) >= larger_occurrences.saturating_mul(3);
    sessions_substantially_overlap && occurrences_are_comparable
}

fn phrase_dedup_units(phrase: &str) -> Vec<String> {
    if phrase.chars().any(|character| {
        matches!(
            character,
            '\u{3400}'..='\u{4DBF}'
                | '\u{4E00}'..='\u{9FFF}'
                | '\u{F900}'..='\u{FAFF}'
        )
    }) {
        return phrase
            .chars()
            .filter(|character| character.is_alphanumeric())
            .map(|character| character.to_string())
            .collect();
    }
    phrase
        .split_whitespace()
        .map(|token| {
            token
                .trim_matches(|character: char| {
                    !character.is_ascii_alphanumeric() && character != '\''
                })
                .to_ascii_lowercase()
        })
        .filter(|token| !token.is_empty())
        .collect()
}

fn contiguous_units_contain(longer: &[String], shorter: &[String]) -> bool {
    !shorter.is_empty()
        && shorter.len() < longer.len()
        && longer
            .windows(shorter.len())
            .any(|window| window == shorter)
}

fn phrase_is_complete(phrase: &str) -> bool {
    let units = phrase_dedup_units(phrase);
    let Some(last) = units.last() else {
        return false;
    };
    !matches!(
        last.as_str(),
        "a" | "an"
            | "the"
            | "to"
            | "for"
            | "of"
            | "and"
            | "or"
            | "with"
            | "from"
            | "in"
            | "on"
            | "at"
            | "by"
    )
}

fn phrase_voice_factor(phrase: &str) -> Option<f64> {
    if phrase.contains('…') {
        return Some(1.9);
    }
    let chinese_characters = phrase
        .chars()
        .filter(|character| {
            matches!(
                character,
                '\u{3400}'..='\u{4DBF}'
                    | '\u{4E00}'..='\u{9FFF}'
                    | '\u{F900}'..='\u{FAFF}'
            )
        })
        .count();
    if chinese_characters > 0 {
        if chinese_characters < 3 {
            return None;
        }
        if [
            "把", "按", "用", "对", "将", "在", "的", "地", "得", "和", "与", "及", "或", "为",
            "向", "从", "给", "让", "这", "那", "我", "你",
        ]
        .iter()
        .any(|suffix| phrase.ends_with(suffix))
            || ["接下来", "我现在", "我已经", "现在我", "如果你"].contains(&phrase)
        {
            return None;
        }
        if phrase.starts_with("接下来我会") || phrase.starts_with("下一步我会") {
            return Some(1.8);
        }
        if phrase.starts_with("我会先") {
            return Some(1.75);
        }
        if phrase.starts_with("我会继续") || phrase.starts_with("如果你愿意") {
            return Some(1.55);
        }
        if phrase.contains('我') || phrase.contains('你') {
            return Some(1.3);
        }
        if [
            "先", "继续", "已经", "现在", "可以", "需要", "建议", "直接", "确认", "验证", "检查",
            "完成", "好的", "收到", "明白", "开始",
        ]
        .iter()
        .any(|prefix| phrase.starts_with(prefix))
        {
            return Some(1.12);
        }
        None
    } else {
        let tokens = phrase
            .split_whitespace()
            .map(|token| {
                token
                    .trim_matches(|character: char| {
                        !character.is_ascii_alphabetic() && character != '\''
                    })
                    .to_ascii_lowercase()
            })
            .filter(|token| !token.is_empty())
            .collect::<Vec<_>>();
        if tokens.len() < 2 {
            return None;
        }
        if tokens.iter().any(|token| {
            matches!(
                token.as_str(),
                "i" | "i'll" | "we" | "you" | "my" | "your" | "let" | "let's"
            )
        }) {
            return Some(1.35);
        }
        if tokens.first().is_some_and(|token| {
            matches!(
                token.as_str(),
                "first"
                    | "next"
                    | "now"
                    | "okay"
                    | "sure"
                    | "please"
                    | "run"
                    | "check"
                    | "verify"
                    | "fix"
                    | "update"
                    | "continue"
                    | "make"
                    | "keep"
                    | "use"
                    | "open"
                    | "review"
                    | "test"
                    | "confirm"
                    | "try"
            )
        }) {
            return Some(1.12);
        }
        None
    }
}

fn query_overview_totals(
    connection: &Connection,
    start_timestamp: &str,
    start_date: &str,
) -> AppResult<OverviewTotals> {
    let usage_row = connection.query_row(
        "SELECT COUNT(DISTINCT session_id),
                COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
                COALESCE(SUM(cache_read_tokens),0), COALESCE(SUM(cache_write_tokens),0),
                COALESCE(SUM(cache_write_1h_tokens),0), COALESCE(SUM(reasoning_tokens),0)
         FROM daily_usage
         WHERE date >= ?1
           AND (
                NOT EXISTS(SELECT 1 FROM sources)
                OR agent IN (
                    SELECT agent FROM sources WHERE available=1 AND selected=1
                )
           )",
        params![start_date],
        |row| {
            Ok((
                read_u64(row, 0)?,
                TokenUsage {
                    input_tokens: read_u64(row, 1)?,
                    output_tokens: read_u64(row, 2)?,
                    cache_read_tokens: read_u64(row, 3)?,
                    cache_write_tokens: read_u64(row, 4)?,
                    cache_write_1h_tokens: read_u64(row, 5)?,
                    reasoning_tokens: read_u64(row, 6)?,
                },
            ))
        },
    )?;
    let (estimated_cost_usd, cost_coverage_tokens) = query_overview_cost(connection, start_date)?;
    let evidence_row = connection.query_row(
        "SELECT
                COALESCE(SUM(CASE WHEN files_touched > 0 THEN 1 ELSE 0 END),0),
                COALESCE(SUM(CASE WHEN files_touched > 0 AND verification_events > 0 THEN 1 ELSE 0 END),0),
                COALESCE(MAX(longest_uninterrupted_seconds),0),
                COALESCE(SUM(lines_added),0), COALESCE(SUM(lines_deleted),0),
                COALESCE(SUM(retries),0)
         FROM sessions s
         WHERE s.started_at >= ?1
           AND (
                NOT EXISTS(SELECT 1 FROM sources)
                OR s.agent IN (
                    SELECT agent FROM sources WHERE available=1 AND selected=1
                )
           )",
        params![start_timestamp],
        |row| {
            Ok((
                read_u64(row, 0)?,
                read_u64(row, 1)?,
                read_u64(row, 2)?,
                read_u64(row, 3)?,
                read_u64(row, 4)?,
                read_u64(row, 5)?,
            ))
        },
    )?;
    let session_row = connection.query_row(
        "SELECT COUNT(*), COALESCE(SUM(active_seconds),0),
                COUNT(DISTINCT substr(started_at,1,10)), COALESCE(SUM(errors),0)
         FROM sessions s
         WHERE s.started_at >= ?1
           AND (
                NOT EXISTS(SELECT 1 FROM sources)
                OR s.agent IN (
                    SELECT agent FROM sources WHERE available=1 AND selected=1
                )
           )",
        params![start_timestamp],
        |row| {
            Ok((
                read_u64(row, 0)?,
                read_u64(row, 1)?,
                read_u64(row, 2)?,
                read_u64(row, 3)?,
            ))
        },
    )?;
    let files_touched = connection.query_row(
        "SELECT COUNT(DISTINCT sf.file_hash)
         FROM session_files sf JOIN sessions s ON s.id=sf.session_id
         WHERE s.started_at >= ?1
           AND (
                NOT EXISTS(SELECT 1 FROM sources)
                OR s.agent IN (
                    SELECT agent FROM sources WHERE available=1 AND selected=1
                )
           )",
        params![start_timestamp],
        |result| read_u64(result, 0),
    )?;
    let total_tokens = usage_row.1.total();
    let cost_coverage = if total_tokens == 0 {
        0.0
    } else {
        (cost_coverage_tokens as f64 / total_tokens as f64).clamp(0.0, 1.0)
    };
    let verification_rate = if evidence_row.0 == 0 {
        None
    } else {
        Some(evidence_row.1 as f64 / evidence_row.0 as f64)
    };
    Ok(OverviewTotals {
        // A session can begin before the selected local-day boundary while
        // still producing observed usage inside it. Count the sessions that
        // contributed daily usage to the range, matching the token totals.
        session_count: usage_row.0,
        active_seconds: session_row.1,
        active_days: session_row.2,
        usage: usage_row.1,
        estimated_cost_usd,
        cost_coverage,
        verification_rate,
        longest_uninterrupted_seconds: evidence_row.2,
        files_touched,
        lines_added: evidence_row.3,
        lines_deleted: evidence_row.4,
        errors: session_row.3,
        retries: evidence_row.5,
    })
}

fn query_overview_cost(connection: &Connection, start_date: &str) -> AppResult<(Option<f64>, u64)> {
    let mut statement = connection.prepare(
        "SELECT agent, model, SUM(estimated_cost_usd),
                COALESCE(SUM(CASE WHEN estimated_cost_usd IS NOT NULL THEN
                    input_tokens + output_tokens + cache_read_tokens + cache_write_tokens + cache_write_1h_tokens
                ELSE 0 END),0),
                COALESCE(SUM(CASE WHEN estimated_cost_usd IS NULL THEN input_tokens ELSE 0 END),0),
                COALESCE(SUM(CASE WHEN estimated_cost_usd IS NULL THEN output_tokens ELSE 0 END),0),
                COALESCE(SUM(CASE WHEN estimated_cost_usd IS NULL THEN cache_read_tokens ELSE 0 END),0),
                COALESCE(SUM(CASE WHEN estimated_cost_usd IS NULL THEN cache_write_tokens ELSE 0 END),0),
                COALESCE(SUM(CASE WHEN estimated_cost_usd IS NULL THEN cache_write_1h_tokens ELSE 0 END),0)
         FROM daily_usage
         WHERE date >= ?1
           AND (
                NOT EXISTS(SELECT 1 FROM sources)
                OR agent IN (
                    SELECT agent FROM sources WHERE available=1 AND selected=1
                )
           )
         GROUP BY agent, model",
    )?;
    let rows = statement.query_map(params![start_date], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<f64>>(2)?,
            read_u64(row, 3)?,
            TokenUsage {
                input_tokens: read_u64(row, 4)?,
                output_tokens: read_u64(row, 5)?,
                cache_read_tokens: read_u64(row, 6)?,
                cache_write_tokens: read_u64(row, 7)?,
                cache_write_1h_tokens: read_u64(row, 8)?,
                reasoning_tokens: 0,
            },
        ))
    })?;

    let mut total_cost = 0.0;
    let mut has_cost = false;
    let mut coverage_tokens = 0_u64;
    for row in rows {
        let (agent, model, stored_cost, stored_coverage_tokens, missing_usage) = row?;
        if let Some(cost) = stored_cost {
            total_cost += cost;
            has_cost = true;
            coverage_tokens = coverage_tokens.saturating_add(stored_coverage_tokens);
        }
        if missing_usage.total() > 0
            && let Some(agent) = stored_agent_kind(&agent)
            && let Some(cost) = crate::pricing::estimate_cost(agent, &model, &missing_usage)
        {
            total_cost += cost;
            has_cost = true;
            coverage_tokens = coverage_tokens.saturating_add(missing_usage.total());
        }
    }
    Ok((has_cost.then_some(total_cost), coverage_tokens))
}

fn stored_agent_kind(agent: &str) -> Option<AgentKind> {
    match agent {
        "claude-code" => Some(AgentKind::ClaudeCode),
        "codex" => Some(AgentKind::Codex),
        "kimi-code" => Some(AgentKind::KimiCode),
        "cursor" => Some(AgentKind::Cursor),
        "openclaw" => Some(AgentKind::OpenClaw),
        "hermes" => Some(AgentKind::Hermes),
        "zcode" => Some(AgentKind::ZCode),
        _ => None,
    }
}

fn query_daily(connection: &Connection, start_date: &str) -> AppResult<Vec<DailyUsagePoint>> {
    let mut statement = connection.prepare(
        "WITH daily_rows AS (
            SELECT session_id, date, agent, model,
                   input_tokens, output_tokens, cache_read_tokens,
                   cache_write_tokens, cache_write_1h_tokens, reasoning_tokens,
                   active_seconds, tool_calls, errors, estimated_cost_usd
            FROM daily_usage
            WHERE date >= ?1
              AND (
                    NOT EXISTS(SELECT 1 FROM sources)
                    OR agent IN (
                        SELECT agent FROM sources WHERE available=1 AND selected=1
                    )
              )
            UNION ALL
            SELECT s.id, substr(s.started_at,1,10), s.agent, COALESCE(NULLIF(s.model,''),'unknown'),
                   0, 0, 0, 0, 0, 0,
                   s.active_seconds, s.tool_calls, s.errors, s.estimated_cost_usd
            FROM sessions s
            WHERE s.started_at >= ?2
              AND (
                    NOT EXISTS(SELECT 1 FROM sources)
                    OR s.agent IN (
                        SELECT agent FROM sources WHERE available=1 AND selected=1
                    )
              )
              AND NOT EXISTS(SELECT 1 FROM daily_usage du WHERE du.session_id=s.id)
         )
         SELECT date, agent, model,
                SUM(input_tokens), SUM(output_tokens), SUM(cache_read_tokens),
                SUM(cache_write_tokens), SUM(cache_write_1h_tokens), SUM(reasoning_tokens),
                SUM(active_seconds), COUNT(DISTINCT session_id), SUM(tool_calls),
                SUM(errors), SUM(estimated_cost_usd)
         FROM daily_rows
         GROUP BY date, agent, model ORDER BY date, agent, model",
    )?;
    Ok(statement
        .query_map(
            params![start_date, format!("{start_date}T00:00:00Z")],
            daily_from_row,
        )?
        .collect::<Result<Vec<_>, _>>()?)
}

fn query_hourly(connection: &Connection, start_date: &str) -> AppResult<Vec<HourlyUsagePoint>> {
    let start_hour = format!("{start_date}T00:00");
    let mut statement = connection.prepare(
        "SELECT hour, agent, model,
                SUM(input_tokens), SUM(output_tokens), SUM(cache_read_tokens),
                SUM(cache_write_tokens), SUM(cache_write_1h_tokens), SUM(reasoning_tokens)
         FROM hourly_usage
         WHERE hour >= ?1
           AND (
                NOT EXISTS(SELECT 1 FROM sources)
                OR agent IN (
                    SELECT agent FROM sources WHERE available=1 AND selected=1
                )
           )
         GROUP BY hour, agent, model ORDER BY hour, agent, model",
    )?;
    Ok(statement
        .query_map(params![start_hour], |row| {
            Ok(HourlyUsagePoint {
                hour: row.get(0)?,
                agent: row.get(1)?,
                model: row.get(2)?,
                usage: TokenUsage {
                    input_tokens: read_u64(row, 3)?,
                    output_tokens: read_u64(row, 4)?,
                    cache_read_tokens: read_u64(row, 5)?,
                    cache_write_tokens: read_u64(row, 6)?,
                    cache_write_1h_tokens: read_u64(row, 7)?,
                    reasoning_tokens: read_u64(row, 8)?,
                },
            })
        })?
        .collect::<Result<Vec<_>, _>>()?)
}

fn query_daily_for_session(connection: &Connection, id: &str) -> AppResult<Vec<DailyUsagePoint>> {
    let mut statement = connection.prepare(
        "SELECT date, agent, model, input_tokens, output_tokens, cache_read_tokens,
                cache_write_tokens, cache_write_1h_tokens, reasoning_tokens,
                active_seconds, 1, tool_calls, errors, estimated_cost_usd
         FROM daily_usage WHERE session_id=?1 ORDER BY date, model",
    )?;
    Ok(statement
        .query_map(params![id], daily_from_row)?
        .collect::<Result<Vec<_>, _>>()?)
}

fn query_events(connection: &Connection, session_id: &str) -> AppResult<Vec<CanonicalEvent>> {
    let mut statement = connection.prepare(
        "SELECT sequence, occurred_at, event_type, category, name, success,
                duration_ms, provenance
         FROM events WHERE session_id=?1 ORDER BY sequence",
    )?;
    Ok(statement
        .query_map(params![session_id], |row| {
            Ok(CanonicalEvent {
                sequence: read_u64(row, 0)?,
                occurred_at: row.get(1)?,
                event_type: row.get(2)?,
                category: row.get(3)?,
                name: row.get(4)?,
                success: row.get::<_, Option<i64>>(5)?.map(|value| value != 0),
                duration_ms: row
                    .get::<_, Option<i64>>(6)?
                    .map(|value| value.max(0) as u64),
                provenance: row.get(7)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?)
}

fn phase_for_event(event: &CanonicalEvent) -> &'static str {
    match event.category.as_str() {
        "understand" => "understand",
        "read" | "search" | "web" => "inspect",
        "edit" => "edit",
        "test" | "build" | "lint" | "typecheck" | "git-review" => "verify",
        "error" => "fix",
        "subagent" => "plan",
        _ => "execute",
    }
}

fn derive_process_phases(events: Vec<CanonicalEvent>) -> Vec<ProcessPhase> {
    let mut phases = Vec::<ProcessPhase>::new();
    for event in events {
        let phase_key = phase_for_event(&event).to_string();
        let append = phases
            .last()
            .is_some_and(|phase| phase.phase_key == phase_key);
        if append {
            if let Some(phase) = phases.last_mut() {
                phase.ended_at = event.occurred_at.clone().or_else(|| phase.ended_at.clone());
                phase.event_count = phase.event_count.saturating_add(1);
                phase.events.push(event);
            }
        } else {
            let sequence = phases.len() + 1;
            phases.push(ProcessPhase {
                id: format!("{phase_key}-{sequence}"),
                phase_key,
                started_at: event.occurred_at.clone(),
                ended_at: event.occurred_at.clone(),
                event_count: 1,
                provenance: "derived".into(),
                events: vec![event],
            });
        }
    }
    phases
}

fn query_file_changes(connection: &Connection, session_id: &str) -> AppResult<Vec<FileChange>> {
    let mut statement = connection.prepare(
        "SELECT fc.path, fc.change_kind, fc.lines_added, fc.lines_deleted,
                fc.modification_count,
                CASE WHEN EXISTS(
                    SELECT 1 FROM git_files gf
                    WHERE gf.session_id=fc.session_id AND gf.path=fc.path
                ) THEN 'committed' ELSE fc.final_state END
         FROM file_changes fc WHERE fc.session_id=?1
         ORDER BY fc.modification_count DESC, fc.path",
    )?;
    Ok(statement
        .query_map(params![session_id], |row| {
            let path: String = row.get(0)?;
            Ok(FileChange {
                id: crate::privacy::stable_hash(&format!("{session_id}:{path}")),
                path,
                change_kind: row.get(1)?,
                lines_added: read_u64(row, 2)?,
                lines_deleted: read_u64(row, 3)?,
                modification_count: read_u64(row, 4)?,
                final_state: row.get(5)?,
                provenance: "observed".into(),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?)
}

fn query_git_evidence(connection: &Connection, session_id: &str) -> AppResult<GitEvidence> {
    let row = connection
        .query_row(
            "SELECT available, state, branch FROM git_evidence WHERE session_id=?1",
            params![session_id],
            |row| {
                Ok(GitEvidence {
                    available: row.get::<_, i64>(0)? != 0,
                    state: row.get(1)?,
                    branch: row.get(2)?,
                    commits: Vec::new(),
                })
            },
        )
        .optional()?;
    let mut evidence = row.unwrap_or_else(|| GitEvidence {
        available: false,
        state: "not-detected".into(),
        ..GitEvidence::default()
    });
    let mut commit_statement = connection.prepare(
        "SELECT hash, subject, committed_at FROM git_commits
         WHERE session_id=?1 ORDER BY committed_at DESC",
    )?;
    let commits = commit_statement
        .query_map(params![session_id], |row| {
            Ok(GitCommitEvidence {
                hash: row.get(0)?,
                subject: row.get(1)?,
                committed_at: row.get(2)?,
                files: Vec::new(),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for mut commit in commits {
        let mut file_statement = connection.prepare(
            "SELECT path, lines_added, lines_deleted FROM git_files
             WHERE session_id=?1 AND commit_hash=?2 ORDER BY path",
        )?;
        commit.files = file_statement
            .query_map(params![session_id, commit.hash], |row| {
                Ok(GitFileStat {
                    path: row.get(0)?,
                    lines_added: read_u64(row, 1)?,
                    lines_deleted: read_u64(row, 2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        evidence.commits.push(commit);
    }
    Ok(evidence)
}

fn query_task_for_session(
    connection: &Connection,
    session_id: &str,
) -> AppResult<Option<TaskSummary>> {
    let task_id = connection
        .query_row(
            "SELECT task_id FROM task_sessions WHERE session_id=?1",
            params![session_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let Some(task_id) = task_id else {
        return Ok(None);
    };
    Ok(query_tasks(connection, "1970-01-01T00:00:00Z", 10_000)?
        .into_iter()
        .find(|task| task.id == task_id))
}

fn capabilities_for_agent(agent: &str) -> Vec<String> {
    let capabilities = match agent {
        "claude-code" | "codex" => &[
            "session_timestamps",
            "model_name",
            "token_usage",
            "cost",
            "tool_calls",
            "file_modifications",
            "file_paths",
            "commands",
            "test_runs",
            "build_runs",
            "lint_runs",
            "typecheck_runs",
            "errors",
            "retries",
            "user_interventions",
            "subagent_lifecycle",
        ][..],
        "kimi-code" => &["session_timestamps", "model_name", "token_usage"][..],
        "zcode" => &[
            "session_timestamps",
            "model_name",
            "token_usage",
            "tool_calls",
            "errors",
            "user_interventions",
        ][..],
        _ => &[][..],
    };
    capabilities.iter().map(|value| (*value).into()).collect()
}

fn daily_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DailyUsagePoint> {
    Ok(DailyUsagePoint {
        date: row.get(0)?,
        agent: row.get(1)?,
        model: row.get(2)?,
        usage: TokenUsage {
            input_tokens: read_u64(row, 3)?,
            output_tokens: read_u64(row, 4)?,
            cache_read_tokens: read_u64(row, 5)?,
            cache_write_tokens: read_u64(row, 6)?,
            cache_write_1h_tokens: read_u64(row, 7)?,
            reasoning_tokens: read_u64(row, 8)?,
        },
        active_seconds: read_u64(row, 9)?,
        session_count: read_u64(row, 10)?,
        tool_calls: read_u64(row, 11)?,
        errors: read_u64(row, 12)?,
        estimated_cost_usd: row.get(13)?,
    })
}

fn query_usage_distribution(
    connection: &Connection,
    id_expression: &str,
    label_expression: &str,
    start_date: &str,
) -> AppResult<Vec<DistributionItem>> {
    let sql = format!(
        "SELECT {id_expression}, {label_expression},
                SUM(input_tokens + output_tokens + cache_read_tokens + cache_write_tokens + cache_write_1h_tokens),
                SUM(active_seconds)
         FROM daily_usage
         WHERE date >= ?1
           AND (
                NOT EXISTS(SELECT 1 FROM sources)
                OR agent IN (
                    SELECT agent FROM sources WHERE available=1 AND selected=1
                )
           )
         GROUP BY {id_expression}, {label_expression}
         ORDER BY 3 DESC LIMIT 12"
    );
    let mut statement = connection.prepare(&sql)?;
    Ok(statement
        .query_map(params![start_date], |row| {
            Ok(DistributionItem {
                id: row.get(0)?,
                label: row.get(1)?,
                value: row.get::<_, f64>(2)?,
                secondary_value: Some(row.get::<_, f64>(3)?),
                provenance: Provenance::Derived,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?)
}

fn query_tools(connection: &Connection, start_timestamp: &str) -> AppResult<Vec<DistributionItem>> {
    let mut statement = connection.prepare(
        "SELECT tu.tool, SUM(tu.count)
         FROM tool_usage tu JOIN sessions s ON s.id=tu.session_id
         WHERE s.started_at >= ?1
           AND (
                NOT EXISTS(SELECT 1 FROM sources)
                OR s.agent IN (
                    SELECT agent FROM sources WHERE available=1 AND selected=1
                )
           )
         GROUP BY tu.tool ORDER BY 2 DESC LIMIT 12",
    )?;
    Ok(statement
        .query_map(params![start_timestamp], |row| {
            Ok(DistributionItem {
                id: row.get(0)?,
                label: row.get(0)?,
                value: row.get::<_, f64>(1)?,
                secondary_value: None,
                provenance: Provenance::Observed,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?)
}

fn query_skills(connection: &Connection, start_timestamp: &str) -> AppResult<SkillUsageSummary> {
    let mut statement = connection.prepare(
        "SELECT su.skill, SUM(su.count), COUNT(DISTINCT su.session_id)
         FROM skill_usage su JOIN sessions s ON s.id=su.session_id
         WHERE s.started_at >= ?1
           AND (
                NOT EXISTS(SELECT 1 FROM sources)
                OR s.agent IN (
                    SELECT agent FROM sources WHERE available=1 AND selected=1
                )
           )
         GROUP BY su.skill",
    )?;
    let used = statement
        .query_map(params![start_timestamp], |row| {
            Ok(SkillUsageItem {
                name: row.get(0)?,
                invocation_count: read_u64(row, 1)?,
                session_count: read_u64(row, 2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(build_skill_usage_summary(
        used,
        crate::skill_usage::installed_skill_names(),
    ))
}

fn build_skill_usage_summary(
    mut used: Vec<SkillUsageItem>,
    mut installed: Vec<String>,
) -> SkillUsageSummary {
    used.sort_by(|left, right| {
        right
            .invocation_count
            .cmp(&left.invocation_count)
            .then_with(|| left.name.cmp(&right.name))
    });
    installed.sort();
    installed.dedup();
    let used_names = used
        .iter()
        .map(|item| item.name.as_str())
        .collect::<HashSet<_>>();
    let installed_without_usage = installed
        .iter()
        .filter(|name| !used_names.contains(name.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let mut least_used = used.clone();
    least_used.sort_by(|left, right| {
        left.invocation_count
            .cmp(&right.invocation_count)
            .then_with(|| left.name.cmp(&right.name))
    });
    SkillUsageSummary {
        most_used: used.iter().take(5).cloned().collect(),
        least_used: least_used.into_iter().take(5).collect(),
        installed_without_usage,
        installed_count: installed.len() as u64,
        used_count: used.len() as u64,
    }
}

fn query_tasks(
    connection: &Connection,
    start_timestamp: &str,
    limit: u64,
) -> AppResult<Vec<TaskSummary>> {
    let mut statement = connection.prepare(
        "SELECT t.id, t.title, t.project_label, t.status, t.confidence,
            t.grouping_state, t.grouping_reason_json, t.suggested_task_id,
            (SELECT COUNT(*) FROM task_sessions ts WHERE ts.task_id=t.id),
            (SELECT MIN(s.started_at) FROM sessions s JOIN task_sessions ts ON ts.session_id=s.id WHERE ts.task_id=t.id),
            (SELECT MAX(COALESCE(s.ended_at,s.started_at)) FROM sessions s JOIN task_sessions ts ON ts.session_id=s.id WHERE ts.task_id=t.id),
            (SELECT s.agent FROM sessions s JOIN task_sessions ts ON ts.session_id=s.id WHERE ts.task_id=t.id ORDER BY s.started_at DESC LIMIT 1),
            (SELECT s.model FROM sessions s JOIN task_sessions ts ON ts.session_id=s.id WHERE ts.task_id=t.id ORDER BY s.started_at DESC LIMIT 1),
            (SELECT COALESCE(SUM(s.files_touched),0) FROM sessions s JOIN task_sessions ts ON ts.session_id=s.id WHERE ts.task_id=t.id),
            (SELECT COALESCE(SUM(s.lines_added),0) FROM sessions s JOIN task_sessions ts ON ts.session_id=s.id WHERE ts.task_id=t.id),
            (SELECT COALESCE(SUM(s.lines_deleted),0) FROM sessions s JOIN task_sessions ts ON ts.session_id=s.id WHERE ts.task_id=t.id),
            (SELECT COALESCE(SUM(s.input_tokens+s.output_tokens+s.cache_read_tokens+s.cache_write_tokens+s.cache_write_1h_tokens),0) FROM sessions s JOIN task_sessions ts ON ts.session_id=s.id WHERE ts.task_id=t.id),
            EXISTS(SELECT 1 FROM git_commits gc JOIN task_sessions ts ON ts.session_id=gc.session_id WHERE ts.task_id=t.id),
            (SELECT COALESCE(SUM(s.verification_events),0) FROM sessions s JOIN task_sessions ts ON ts.session_id=s.id WHERE ts.task_id=t.id),
            (SELECT COALESCE(SUM(s.active_seconds),0) FROM sessions s JOIN task_sessions ts ON ts.session_id=s.id WHERE ts.task_id=t.id),
            (SELECT COALESCE(SUM(s.errors),0) FROM sessions s JOIN task_sessions ts ON ts.session_id=s.id WHERE ts.task_id=t.id),
            (SELECT COALESCE(SUM(s.retries),0) FROM sessions s JOIN task_sessions ts ON ts.session_id=s.id WHERE ts.task_id=t.id),
            (SELECT COALESCE(MAX(fc.modification_count),0) FROM file_changes fc JOIN task_sessions ts ON ts.session_id=fc.session_id WHERE ts.task_id=t.id),
            (SELECT s.id FROM sessions s JOIN task_sessions ts ON ts.session_id=s.id WHERE ts.task_id=t.id ORDER BY s.started_at DESC LIMIT 1),
            t.source_excluded
         FROM tasks t
         WHERE EXISTS(
            SELECT 1 FROM sessions s JOIN task_sessions ts ON ts.session_id=s.id
            WHERE ts.task_id=t.id AND s.started_at>=?1
         )
         AND (
            trim(COALESCE(t.title, '')) != ''
            OR trim(COALESCE(t.project_label, '')) != ''
            OR EXISTS(
                SELECT 1 FROM sessions s JOIN task_sessions ts ON ts.session_id=s.id
                WHERE ts.task_id=t.id AND (
                    s.input_tokens > 0 OR s.output_tokens > 0
                    OR s.cache_read_tokens > 0 OR s.cache_write_tokens > 0
                    OR s.cache_write_1h_tokens > 0 OR s.reasoning_tokens > 0
                    OR s.files_touched > 0 OR s.lines_added > 0 OR s.lines_deleted > 0
                    OR s.errors > 0 OR s.retries > 0 OR s.verification_events > 0
                    OR EXISTS(SELECT 1 FROM git_commits gc WHERE gc.session_id=s.id)
                )
            )
         )
         ORDER BY 11 DESC LIMIT ?2",
    )?;
    let rows = statement
        .query_map(params![start_timestamp, sql_i64(limit)], |row| {
            let files_changed = read_u64(row, 13)?;
            let lines_added = read_u64(row, 14)?;
            let lines_deleted = read_u64(row, 15)?;
            let total_tokens = read_u64(row, 16)?;
            let has_commit = row.get::<_, i64>(17)? != 0;
            let verification_events = read_u64(row, 18)?;
            let active_seconds = read_u64(row, 19)?;
            let errors = read_u64(row, 20)?;
            let retries = read_u64(row, 21)?;
            let max_modification_count = read_u64(row, 22)?;
            let mut reasons = Vec::new();
            if total_tokens >= 150_000 && (files_changed == 0 || lines_added + lines_deleted < 20) {
                reasons.push("today.reason.highTokenLowOutput".into());
            }
            if max_modification_count >= 5 {
                reasons.push("today.reason.repeatedFileEdits".into());
            }
            if errors >= 3 || retries >= 2 {
                reasons.push("today.reason.repeatedErrors".into());
            }
            if active_seconds >= 30 * 60
                && files_changed > 0
                && verification_events == 0
                && !has_commit
            {
                reasons.push("today.reason.longWithoutVerification".into());
            }
            let verification_state = if has_commit || verification_events > 0 {
                "verified"
            } else if files_changed > 0 {
                "unverified"
            } else {
                "not-applicable"
            };
            Ok(TaskSummary {
                id: row.get(0)?,
                title: crate::privacy::clean_display_title(&row.get::<_, String>(1)?),
                project_label: row.get(2)?,
                status: row.get(3)?,
                confidence: row.get(4)?,
                grouping_state: row.get(5)?,
                grouping_reason_keys: serde_json::from_str(&row.get::<_, String>(6)?)
                    .unwrap_or_default(),
                suggested_task_id: row.get(7)?,
                session_count: read_u64(row, 8)?,
                started_at: row.get::<_, Option<String>>(9)?.unwrap_or_default(),
                ended_at: row.get(10)?,
                agent: row.get::<_, Option<String>>(11)?.unwrap_or_default(),
                model: row.get(12)?,
                files_changed,
                lines_added,
                lines_deleted,
                total_tokens,
                has_commit,
                verification_state: verification_state.into(),
                worth_reviewing: !reasons.is_empty(),
                review_reason_keys: reasons,
                primary_session_id: row.get(23)?,
                source_excluded: row.get::<_, i64>(24)? != 0,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn playbook_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PlaybookItem> {
    Ok(PlaybookItem {
        id: row.get(0)?,
        title: row.get(1)?,
        body: row.get(2)?,
        category: row.get(3)?,
        project_label: row.get(4)?,
        task_type: row.get(5)?,
        source_review_id: row.get(6)?,
        source_finding_id: row.get(7)?,
        source_excluded: row.get::<_, i64>(8)? != 0,
        applied: row.get::<_, i64>(9)? != 0,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

fn is_routine_edit_path(path: &str) -> bool {
    let lower = path.to_lowercase();
    if lower.starts_with("[external]/") {
        return true;
    }
    let file_name = lower.rsplit(['/', '\\']).next().unwrap_or(lower.as_str());
    let ui_names = [
        "styles.css",
        "style.css",
        "globals.css",
        "index.css",
        "app.css",
        "resources.ts",
        "i18n.ts",
        "ui.tsx",
        "ui.ts",
        "appshell.tsx",
        "package-lock.json",
        "pnpm-lock.yaml",
        "yarn.lock",
        "cargo.lock",
    ];
    if ui_names.contains(&file_name) {
        return true;
    }
    lower.ends_with(".css")
        || lower.ends_with(".scss")
        || lower.ends_with(".less")
        || lower.contains("/i18n/")
        || lower.contains("/locales/")
        || lower.contains("/styles/")
        || lower.ends_with(".lock")
}

fn query_comparison(
    connection: &Connection,
    start_timestamp: &str,
    group_kind: &str,
) -> AppResult<Vec<ComparisonItem>> {
    let (group_expression, label_expression) = match group_kind {
        "agent" => ("agent", "agent"),
        "model" => ("COALESCE(model, 'unknown')", "COALESCE(model, 'unknown')"),
        _ => return Err(AppError::InvalidRequest("unknown comparison group".into())),
    };
    let sql = format!(
        "SELECT {group_expression}, {label_expression},
                MIN(agent), CASE WHEN ?2='model' THEN {group_expression} ELSE NULL END,
                COUNT(*), COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
                COALESCE(SUM(cache_read_tokens),0), COALESCE(SUM(cache_write_tokens),0),
                COALESCE(SUM(cache_write_1h_tokens),0), COALESCE(SUM(reasoning_tokens),0),
                COALESCE(SUM(active_seconds),0), COALESCE(SUM(files_touched),0),
                COALESCE(SUM(lines_added),0), COALESCE(SUM(lines_deleted),0),
                SUM(estimated_cost_usd), COALESCE(SUM(cost_coverage_tokens),0)
         FROM sessions WHERE started_at >= ?1
         GROUP BY {group_expression}, {label_expression}
         ORDER BY SUM(input_tokens + output_tokens + cache_read_tokens + cache_write_tokens + cache_write_1h_tokens) DESC"
    );
    let mut statement = connection.prepare(&sql)?;
    let mut rows = statement
        .query_map(params![start_timestamp, group_kind], |row| {
            let usage = TokenUsage {
                input_tokens: read_u64(row, 5)?,
                output_tokens: read_u64(row, 6)?,
                cache_read_tokens: read_u64(row, 7)?,
                cache_write_tokens: read_u64(row, 8)?,
                cache_write_1h_tokens: read_u64(row, 9)?,
                reasoning_tokens: read_u64(row, 10)?,
            };
            let covered = read_u64(row, 16)?;
            let id_value: String = row.get(0)?;
            Ok(ComparisonItem {
                id: if group_kind == "model" {
                    format!("model:{id_value}")
                } else {
                    id_value
                },
                group_kind: group_kind.into(),
                agent: row.get(2)?,
                model: row.get(3)?,
                label: row.get(1)?,
                session_count: read_u64(row, 4)?,
                usage: usage.clone(),
                active_seconds: read_u64(row, 11)?,
                files_touched: read_u64(row, 12)?,
                lines_added: read_u64(row, 13)?,
                lines_deleted: read_u64(row, 14)?,
                estimated_cost_usd: row.get(15)?,
                cost_coverage: if usage.total() == 0 {
                    0.0
                } else {
                    (covered as f64 / usage.total() as f64).clamp(0.0, 1.0)
                },
                usage_share: 0.0,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let total = rows.iter().map(|item| item.usage.total()).sum::<u64>();
    if total > 0 {
        for item in &mut rows {
            item.usage_share = item.usage.total() as f64 / total as f64;
        }
    }
    Ok(rows)
}

fn session_list_where_clause() -> &'static str {
    "COALESCE(ended_at, started_at) >= ?1
        AND (?2='' OR agent=?2)
        AND (?3='' OR COALESCE(title,'') LIKE ?4 OR COALESCE(model,'') LIKE ?4
            OR COALESCE(project_label,'') LIKE ?4
            OR EXISTS(SELECT 1 FROM file_changes fc WHERE fc.session_id=sessions.id AND fc.path LIKE ?4))
        AND (?5='' OR COALESCE(model,'')=?5)
        AND (?6='' OR project_label=?6 OR COALESCE(project_label, project_hash)=?6
            OR (length(COALESCE(project_label, project_hash))=16
                AND substr(COALESCE(project_label, project_hash),1,6)=?6))
        AND (
            ?7='' OR
            (?7='verified' AND verification_events > 0) OR
            (?7='unverified' AND verification_events = 0
                AND (files_touched > 0 OR lines_added > 0 OR lines_deleted > 0)) OR
            (?7='not-applicable' AND verification_events = 0
                AND files_touched = 0 AND lines_added = 0 AND lines_deleted = 0)
        )
        AND (?8=0 OR errors >= 3 OR retries >= 2
            OR (active_seconds >= 1800 AND files_touched > 0 AND verification_events = 0))
        AND (?9=0 OR files_touched > 0)
        AND (?10=0 OR EXISTS(SELECT 1 FROM git_commits gc WHERE gc.session_id=sessions.id))"
}

fn query_session_rows(
    connection: &Connection,
    start_timestamp: &str,
    filters: &SessionListFilters<'_>,
    page: u64,
    page_size: u64,
) -> AppResult<(Vec<SessionSummary>, u64)> {
    let agent_filter = filters.agent.unwrap_or("");
    let search_filter = filters.search.unwrap_or("").trim();
    let search_pattern = format!("%{search_filter}%");
    let model_filter = filters.model.unwrap_or("");
    let project_filter = filters.project.unwrap_or("");
    let verification_filter = filters.verification_state.unwrap_or("");
    let attention_flag = i64::from(filters.attention_only);
    let code_flag = i64::from(filters.code_only);
    let commit_flag = i64::from(filters.commit_only);
    let base_where = session_list_where_clause();
    let total = connection.query_row(
        &format!("SELECT COUNT(*) FROM sessions WHERE {base_where}"),
        params![
            start_timestamp,
            agent_filter,
            search_filter,
            search_pattern,
            model_filter,
            project_filter,
            verification_filter,
            attention_flag,
            code_flag,
            commit_flag,
        ],
        |row| read_u64(row, 0),
    )?;
    let limit = if page_size == 0 { 8 } else { page_size };
    let offset = page.saturating_mul(limit);
    let sql = format!(
        "SELECT id, agent, model, title, COALESCE(project_label, project_hash), started_at, ended_at,
                active_seconds, input_tokens, output_tokens, cache_read_tokens,
                cache_write_tokens, cache_write_1h_tokens, reasoning_tokens,
                estimated_cost_usd, cost_coverage_tokens, tool_calls, files_touched,
                lines_added, lines_deleted, errors, retries, verification_events,
                longest_uninterrupted_seconds, subagent_count,
                EXISTS(SELECT 1 FROM git_commits gc WHERE gc.session_id=sessions.id)
         FROM sessions WHERE {base_where}
         ORDER BY COALESCE(ended_at, started_at) DESC, started_at DESC LIMIT ?11 OFFSET ?12"
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement
        .query_map(
            params![
                start_timestamp,
                agent_filter,
                search_filter,
                search_pattern,
                model_filter,
                project_filter,
                verification_filter,
                attention_flag,
                code_flag,
                commit_flag,
                sql_i64(limit),
                sql_i64(offset)
            ],
            session_from_row,
        )?
        .collect::<Result<Vec<_>, _>>()?;
    Ok((rows, total))
}

fn query_session_facets(
    connection: &Connection,
    start_timestamp: &str,
    agent: Option<&str>,
) -> AppResult<(Vec<String>, Vec<String>)> {
    let agent_filter = agent.unwrap_or("");
    let mut model_statement = connection.prepare(
        "SELECT DISTINCT model FROM sessions
         WHERE COALESCE(ended_at, started_at) >= ?1
           AND (?2='' OR agent=?2)
           AND model IS NOT NULL AND trim(model) != ''
         ORDER BY model COLLATE NOCASE",
    )?;
    let models = model_statement
        .query_map(params![start_timestamp, agent_filter], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?;

    let mut project_statement = connection.prepare(
        "SELECT DISTINCT COALESCE(project_label, project_hash) FROM sessions
         WHERE COALESCE(ended_at, started_at) >= ?1
           AND (?2='' OR agent=?2)
           AND COALESCE(project_label, project_hash) IS NOT NULL
           AND trim(COALESCE(project_label, project_hash)) != ''
         ORDER BY 1 COLLATE NOCASE",
    )?;
    let mut projects = project_statement
        .query_map(params![start_timestamp, agent_filter], |row| {
            Ok(display_project_label(&row.get::<_, String>(0)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    projects.sort_by_key(|left| left.to_lowercase());
    projects.dedup();
    Ok((models, projects))
}

fn display_project_label(value: &str) -> String {
    if value.len() == 16 && value.chars().all(|item| item.is_ascii_hexdigit()) {
        value.chars().take(6).collect()
    } else {
        value.to_string()
    }
}

fn session_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionSummary> {
    let usage = TokenUsage {
        input_tokens: read_u64(row, 8)?,
        output_tokens: read_u64(row, 9)?,
        cache_read_tokens: read_u64(row, 10)?,
        cache_write_tokens: read_u64(row, 11)?,
        cache_write_1h_tokens: read_u64(row, 12)?,
        reasoning_tokens: read_u64(row, 13)?,
    };
    let cost_coverage_tokens = read_u64(row, 15)?;
    let total_tokens = usage.total();
    let files_touched = read_u64(row, 17)?;
    let lines_added = read_u64(row, 18)?;
    let lines_deleted = read_u64(row, 19)?;
    let verification_events = read_u64(row, 22)?;
    let verification_state = derive_verification_state(
        files_touched,
        lines_added,
        lines_deleted,
        verification_events,
    );
    let project_value = row.get::<_, Option<String>>(4)?.unwrap_or_default();
    Ok(SessionSummary {
        id: row.get(0)?,
        agent: row.get(1)?,
        model: row.get(2)?,
        title: crate::privacy::clean_display_title(
            &row.get::<_, Option<String>>(3)?.unwrap_or_default(),
        ),
        project_label: display_project_label(&project_value),
        started_at: row.get(5)?,
        ended_at: row.get(6)?,
        active_seconds: read_u64(row, 7)?,
        usage,
        estimated_cost_usd: row.get(14)?,
        cost_coverage: if total_tokens == 0 {
            0.0
        } else {
            (cost_coverage_tokens as f64 / total_tokens as f64).clamp(0.0, 1.0)
        },
        tool_calls: read_u64(row, 16)?,
        files_touched,
        lines_added,
        lines_deleted,
        errors: read_u64(row, 20)?,
        retries: read_u64(row, 21)?,
        verification_state: verification_state.into(),
        longest_uninterrupted_seconds: read_u64(row, 23)?,
        subagent_count: read_u64(row, 24)?,
        has_commit: row.get::<_, i64>(25)? != 0,
        provenance: Provenance::Observed,
    })
}

fn derive_verification_state(
    files_touched: u64,
    lines_added: u64,
    lines_deleted: u64,
    verification_events: u64,
) -> &'static str {
    if verification_events > 0 {
        "verified"
    } else if files_touched > 0 || lines_added > 0 || lines_deleted > 0 {
        "unverified"
    } else {
        "not-applicable"
    }
}

fn sql_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn read_u64(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    Ok(row.get::<_, i64>(index)?.max(0) as u64)
}

fn query_behavior_summary(
    connection: &Connection,
    start_timestamp: &str,
) -> AppResult<BehaviorSummary> {
    let mut statement = connection.prepare(
        "SELECT s.started_at, s.agent, s.parser_version, sb.behavior_json
         FROM sessions s
         LEFT JOIN session_behavior sb ON sb.session_id=s.id
         WHERE s.started_at>=?1
           AND (
                NOT EXISTS(SELECT 1 FROM sources)
                OR s.agent IN (
                    SELECT agent FROM sources WHERE available=1 AND selected=1
                )
           )",
    )?;
    let rows = statement
        .query_map(params![start_timestamp], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut summary = BehaviorSummary::default();
    let mut active_days = HashSet::new();
    let mut structure_capable = 0_u64;
    let mut lifecycle_capable = 0_u64;
    let mut tool_result_capable = 0_u64;
    let mut orchestration_capable = 0_u64;
    let mut process_capable = 0_u64;
    for (started_at, agent, parser_version, behavior_json) in rows {
        summary.sessions = summary.sessions.saturating_add(1);
        active_days.insert(started_at.chars().take(10).collect::<String>());
        let parser_current = parser_version
            .split('.')
            .next()
            .and_then(|major| major.parse::<u64>().ok())
            .is_some_and(|major| major >= 4);
        if parser_current {
            structure_capable = structure_capable.saturating_add(1);
            tool_result_capable = tool_result_capable.saturating_add(1);
            process_capable = process_capable.saturating_add(1);
            if agent == "codex" || agent == "claude-code" {
                lifecycle_capable = lifecycle_capable.saturating_add(1);
            }
            if agent == "codex" || agent == "kimi-code" {
                orchestration_capable = orchestration_capable.saturating_add(1);
            }
        }
        let behavior = behavior_json
            .as_deref()
            .and_then(|json| serde_json::from_str::<BehaviorSignals>(json).ok())
            .unwrap_or_default();
        summary.prompt_count = summary.prompt_count.saturating_add(behavior.prompt_count);
        summary.task_starts = summary.task_starts.saturating_add(behavior.task_starts);
        summary.task_completions = summary
            .task_completions
            .saturating_add(behavior.task_completions);
        summary.task_aborts = summary.task_aborts.saturating_add(behavior.task_aborts);
        summary.successful_tools = summary
            .successful_tools
            .saturating_add(behavior.successful_tools);
        summary.failed_tools = summary.failed_tools.saturating_add(behavior.failed_tools);
        summary.tool_duration_seconds = summary
            .tool_duration_seconds
            .saturating_add(behavior.tool_duration_ms / 1_000);
        summary.plan_events = summary.plan_events.saturating_add(behavior.plan_events);
        summary.goal_changes = summary.goal_changes.saturating_add(behavior.goal_changes);
        summary.context_compactions = summary
            .context_compactions
            .saturating_add(behavior.context_compactions);
        summary.rollbacks = summary.rollbacks.saturating_add(behavior.rollbacks);
        summary.subagent_starts = summary
            .subagent_starts
            .saturating_add(behavior.subagent_starts);
        summary.subagent_interactions = summary
            .subagent_interactions
            .saturating_add(behavior.subagent_interactions);
        summary.subagent_interruptions = summary
            .subagent_interruptions
            .saturating_add(behavior.subagent_interruptions);
        summary.parallel_batches = summary
            .parallel_batches
            .saturating_add(behavior.parallel_batches);
        summary.deploy_events = summary.deploy_events.saturating_add(behavior.deploy_events);
        summary.document_events = summary
            .document_events
            .saturating_add(behavior.document_events);
        summary.style_events = summary.style_events.saturating_add(behavior.style_events);
        summary.infrastructure_events = summary
            .infrastructure_events
            .saturating_add(behavior.infrastructure_events);
        summary.automation_events = summary
            .automation_events
            .saturating_add(behavior.automation_events);
        if behavior.prompt_structure_enabled {
            summary.structured_prompt_rate =
                add_rate_count(summary.structured_prompt_rate, behavior.structured_prompts);
            summary.acceptance_criteria_rate = add_rate_count(
                summary.acceptance_criteria_rate,
                behavior.acceptance_criteria_prompts,
            );
            summary.file_scope_rate =
                add_rate_count(summary.file_scope_rate, behavior.file_scope_prompts);
        }
        summary.average_task_duration_seconds = Some(
            summary.average_task_duration_seconds.unwrap_or(0.0)
                + behavior.completed_task_duration_ms as f64 / 1_000.0,
        );
    }
    summary.active_days = active_days.len() as u64;
    if summary.prompt_count > 0 {
        summary.structured_prompt_rate = summary
            .structured_prompt_rate
            .map(|count| count / summary.prompt_count as f64);
        summary.acceptance_criteria_rate = summary
            .acceptance_criteria_rate
            .map(|count| count / summary.prompt_count as f64);
        summary.file_scope_rate = summary
            .file_scope_rate
            .map(|count| count / summary.prompt_count as f64);
    } else {
        summary.structured_prompt_rate = None;
        summary.acceptance_criteria_rate = None;
        summary.file_scope_rate = None;
    }
    let tasks = summary.task_completions.saturating_add(summary.task_aborts);
    summary.completion_rate = (tasks > 0).then_some(summary.task_completions as f64 / tasks as f64);
    summary.average_task_duration_seconds = (summary.task_completions > 0).then_some(
        summary.average_task_duration_seconds.unwrap_or(0.0) / summary.task_completions as f64,
    );
    let tool_results = summary
        .successful_tools
        .saturating_add(summary.failed_tools);
    summary.tool_success_rate =
        (tool_results > 0).then_some(summary.successful_tools as f64 / tool_results as f64);
    let sessions = summary.sessions.max(1) as f64;
    summary.structure_capable_sessions = structure_capable;
    summary.lifecycle_capable_sessions = lifecycle_capable;
    summary.tool_result_capable_sessions = tool_result_capable;
    summary.orchestration_capable_sessions = orchestration_capable;
    summary.process_control_capable_sessions = process_capable;
    summary.structure_coverage = structure_capable as f64 / sessions;
    summary.lifecycle_coverage = lifecycle_capable as f64 / sessions;
    summary.tool_result_coverage = tool_result_capable as f64 / sessions;
    summary.orchestration_coverage = orchestration_capable as f64 / sessions;
    summary.process_control_coverage = process_capable as f64 / sessions;
    Ok(summary)
}

fn add_rate_count(current: Option<f64>, value: u64) -> Option<f64> {
    Some(current.unwrap_or(0.0) + value as f64)
}

#[cfg(test)]
mod concurrency_tests {
    use super::*;
    use crate::models::{DailyAggregate, ObservedLiveEvent, PhraseAggregate};
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::{Arc, Barrier};

    fn create_v13_live_database(path: &Path, incompatible_canonical_view: bool) {
        let database = Database::open(path.to_path_buf()).expect("fixture database should open");
        let now = Utc::now().to_rfc3339_opts(SecondsFormat::AutoSi, true);
        let mut state = ParseState::new(AgentKind::Codex, "legacy-session".into());
        state.started_at = Some(now.clone());
        state.ended_at = Some(now.clone());
        state.title = Some("Legacy indexed session".into());
        state.project_hash = Some("legacy-project-hash".into());
        state.project_label = Some("legacy-project".into());
        state.usage.input_tokens = 42;
        state.file_changes.insert(
            "src/legacy.rs".into(),
            crate::models::FileChangeAccumulator {
                path: "src/legacy.rs".into(),
                change_kind: "modified".into(),
                lines_added: 3,
                modification_count: 1,
                ..crate::models::FileChangeAccumulator::default()
            },
        );
        database
            .persist_parse_state("legacy-source-file", 1, 1, 1, &state)
            .expect("legacy indexed session should persist");
        database
            .record_live_event(
                &now,
                &(Utc::now() + Duration::hours(1)).to_rfc3339_opts(SecondsFormat::AutoSi, true),
                "codex",
                "legacy-session",
                "PermissionRequest",
                "legacy-project",
                "{}",
                "waiting",
            )
            .expect("legacy live event should persist");
        drop(database);

        let connection = Connection::open(path).expect("legacy database should reopen");
        connection
            .execute_batch(
                "ALTER TABLE live_events RENAME TO live_events_v14;
                 CREATE TABLE live_events (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    received_at TEXT NOT NULL,
                    expires_at TEXT NOT NULL,
                    agent TEXT NOT NULL,
                    source_session_id TEXT NOT NULL,
                    event_name TEXT NOT NULL,
                    project_label TEXT NOT NULL DEFAULT '',
                    payload_json TEXT NOT NULL,
                    status TEXT NOT NULL DEFAULT 'running'
                 );
                 INSERT INTO live_events(
                    id,
                    received_at, expires_at, agent, source_session_id,
                    event_name, project_label, payload_json, status
                 ) SELECT
                    id,
                    received_at, expires_at, agent, source_session_id,
                    event_name, project_label, payload_json, status
                   FROM live_events_v14;
                 DROP TABLE live_events_v14;
                 DROP TABLE canonical_events;
                 CREATE INDEX live_events_expiry_idx ON live_events(expires_at);
                 CREATE INDEX live_events_session_idx
                    ON live_events(agent, source_session_id, received_at);
                 CREATE INDEX live_events_status_idx ON live_events(status, received_at);
                 PRAGMA user_version = 13;",
            )
            .expect("v13 fixture schema should be restored");
        if incompatible_canonical_view {
            connection
                .execute("CREATE VIEW canonical_events AS SELECT 1 AS id", [])
                .expect("incompatible view should be created");
        }
    }

    fn assert_no_schema_migration_artifacts(path: &Path) {
        let (staging, rollback, marker) = migration_paths(path);
        let copying = rollback_copy_path(&rollback);
        for artifact in [staging, rollback, copying, marker] {
            assert!(!artifact.exists(), "migration artifact should be removed");
            assert!(!sqlite_sidecar(&artifact, "-wal").exists());
            assert!(!sqlite_sidecar(&artifact, "-shm").exists());
        }
    }

    fn notch_session(id: &str, status: &str, started_at: &str, updated_at: &str) -> LiveSession {
        LiveSession {
            id: id.into(),
            source_session_id: format!("source-{id}"),
            agent: "codex".into(),
            project_label: format!("project-{id}"),
            conversation_title: None,
            status: status.into(),
            phase: if status == "completed" {
                "completed".into()
            } else {
                "thinking".into()
            },
            started_at: started_at.into(),
            updated_at: updated_at.into(),
            waiting_reason: None,
            actions: Vec::new(),
            process_id: None,
            origin: Some("desktop".into()),
            jump_context: None,
        }
    }

    #[test]
    fn derives_verification_state_from_observed_evidence() {
        assert_eq!(derive_verification_state(0, 0, 0, 1), "verified");
        assert_eq!(derive_verification_state(0, 12, 0, 0), "unverified");
        assert_eq!(derive_verification_state(1, 0, 0, 0), "unverified");
        assert_eq!(derive_verification_state(0, 0, 0, 0), "not-applicable");
    }

    #[test]
    fn sources_report_the_canonical_capability_contract() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let database = Database::open(temporary.path().join("source-capabilities.sqlite"))
            .expect("database should open");

        let sources = database.sources().expect("sources should load");
        let capabilities = sources
            .iter()
            .map(|source| {
                (
                    source.agent.as_str(),
                    source.capability_level.as_str(),
                    source.live_capability.as_str(),
                    source.parser_version.as_str(),
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            capabilities,
            vec![
                ("claude-code", "full", "exact", PARSER_VERSION),
                ("codex", "full", "exact", PARSER_VERSION),
                ("kimi-code", "partial", "experimental", PARSER_VERSION),
                ("zcode", "partial", "experimental", PARSER_VERSION),
                ("cursor", "partial", "none", PARSER_VERSION),
                ("openclaw", "partial", "none", PARSER_VERSION),
                ("hermes", "partial", "none", PARSER_VERSION),
            ]
        );
    }

    #[test]
    fn source_upsert_repairs_a_stale_stored_capability() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let database = Database::open(temporary.path().join("stale-source-capability.sqlite"))
            .expect("database should open");
        database
            .upsert_source(AgentKind::Codex, "path", true, "ready")
            .expect("source should persist");
        let connection = database.connect().expect("database should connect");
        connection
            .execute(
                "UPDATE sources SET capability_level='partial' WHERE agent='codex'",
                [],
            )
            .expect("test should create stale capability");
        drop(connection);

        database
            .upsert_source(AgentKind::Codex, "path", true, "ready")
            .expect("source should update");
        let connection = database.connect().expect("database should reconnect");
        let stored: String = connection
            .query_row(
                "SELECT capability_level FROM sources WHERE agent='codex'",
                [],
                |row| row.get(0),
            )
            .expect("stored capability should load");

        assert_eq!(stored, "full");
    }

    #[test]
    fn zcode_source_selection_uses_the_canonical_registry() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let database = Database::open(temporary.path().join("zcode-selection.sqlite"))
            .expect("database should open");
        database
            .upsert_source(AgentKind::ZCode, "path", true, "ready")
            .expect("source should persist");

        database
            .set_source_selected("zcode", false)
            .expect("registered source should be selectable");

        let source = database
            .sources()
            .expect("sources should load")
            .into_iter()
            .find(|source| source.agent == "zcode")
            .expect("zcode should be present");
        assert!(!source.selected);
    }

    #[test]
    fn exact_waiting_event_is_canonical_deduplicated_and_user_visible() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let database = Database::open(temporary.path().join("canonical-waiting.sqlite"))
            .expect("database should open");
        let occurred_at =
            (Utc::now() - Duration::seconds(2)).to_rfc3339_opts(SecondsFormat::AutoSi, true);
        let first_observed_at =
            (Utc::now() - Duration::seconds(1)).to_rfc3339_opts(SecondsFormat::AutoSi, true);
        let second_observed_at = Utc::now().to_rfc3339_opts(SecondsFormat::AutoSi, true);
        let expires_at = (Utc::now() + Duration::hours(1)).to_rfc3339();
        let waiting = |observed_at: &str| ObservedLiveEvent {
            occurred_at: occurred_at.clone(),
            observed_at: observed_at.into(),
            expires_at: expires_at.clone(),
            agent: "codex".into(),
            source_session_id: "source-session".into(),
            source_event_id: Some("permission-1".into()),
            source_event_fingerprint: None,
            event_name: "PermissionRequest".into(),
            project_label: "project".into(),
            payload_json: r#"{"prompt":"do not expose","command":"rm private"}"#.into(),
            status: "waiting".into(),
            phase: Some("needs-you".into()),
        };

        database
            .record_observed_live_event(&waiting(&first_observed_at))
            .expect("first observation should persist");
        database
            .record_observed_live_event(&waiting(&second_observed_at))
            .expect("duplicate observation should be accepted");

        let connection = database.connect().expect("database should connect");
        let canonical = connection
            .query_row(
                "SELECT COUNT(*), MIN(occurred_at), MIN(observed_at),
                        MIN(protocol_version), MIN(schema_version), MIN(algorithm_version),
                        MIN(evidence_level), MIN(source_coverage), MIN(privacy_level),
                        MIN(lifecycle_status), MIN(live_phase), MIN(event_type),
                        MIN(source_event_name)
                 FROM canonical_events",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, String>(8)?,
                        row.get::<_, String>(9)?,
                        row.get::<_, String>(10)?,
                        row.get::<_, String>(11)?,
                        row.get::<_, String>(12)?,
                    ))
                },
            )
            .expect("canonical event should load");
        let payload_columns: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('canonical_events')
                 WHERE name IN ('payload_json', 'prompt', 'command', 'path', 'tool_arguments')",
                [],
                |row| row.get(0),
            )
            .expect("canonical columns should load");
        let metrics: (i64, i64) = connection
            .query_row(
                "SELECT event_count, waiting_count
                 FROM live_session_metrics
                 WHERE agent='codex' AND source_session_id='source-session'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("canonical metrics should load");
        drop(connection);

        assert_eq!(canonical.0, 1);
        assert_eq!(canonical.1, occurred_at);
        assert_eq!(canonical.2, first_observed_at);
        assert_eq!(canonical.3, "1.0.0");
        assert_eq!(canonical.4, 14);
        assert_eq!(canonical.5, "live-normalizer-1.0.0");
        assert_eq!(canonical.6, "observed");
        assert_eq!(canonical.7, "exact-lifecycle");
        assert_eq!(canonical.8, "normalized-local");
        assert_eq!(canonical.9, "waiting");
        assert_eq!(canonical.10, "needs-you");
        assert_eq!(canonical.11, "attention.waiting");
        assert_eq!(canonical.12, "PermissionRequest");
        assert_eq!(payload_columns, 0);
        assert_eq!(metrics, (1, 1));

        let activity = database.live_activity().expect("live activity should load");
        assert_eq!(activity.timeline.len(), 1);
        assert_eq!(activity.timeline[0].status, "waiting");
        assert_eq!(
            activity.timeline[0].observed_at.as_deref(),
            Some(first_observed_at.as_str())
        );
        assert_eq!(activity.history.len(), 1);
        assert_eq!(activity.history[0].status, "waiting");
        assert_eq!(
            activity.history[0].observed_at.as_deref(),
            Some(first_observed_at.as_str())
        );
    }

    #[test]
    fn stable_fingerprint_deduplicates_waiting_replays_without_source_id_or_time() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let database = Database::open(temporary.path().join("fingerprint-waiting.sqlite"))
            .expect("database should open");
        let first_time = "2026-08-09T10:00:01Z";
        let second_time = "2026-08-09T10:00:02Z";
        let event = |time: &str| ObservedLiveEvent {
            occurred_at: time.into(),
            observed_at: time.into(),
            expires_at: "2026-08-09T11:00:00Z".into(),
            agent: "claude-code".into(),
            source_session_id: "fingerprint-session".into(),
            source_event_id: None,
            source_event_fingerprint: Some("stable-private-payload-hash".into()),
            event_name: "Notification".into(),
            project_label: "project".into(),
            payload_json: "{}".into(),
            status: "waiting".into(),
            phase: Some("needs-you".into()),
        };

        database
            .record_observed_live_event(&event(first_time))
            .expect("first replay should persist");
        database
            .record_observed_live_event(&event(second_time))
            .expect("second replay should deduplicate");

        let connection = database.connect().expect("database should connect");
        let canonical: (i64, String, String) = connection
            .query_row(
                "SELECT COUNT(*), MIN(occurred_at), MIN(observed_at)
                 FROM canonical_events",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("canonical replay should load");
        assert_eq!(canonical, (1, first_time.into(), first_time.into()));
    }

    #[test]
    fn resumed_session_creates_a_new_waiting_episode_without_source_ids() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let database = Database::open(temporary.path().join("resumed-waiting.sqlite"))
            .expect("database should open");
        let event = |time: &str, status: &str| ObservedLiveEvent {
            occurred_at: time.into(),
            observed_at: time.into(),
            expires_at: "2026-08-09T11:00:00Z".into(),
            agent: "claude-code".into(),
            source_session_id: "resumed-session".into(),
            source_event_id: None,
            source_event_fingerprint: Some(if status == "waiting" {
                "stable-waiting-fingerprint".into()
            } else {
                "stable-running-fingerprint".into()
            }),
            event_name: if status == "waiting" {
                "Notification".into()
            } else {
                "PostToolUse".into()
            },
            project_label: "project".into(),
            payload_json: "{}".into(),
            status: status.into(),
            phase: Some(if status == "waiting" {
                "needs-you".into()
            } else {
                "thinking".into()
            }),
        };

        database
            .record_observed_live_event(&event("2026-08-09T10:00:01Z", "waiting"))
            .expect("first waiting episode should persist");
        database
            .record_observed_live_event(&event("2026-08-09T10:00:02Z", "running"))
            .expect("resume should persist");
        database
            .record_observed_live_event(&event("2026-08-09T10:00:03Z", "waiting"))
            .expect("second waiting episode should persist");

        let connection = database.connect().expect("database should connect");
        let canonical_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM canonical_events", [], |row| {
                row.get(0)
            })
            .expect("canonical count should load");
        assert_eq!(canonical_count, 2);
    }

    #[test]
    fn canonical_times_normalize_to_utc_and_ties_use_stable_ids() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let database = Database::open(temporary.path().join("canonical-time-order.sqlite"))
            .expect("database should open");
        let instant = Utc::now() - Duration::seconds(1);
        let utc_time = instant.to_rfc3339_opts(SecondsFormat::AutoSi, true);
        let offset_time = instant
            .with_timezone(&chrono::FixedOffset::east_opt(8 * 60 * 60).expect("valid offset"))
            .to_rfc3339_opts(SecondsFormat::AutoSi, false);
        let observed_at = Utc::now().to_rfc3339_opts(SecondsFormat::AutoSi, true);
        let event = |source_event_id: &str, occurred_at: &str| ObservedLiveEvent {
            occurred_at: occurred_at.into(),
            observed_at: observed_at.clone(),
            expires_at: (Utc::now() + Duration::hours(1))
                .to_rfc3339_opts(SecondsFormat::AutoSi, true),
            agent: "codex".into(),
            source_session_id: "time-order-session".into(),
            source_event_id: Some(source_event_id.into()),
            source_event_fingerprint: None,
            event_name: "PermissionRequest".into(),
            project_label: "project".into(),
            payload_json: "{}".into(),
            status: "waiting".into(),
            phase: Some("needs-you".into()),
        };
        database
            .record_observed_live_event(&event("event-b", &offset_time))
            .expect("offset event should persist");
        database
            .record_observed_live_event(&event("event-a", &utc_time))
            .expect("UTC event should persist");

        let connection = database.connect().expect("database should connect");
        let times: (i64, String, String) = connection
            .query_row(
                "SELECT COUNT(DISTINCT occurred_at), MIN(occurred_at), MAX(occurred_at)
                 FROM canonical_events",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("canonical times should load");
        drop(connection);
        let activity = database.live_activity().expect("activity should load");
        let ids = activity
            .timeline
            .iter()
            .map(|item| item.id.clone())
            .collect::<Vec<_>>();
        let mut sorted_ids = ids.clone();
        sorted_ids.sort();

        assert_eq!(times.0, 1);
        assert_eq!(times.1, utc_time);
        assert_eq!(times.2, utc_time);
        assert_eq!(ids, sorted_ids);
    }

    #[test]
    fn v13_live_records_remain_visible_after_idempotent_upgrade() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let path = temporary.path().join("legacy-live.sqlite");
        create_v13_live_database(&path, false);

        let database = Database::open(path.clone()).expect("legacy database should upgrade");
        let first = database
            .live_activity()
            .expect("legacy activity should load");
        let first_sessions = database
            .sessions("all", SessionListFilters::default(), 0, 10)
            .expect("legacy sessions should load");
        let first_detail = database
            .session_detail(&first_sessions.items[0].id)
            .expect("legacy session detail should load");
        assert_eq!(first.timeline.len(), 1);
        assert_eq!(first.timeline[0].status, "waiting");
        assert_eq!(first.timeline[0].observed_at, None);
        assert_eq!(first_sessions.items[0].title, "Legacy indexed session");
        assert_eq!(first_detail.file_changes.len(), 1);
        drop(database);
        let _ = std::fs::remove_file(sqlite_sidecar(&path, "-wal"));
        let _ = std::fs::remove_file(sqlite_sidecar(&path, "-shm"));

        let reopened = Database::open(path).expect("upgraded database should reopen");
        let second = reopened
            .live_activity()
            .expect("activity should still load");
        let second_sessions = reopened
            .sessions("all", SessionListFilters::default(), 0, 10)
            .expect("legacy sessions should still load");
        let second_detail = reopened
            .session_detail(&second_sessions.items[0].id)
            .expect("legacy session detail should still load");
        let connection = reopened.connect().expect("database should connect");
        let canonical_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM canonical_events", [], |row| {
                row.get(0)
            })
            .expect("canonical count should load");
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("version should load");

        assert_eq!(second.timeline.len(), 1);
        assert_eq!(second_sessions.items[0].title, "Legacy indexed session");
        assert_eq!(second_detail.file_changes.len(), 1);
        assert_eq!(canonical_count, 0);
        assert_eq!(version, 14);
        assert_no_schema_migration_artifacts(temporary.path().join("legacy-live.sqlite").as_path());
    }

    #[test]
    fn failed_v14_migration_rolls_back_without_damaging_v13_data() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let path = temporary.path().join("migration-rollback.sqlite");
        create_v13_live_database(&path, true);
        let original_bytes = std::fs::read(&path).expect("original database should be readable");

        assert!(Database::open(path.clone()).is_err());
        let after_bytes = std::fs::read(&path).expect("failed migration should preserve database");

        let connection = Connection::open(path).expect("original database should remain usable");
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("version should remain readable");
        let rows: i64 = connection
            .query_row("SELECT COUNT(*) FROM live_events", [], |row| row.get(0))
            .expect("legacy events should remain readable");
        let canonical_link_columns: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('live_events')
                 WHERE name='canonical_event_id'",
                [],
                |row| row.get(0),
            )
            .expect("legacy schema should remain readable");
        let historical_rows: i64 = connection
            .query_row(
                "SELECT COUNT(*)
                 FROM sessions s JOIN file_changes fc ON fc.session_id=s.id",
                [],
                |row| row.get(0),
            )
            .expect("historical evidence should remain readable");

        assert_eq!(after_bytes, original_bytes);
        assert_eq!(version, 13);
        assert_eq!(rows, 1);
        assert_eq!(canonical_link_columns, 0);
        assert_eq!(historical_rows, 1);
        assert_no_schema_migration_artifacts(
            temporary.path().join("migration-rollback.sqlite").as_path(),
        );
    }

    #[test]
    fn interrupted_installed_migration_recovers_from_its_persistent_marker() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let path = temporary.path().join("interrupted-migration.sqlite");
        create_v13_live_database(&path, false);
        let (staging, rollback, marker) = migration_paths(&path);

        let source = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .expect("source should open");
        source
            .backup(MAIN_DB, &staging, None)
            .expect("staging copy should be created");
        drop(source);
        let staged = Connection::open(&staging).expect("staging should open");
        apply_schema_migrations(&staged, 13).expect("staging should migrate");
        staged
            .pragma_update(None, "journal_mode", "DELETE")
            .expect("staging should checkpoint");
        validate_v14_connection(&staged).expect("staging should validate");
        drop(staged);
        std::fs::hard_link(&path, &rollback).expect("rollback link should persist");
        create_migration_marker(&marker).expect("migration marker should persist");
        std::fs::rename(&staging, &path).expect("staging should be installed");

        let recovered = Database::open(path.clone()).expect("interrupted migration should recover");
        let version: i64 = recovered
            .connect()
            .expect("database should connect")
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("version should load");

        assert_eq!(version, 14);
        assert_no_schema_migration_artifacts(&path);
    }

    #[test]
    fn interrupted_rollback_resumes_sidecar_restoration() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let path = temporary.path().join("interrupted-rollback.sqlite");
        let base = temporary.path().join("interrupted-rollback-base.sqlite");
        create_v13_live_database(&path, false);
        let (_, rollback, marker) = migration_paths(&path);

        let connection = Connection::open(&path).expect("legacy database should open");
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .expect("legacy database should use WAL");
        connection
            .pragma_update(None, "wal_autocheckpoint", 0)
            .expect("automatic checkpointing should be disabled");
        connection
            .execute(
                "INSERT INTO live_events(
                    received_at, expires_at, agent, source_session_id,
                    event_name, project_label, payload_json, status
                 ) VALUES (
                    '2026-08-10T00:00:00Z', '2026-08-10T01:00:00Z',
                    'codex', 'sidecar-session', 'SidecarOnly', 'project', '{}', 'waiting'
                 )",
                [],
            )
            .expect("sidecar-only transaction should commit");
        std::fs::copy(&path, &base).expect("pre-checkpoint main database should be copied");
        std::fs::copy(
            sqlite_sidecar(&path, "-wal"),
            sqlite_sidecar(&rollback, "-wal"),
        )
        .expect("rollback WAL should be copied");
        std::fs::copy(
            sqlite_sidecar(&path, "-shm"),
            sqlite_sidecar(&rollback, "-shm"),
        )
        .expect("rollback shared-memory file should be copied");
        drop(connection);

        std::fs::copy(&base, &path).expect("restored main database should be simulated");
        remove_file_if_exists(&sqlite_sidecar(&path, "-wal")).expect("live WAL should be absent");
        remove_file_if_exists(&sqlite_sidecar(&path, "-shm")).expect("live SHM should be absent");
        drop(create_migration_marker(&marker).expect("migration marker should persist"));
        assert!(
            !rollback.exists(),
            "main rollback should already be restored"
        );

        recover_interrupted_schema_migration(&path)
            .expect("sidecar restoration should resume after interruption");

        let recovered = Connection::open(&path).expect("restored database should open");
        let rows: i64 = recovered
            .query_row(
                "SELECT COUNT(*) FROM live_events WHERE event_name='SidecarOnly'",
                [],
                |row| row.get(0),
            )
            .expect("sidecar transaction should remain visible");
        assert_eq!(rows, 1);
        assert_no_schema_migration_artifacts(&path);
    }

    #[test]
    fn partial_rollback_copy_never_replaces_a_valid_legacy_database() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let path = temporary.path().join("partial-rollback.sqlite");
        create_v13_live_database(&path, false);
        let (_, rollback, marker) = migration_paths(&path);
        let copying = rollback_copy_path(&rollback);
        let original_bytes = std::fs::read(&path).expect("legacy database should be readable");
        std::fs::write(&copying, b"incomplete rollback copy")
            .expect("partial rollback copy should be simulated");
        drop(create_migration_marker(&marker).expect("migration marker should persist"));

        recover_interrupted_schema_migration(&path)
            .expect("valid legacy database should win over a partial copy");

        assert_eq!(
            std::fs::read(&path).expect("legacy database should remain readable"),
            original_bytes
        );
        assert_no_schema_migration_artifacts(&path);
    }

    #[test]
    fn active_migration_marker_prevents_a_second_migrator() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let path = temporary.path().join("active-migration.sqlite");
        create_v13_live_database(&path, false);
        let (_, _, marker) = migration_paths(&path);
        let marker_lock = create_migration_marker(&marker).expect("marker should lock");
        let original_bytes = std::fs::read(&path).expect("database should be readable");

        assert!(Database::open(path.clone()).is_err());
        assert_eq!(
            std::fs::read(&path).expect("database should remain readable"),
            original_bytes
        );
        assert!(marker.exists());

        drop(marker_lock);
        recover_interrupted_schema_migration(&path).expect("stale marker should recover");
        assert_no_schema_migration_artifacts(&path);
    }

    #[test]
    fn canonical_waiting_output_excludes_sensitive_payload_and_paths() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let database = Database::open(temporary.path().join("private-waiting.sqlite"))
            .expect("database should open");
        let now = Utc::now().to_rfc3339();
        let event = ObservedLiveEvent {
            occurred_at: now.clone(),
            observed_at: now,
            expires_at: (Utc::now() + Duration::hours(1)).to_rfc3339(),
            agent: "codex".into(),
            source_session_id: "/Users/private/work/private-session".into(),
            source_event_id: Some("request-private-value".into()),
            source_event_fingerprint: None,
            event_name: "prompt-private-value".into(),
            project_label: "/Users/private/work/project".into(),
            payload_json: r#"{"prompt":"private-value","command":"private-command","tool_arguments":{"path":"/Users/private/work/project"}}"#.into(),
            status: "waiting".into(),
            phase: Some("prompt-private-value".into()),
        };
        database
            .record_observed_live_event(&event)
            .expect("private event should persist safely");

        let activity = database.live_activity().expect("activity should load");
        let output = serde_json::to_string(&activity).expect("activity should serialize");
        let connection = database.connect().expect("database should connect");
        let canonical_projection: String = connection
            .query_row(
                "SELECT id || '|' || COALESCE(source_event_id, '') || '|' || source_session_id || '|' ||
                        event_fingerprint || '|' || dedup_key || '|' || source_event_name ||
                        '|' || project_label || '|' || COALESCE(live_phase, '')
                 FROM canonical_events",
                [],
                |row| row.get(0),
            )
            .expect("canonical projection should load");

        for sensitive in [
            "private-value",
            "private-command",
            "/Users/private/work/project",
            "/Users/private/work/private-session",
            "tool_arguments",
        ] {
            assert!(!output.contains(sensitive));
            assert!(!canonical_projection.contains(sensitive));
        }
        assert_eq!(activity.timeline[0].event_name, "Waiting");
        assert!(activity.timeline[0].project_label.starts_with("private-"));
    }

    #[test]
    fn work_events_hide_untitled_sessions_without_work_evidence() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let database = Database::open(temporary.path().join("task-feed.sqlite"))
            .expect("database should open");
        let date = Local::now().date_naive().format("%Y-%m-%d").to_string();

        let mut empty = ParseState::new(AgentKind::Cursor, "empty-session".into());
        empty.started_at = Some(format!("{date}T10:00:00Z"));
        empty.ended_at = Some(format!("{date}T10:00:30Z"));
        empty.active_seconds = 30;
        empty.tool_calls = 3;
        database
            .persist_parse_state("empty-source", 1, 1, 1, &empty)
            .expect("empty session should persist");

        let mut observed = ParseState::new(AgentKind::Cursor, "observed-session".into());
        observed.started_at = Some(format!("{date}T11:00:00Z"));
        observed.ended_at = Some(format!("{date}T11:02:00Z"));
        observed.title = Some("Observed work".into());
        observed.project_label = Some("visible-project".into());
        observed.usage.input_tokens = 12;
        database
            .persist_parse_state("observed-source", 1, 1, 1, &observed)
            .expect("observed session should persist");

        let tasks = database.tasks("today").expect("tasks should load");
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title, "Observed work");
        assert_eq!(tasks[0].project_label, "visible-project");
    }

    #[test]
    fn sessions_filters_and_pagination_run_server_side() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let database = Database::open(temporary.path().join("session-filters.sqlite"))
            .expect("database should open");
        let date = Local::now().date_naive().format("%Y-%m-%d").to_string();
        let persist =
            |id: &str, model: &str, project: &str, files: u64, errors: u64, verified: bool| {
                let mut state = ParseState::new(AgentKind::Codex, id.into());
                state.started_at = Some(format!("{date}T10:00:00Z"));
                state.ended_at = Some(format!("{date}T11:00:00Z"));
                state.current_model = Some(model.into());
                state.project_label = Some(project.into());
                state.project_hash = Some(format!("hash-{project}"));
                state.title = Some(format!("{id} title"));
                state.touched_file_hashes = (0..files)
                    .map(|index| format!("{id}-file-{index}"))
                    .collect();
                state.lines_added = files;
                state.errors = errors;
                state.verification_events = u64::from(verified);
                state.active_seconds = if errors >= 3 { 2_000 } else { 600 };
                database
                    .persist_parse_state(&format!("{id}-file"), 1, 1, 1, &state)
                    .expect("session should persist");
            };
        persist("alpha", "model-a", "proj-one", 2, 0, false);
        persist("beta", "model-b", "proj-two", 0, 4, false);
        persist("gamma", "model-a", "proj-one", 3, 0, true);

        let code_only = database
            .sessions(
                "today",
                SessionListFilters {
                    code_only: true,
                    ..SessionListFilters::default()
                },
                0,
                10,
            )
            .expect("code filter");
        assert_eq!(code_only.total, 2);
        assert!(code_only.items.iter().all(|item| item.files_touched > 0));
        assert!(code_only.models.iter().any(|item| item == "model-a"));
        assert!(code_only.projects.iter().any(|item| item == "proj-one"));

        let verified = database
            .sessions(
                "today",
                SessionListFilters {
                    verification_state: Some("verified"),
                    ..SessionListFilters::default()
                },
                0,
                10,
            )
            .expect("verification filter");
        assert_eq!(verified.total, 1);
        assert_eq!(verified.items[0].title, "gamma title");

        let attention = database
            .sessions(
                "today",
                SessionListFilters {
                    attention_only: true,
                    ..SessionListFilters::default()
                },
                0,
                10,
            )
            .expect("attention filter");
        assert_eq!(attention.total, 1);
        assert_eq!(attention.items[0].title, "beta title");

        let project = database
            .sessions(
                "today",
                SessionListFilters {
                    project: Some("proj-one"),
                    model: Some("model-a"),
                    ..SessionListFilters::default()
                },
                0,
                10,
            )
            .expect("project and model filter");
        assert_eq!(project.total, 2);

        let page = database
            .sessions("today", SessionListFilters::default(), 0, 2)
            .expect("page 0");
        assert_eq!(page.total, 3);
        assert_eq!(page.items.len(), 2);
        assert_eq!(page.page_size, 2);
        let page_two = database
            .sessions("today", SessionListFilters::default(), 1, 2)
            .expect("page 1");
        assert_eq!(page_two.items.len(), 1);
    }

    #[test]
    fn excludes_external_evidence_from_most_edited_file() {
        assert!(is_routine_edit_path("[external]/transcript.md"));
        assert!(!is_routine_edit_path("src/pages/InsightsPage.tsx"));
    }

    #[test]
    fn counts_usage_by_observed_day_even_when_session_started_earlier() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let database =
            Database::open(temporary.path().join("range.sqlite")).expect("database should open");
        let usage = TokenUsage {
            input_tokens: 120,
            output_tokens: 30,
            cache_read_tokens: 50,
            ..TokenUsage::default()
        };
        let mut state = ParseState::new(AgentKind::ClaudeCode, "spanning-session".into());
        state.started_at = Some("2026-07-19T15:00:00Z".into());
        state.ended_at = Some("2026-07-20T03:30:00Z".into());
        state.current_model = Some("claude-sonnet-4-6".into());
        state.usage = usage.clone();
        state.daily = HashMap::from([(
            "2026-07-20".into(),
            DailyAggregate {
                usage: usage.clone(),
                events: 1,
                ..DailyAggregate::default()
            },
        )]);
        state.hourly = HashMap::from([("2026-07-20T11:00".into(), usage.clone())]);
        database
            .persist_parse_state("fixture", 1, 1, 1, &state)
            .expect("state should persist");

        let connection = database.connect().expect("database connection");
        let totals = query_overview_totals(&connection, "2026-07-20T00:00:00Z", "2026-07-20")
            .expect("overview totals");
        assert_eq!(totals.usage.total(), 200);
        assert_eq!(totals.session_count, 1);
        assert!(totals.estimated_cost_usd.is_some());
        assert_eq!(totals.cost_coverage, 1.0);
        let distribution = query_usage_distribution(&connection, "agent", "agent", "2026-07-20")
            .expect("agent distribution");
        assert_eq!(distribution[0].label, "claude-code");
        assert_eq!(distribution[0].value, 200.0);
    }

    #[test]
    fn overview_cost_estimates_missing_historical_rows_for_the_selected_range() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let database = Database::open(temporary.path().join("historical-cost.sqlite"))
            .expect("database should open");
        let today = Local::now().date_naive();
        let recent_date = today - Duration::days(10);
        let older_date = today - Duration::days(60);
        let persist = |id: &str, date: chrono::NaiveDate, input_tokens: u64| {
            let date = date.format("%Y-%m-%d").to_string();
            let usage = TokenUsage {
                input_tokens,
                ..TokenUsage::default()
            };
            let mut state = ParseState::new(AgentKind::Codex, id.into());
            state.started_at = Some(format!("{date}T08:00:00Z"));
            state.ended_at = Some(format!("{date}T08:10:00Z"));
            state.current_model = Some("gpt-5.6-sol".into());
            state.usage = usage.clone();
            state.daily = HashMap::from([(
                date,
                DailyAggregate {
                    usage,
                    events: 1,
                    estimated_cost_usd: None,
                    ..DailyAggregate::default()
                },
            )]);
            database
                .persist_parse_state(id, 1, 1, 1, &state)
                .expect("state should persist");
        };
        persist("recent", recent_date, 1_000_000);
        persist("older", older_date, 2_000_000);

        let connection = database.connect().expect("database connection");
        let month_start = (today - Duration::days(29)).format("%Y-%m-%d").to_string();
        let ninety_day_start = (today - Duration::days(89)).format("%Y-%m-%d").to_string();
        let month = query_overview_totals(&connection, "1970-01-01T00:00:00Z", &month_start)
            .expect("month totals");
        let ninety_days =
            query_overview_totals(&connection, "1970-01-01T00:00:00Z", &ninety_day_start)
                .expect("ninety-day totals");

        assert_eq!(month.estimated_cost_usd, Some(5.0));
        assert_eq!(ninety_days.estimated_cost_usd, Some(15.0));
        assert_eq!(month.cost_coverage, 1.0);
        assert_eq!(ninety_days.cost_coverage, 1.0);
    }

    #[test]
    fn lists_spanning_sessions_by_their_latest_activity() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let database = Database::open(temporary.path().join("session-recency.sqlite"))
            .expect("database should open");
        let today = Local::now().date_naive();
        let day = |offset: i64| {
            (today - Duration::days(offset))
                .format("%Y-%m-%d")
                .to_string()
        };

        let mut spanning = ParseState::new(AgentKind::Codex, "spanning".into());
        spanning.started_at = Some(format!("{}T08:00:00Z", day(2)));
        spanning.ended_at = Some(format!("{}T09:30:00Z", day(0)));
        spanning.title = Some("current conversation".into());
        database
            .persist_parse_state("spanning-file", 1, 1, 1, &spanning)
            .expect("spanning session should persist");

        let mut older = ParseState::new(AgentKind::Codex, "older".into());
        older.started_at = Some(format!("{}T12:00:00Z", day(1)));
        older.ended_at = Some(format!("{}T12:30:00Z", day(1)));
        older.title = Some("older conversation".into());
        database
            .persist_parse_state("older-file", 1, 1, 1, &older)
            .expect("older session should persist");

        let sessions = database
            .sessions("today", SessionListFilters::default(), 0, 10)
            .expect("today sessions should load");
        assert_eq!(sessions.total, 1);
        assert_eq!(sessions.items[0].title, "current conversation");
    }

    #[test]
    fn selected_sources_persist_and_filter_the_data_overview() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let database = Database::open(temporary.path().join("source-selection.sqlite"))
            .expect("database should open");
        let date = Local::now().date_naive().format("%Y-%m-%d").to_string();
        let make_state = |agent: AgentKind, id: &str, tokens: u64| {
            let mut state = ParseState::new(agent, id.into());
            state.started_at = Some(format!("{date}T02:00:00Z"));
            state.ended_at = Some(format!("{date}T02:10:00Z"));
            state.current_model = Some("fixture-model".into());
            state.usage.input_tokens = tokens;
            state.daily = HashMap::from([(
                date.clone(),
                DailyAggregate {
                    usage: state.usage.clone(),
                    active_seconds: 600,
                    events: 1,
                    ..DailyAggregate::default()
                },
            )]);
            state
        };

        database
            .persist_parse_state(
                "codex-selection",
                1,
                1,
                1,
                &make_state(AgentKind::Codex, "codex-selection", 300),
            )
            .expect("Codex state should persist");
        database
            .persist_parse_state(
                "claude-selection",
                1,
                1,
                1,
                &make_state(AgentKind::ClaudeCode, "claude-selection", 700),
            )
            .expect("Claude state should persist");
        database
            .upsert_source(AgentKind::Codex, "codex-path", true, "ready")
            .expect("Codex source should persist");
        database
            .upsert_source(AgentKind::ClaudeCode, "claude-path", true, "ready")
            .expect("Claude source should persist");

        let initial = database
            .overview("today", IndexStatus::default())
            .expect("initial overview");
        assert_eq!(initial.totals.usage.total(), 1_000);
        assert_eq!(initial.agents.len(), 2);

        database
            .set_source_selected("codex", false)
            .expect("Codex selection should update");
        let sources = database.sources().expect("sources should load");
        assert!(
            !sources
                .iter()
                .find(|source| source.agent == "codex")
                .expect("Codex source")
                .selected
        );

        let filtered = database
            .overview("today", IndexStatus::default())
            .expect("filtered overview");
        assert_eq!(filtered.totals.usage.total(), 700);
        assert_eq!(filtered.agents.len(), 1);
        assert_eq!(filtered.agents[0].label, "claude-code");
    }

    #[test]
    fn removes_cursor_events_previously_misattributed_to_claude_code() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let database = Database::open(temporary.path().join("live-cleanup.sqlite"))
            .expect("database should open");
        let received_at = Utc::now().to_rfc3339();
        let expires_at = (Utc::now() + Duration::hours(1)).to_rfc3339();

        database
            .record_live_event(
                &received_at,
                &expires_at,
                "claude-code",
                "cursor-session",
                "sessionStart",
                "CursorProject",
                r#"{"cursor_version":"3.12.30","composer_mode":"agent"}"#,
                "idle",
            )
            .expect("misattributed Cursor event should persist");
        database
            .record_live_event(
                &received_at,
                &expires_at,
                "claude-code",
                "real-claude-session",
                "SessionStart",
                "ClaudeProject",
                r#"{"session_id":"real-claude-session"}"#,
                "idle",
            )
            .expect("real Claude event should persist");

        assert_eq!(
            database
                .purge_misattributed_cursor_live_events()
                .expect("cleanup should succeed"),
            1
        );
        let connection = database.connect().expect("database connection");
        let cursor_events: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM live_events WHERE source_session_id='cursor-session'",
                [],
                |row| row.get(0),
            )
            .expect("Cursor event count");
        let cursor_metrics: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM live_session_metrics
                 WHERE source_session_id='cursor-session'",
                [],
                |row| row.get(0),
            )
            .expect("Cursor metric count");
        let real_claude_events: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM live_events
                 WHERE source_session_id='real-claude-session'",
                [],
                |row| row.get(0),
            )
            .expect("real Claude event count");
        assert_eq!(cursor_events, 0);
        assert_eq!(cursor_metrics, 0);
        assert_eq!(real_claude_events, 1);
    }

    #[test]
    fn removes_only_known_live_validation_sessions() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let database = Database::open(temporary.path().join("validation-cleanup.sqlite"))
            .expect("database should open");
        let received_at = Utc::now().to_rfc3339();
        let expires_at = (Utc::now() + Duration::hours(1)).to_rfc3339();
        for (index, session_id) in [
            "claude-expanded-check",
            "codex-expanded-check",
            "vibemeter-direct-check",
            "vibemeter-visual-check",
        ]
        .into_iter()
        .enumerate()
        {
            database
                .record_live_event(
                    &received_at,
                    &expires_at,
                    "codex",
                    session_id,
                    if index == 0 {
                        "PermissionRequest"
                    } else {
                        "PreToolUse"
                    },
                    "validation",
                    "{}",
                    if index == 0 { "waiting" } else { "running" },
                )
                .expect("validation event should persist");
        }
        database
            .record_live_event(
                &received_at,
                &expires_at,
                "claude-code",
                "real-claude-session",
                "PermissionRequest",
                "project",
                "{}",
                "waiting",
            )
            .expect("real Claude event should persist");

        assert_eq!(
            database
                .purge_known_live_validation_events()
                .expect("validation cleanup"),
            4
        );
        let connection = database.connect().expect("database connection");
        let validation_events: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM live_events
                 WHERE source_session_id IN (
                    'claude-expanded-check',
                    'codex-expanded-check',
                    'vibemeter-direct-check',
                    'vibemeter-visual-check'
                 )",
                [],
                |row| row.get(0),
            )
            .expect("validation event count");
        let validation_metrics: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM live_session_metrics
                 WHERE source_session_id IN (
                    'claude-expanded-check',
                    'codex-expanded-check',
                    'vibemeter-direct-check',
                    'vibemeter-visual-check'
                 )",
                [],
                |row| row.get(0),
            )
            .expect("validation metric count");
        let real_events: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM live_events
                 WHERE source_session_id='real-claude-session'",
                [],
                |row| row.get(0),
            )
            .expect("real event count");
        let canonical_events: i64 = connection
            .query_row("SELECT COUNT(*) FROM canonical_events", [], |row| {
                row.get(0)
            })
            .expect("canonical event count");
        assert_eq!(validation_events, 0);
        assert_eq!(validation_metrics, 0);
        assert_eq!(real_events, 1);
        assert_eq!(canonical_events, 1);
    }

    #[test]
    fn live_history_links_to_the_matching_indexed_session() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let database = Database::open(temporary.path().join("live-history-link.sqlite"))
            .expect("database should open");
        let received_at = Utc::now().to_rfc3339();
        let expires_at = (Utc::now() + Duration::hours(1)).to_rfc3339();
        let connection = database.connect().expect("database connection");
        connection
            .execute(
                "INSERT INTO sessions(
                    id, source_session_id, agent, started_at, parser_version,
                    source_file_hash, source_size, source_mtime, updated_at
                 ) VALUES(
                    'indexed-session', 'source-session', 'claude-code', ?1, 'test',
                    'source-hash', 1, 1, ?1
                 )",
                params![received_at],
            )
            .expect("indexed session should persist");
        drop(connection);
        database
            .record_live_event(
                &received_at,
                &expires_at,
                "claude-code",
                "source-session",
                "PermissionRequest",
                "project",
                "{}",
                "waiting",
            )
            .expect("waiting event should persist");

        let activity = database.live_activity().expect("live activity");
        assert_eq!(activity.history.len(), 1);
        assert_eq!(
            activity.history[0].session_id.as_deref(),
            Some("indexed-session")
        );
    }

    #[test]
    fn live_timeline_returns_the_newest_events_first() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let database = Database::open(temporary.path().join("live-timeline-order.sqlite"))
            .expect("database should open");
        let earlier = (Utc::now() - Duration::minutes(2)).to_rfc3339();
        let later = (Utc::now() - Duration::minutes(1)).to_rfc3339();
        let expires_at = (Utc::now() + Duration::hours(1)).to_rfc3339();

        database
            .record_live_event(
                &earlier,
                &expires_at,
                "codex",
                "earlier-session",
                "EarlierEvent",
                "project",
                "{}",
                "running",
            )
            .expect("earlier event should persist");
        database
            .record_live_event(
                &later,
                &expires_at,
                "codex",
                "later-session",
                "LaterEvent",
                "project",
                "{}",
                "running",
            )
            .expect("later event should persist");

        let activity = database.live_activity().expect("live activity");
        assert_eq!(activity.timeline.len(), 2);
        assert_eq!(activity.timeline[0].event_name, "LaterEvent");
        assert_eq!(activity.timeline[1].event_name, "EarlierEvent");
    }

    #[test]
    fn removes_codex_memory_children_from_existing_live_metrics() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let database = Database::open(temporary.path().join("memory-cleanup.sqlite"))
            .expect("database should open");
        let received_at = Utc::now().to_rfc3339();
        let expires_at = (Utc::now() + Duration::hours(1)).to_rfc3339();
        database
            .record_live_event(
                &received_at,
                &expires_at,
                "codex",
                "memory-child",
                "PermissionRequest",
                "memories",
                r#"{"payload":{"cwd":"/Users/test/.codex/memories"}}"#,
                "waiting",
            )
            .expect("memory child should persist");
        database
            .record_live_event(
                &received_at,
                &expires_at,
                "codex",
                "real-session",
                "SessionStart",
                "project",
                r#"{"payload":{"cwd":"/Users/test/Code/project"}}"#,
                "running",
            )
            .expect("real session should persist");

        assert_eq!(
            database
                .purge_codex_memory_live_events()
                .expect("memory cleanup"),
            1
        );
        let connection = database.connect().expect("database connection");
        let memory_metrics: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM live_session_metrics
                 WHERE source_session_id='memory-child'",
                [],
                |row| row.get(0),
            )
            .expect("memory metric count");
        let real_metrics: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM live_session_metrics
                 WHERE source_session_id='real-session'",
                [],
                |row| row.get(0),
            )
            .expect("real metric count");
        let canonical_events: i64 = connection
            .query_row("SELECT COUNT(*) FROM canonical_events", [], |row| {
                row.get(0)
            })
            .expect("canonical event count");
        assert_eq!(memory_metrics, 0);
        assert_eq!(real_metrics, 1);
        assert_eq!(canonical_events, 0);
    }

    #[test]
    fn preserves_each_source_file_when_agents_reuse_a_session_id() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let database = Database::open(temporary.path().join("source-collision.sqlite"))
            .expect("database should open");
        let state = |tokens: u64, started_at: &str| {
            let mut state = ParseState::new(AgentKind::ClaudeCode, "shared-session".into());
            state.started_at = Some(started_at.into());
            state.ended_at = Some(started_at.into());
            state.usage.input_tokens = tokens;
            state
        };

        database
            .persist_parse_state("root-file", 1, 1, 1, &state(7_000, "2026-07-23T01:00:00Z"))
            .expect("root source should persist");
        database
            .persist_parse_state("agent-file", 1, 1, 1, &state(900, "2026-07-23T01:01:00Z"))
            .expect("agent source should persist");

        let connection = database.connect().expect("database connection");
        let (session_count, token_total): (i64, i64) = connection
            .query_row(
                "SELECT COUNT(*), SUM(input_tokens + output_tokens + cache_read_tokens
                    + cache_write_tokens + cache_write_1h_tokens)
                 FROM sessions WHERE agent='claude-code'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("collision totals");
        assert_eq!(session_count, 2);
        assert_eq!(token_total, 7_900);
    }

    #[test]
    fn catchphrase_cloud_requires_cross_session_repetition_and_tracks_agent_attribution() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let database =
            Database::open(temporary.path().join("phrases.sqlite")).expect("database should open");
        let date = Local::now().date_naive().format("%Y-%m-%d").to_string();
        let make_state = |agent: AgentKind, id: &str, agent_occurrences: u64| {
            let mut state = ParseState::new(agent, id.into());
            state.started_at = Some(format!("{date}T02:00:00Z"));
            state.ended_at = Some(format!("{date}T02:10:00Z"));
            state.current_model = Some(
                match agent {
                    AgentKind::Codex => "gpt-5.4",
                    AgentKind::ClaudeCode => "claude-opus-4.6",
                    _ => "unknown-model",
                }
                .into(),
            );
            state.phrase_counts.insert(
                "user".into(),
                PhraseAggregate {
                    date: date.clone(),
                    role: "user".into(),
                    phrase: "先验证一下".into(),
                    occurrences: 2,
                },
            );
            state.phrase_counts.insert(
                "agent".into(),
                PhraseAggregate {
                    date: date.clone(),
                    role: "agent".into(),
                    phrase: "验证已经通过".into(),
                    occurrences: agent_occurrences,
                },
            );
            state.phrase_counts.insert(
                "agent-topic".into(),
                PhraseAggregate {
                    date: date.clone(),
                    role: "agent".into(),
                    phrase: "nature sustainability".into(),
                    occurrences: 100,
                },
            );
            state
        };
        database
            .persist_parse_state(
                "phrase-one",
                1,
                1,
                1,
                &make_state(AgentKind::Codex, "phrase-one", 3),
            )
            .expect("first phrase session");
        database
            .persist_parse_state(
                "phrase-two",
                1,
                1,
                1,
                &make_state(AgentKind::ClaudeCode, "phrase-two", 1),
            )
            .expect("second phrase session");

        let response = database.phrase_cloud("30d").expect("phrase cloud");
        assert_eq!(response.user.status, "ready");
        assert_eq!(response.agents.status, "ready");
        assert_eq!(response.user.items[0].phrase, "先验证一下");
        assert_eq!(
            response.agents.items[0].dominant_agent.as_deref(),
            Some("codex")
        );
        assert_eq!(
            response.agents.items[0].dominant_model.as_deref(),
            Some("gpt-5.4")
        );
        assert_eq!(response.agents.items[0].session_count, 2);
        assert_eq!(response.agents.items[0].models.len(), 2);
        assert!(
            response
                .agents
                .items
                .iter()
                .all(|item| item.phrase != "nature sustainability")
        );
    }

    #[test]
    fn catchphrase_voice_gate_rejects_topics_and_incomplete_fragments() {
        assert!(phrase_voice_factor("我会先").is_some());
        assert!(phrase_voice_factor("接下来我会").is_some());
        assert!(phrase_voice_factor("你接受……吗").is_some());
        assert!(phrase_voice_factor("run tests").is_some());
        assert!(phrase_voice_factor("nature sustainability").is_none());
        assert!(phrase_voice_factor("我会把").is_none());
        assert!(phrase_voice_factor("我会按的").is_none());
    }

    #[test]
    fn catchphrase_cloud_collapses_nested_phrases_with_the_same_session_evidence() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let database =
            Database::open(temporary.path().join("phrase-dedup.sqlite")).expect("database open");
        let date = Local::now().date_naive().format("%Y-%m-%d").to_string();
        for session in ["dedup-one", "dedup-two"] {
            let mut state = ParseState::new(AgentKind::Codex, session.into());
            state.started_at = Some(format!("{date}T02:00:00Z"));
            state.ended_at = Some(format!("{date}T02:10:00Z"));
            for phrase in [
                "please implement",
                "please implement this",
                "please implement this plan",
            ] {
                state.phrase_counts.insert(
                    phrase.into(),
                    PhraseAggregate {
                        date: date.clone(),
                        role: "user".into(),
                        phrase: phrase.into(),
                        occurrences: 2,
                    },
                );
            }
            database
                .persist_parse_state(session, 1, 1, 1, &state)
                .expect("phrase session");
        }

        let response = database.phrase_cloud("30d").expect("phrase cloud");
        let phrases = response
            .user
            .items
            .iter()
            .map(|item| item.phrase.as_str())
            .collect::<HashSet<_>>();
        assert!(phrases.contains("please implement this plan"));
        assert!(!phrases.contains("please implement"));
        assert!(!phrases.contains("please implement this"));
    }

    #[test]
    fn task_aggregation_uses_objective_semantics_with_supporting_signals() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let database =
            Database::open(temporary.path().join("tasks.sqlite")).expect("database should open");
        let state = |id: &str, start: &str, end: &str, path: &str| {
            let mut state = ParseState::new(AgentKind::Codex, id.into());
            state.started_at = Some(start.into());
            state.ended_at = Some(end.into());
            state.title = Some("Continue VibeMeter review flow".into());
            state.project_hash = Some("project-hash".into());
            state.project_label = Some("vibemeter-fixture".into());
            state.current_model = Some("gpt-5".into());
            state.file_changes.insert(
                path.into(),
                crate::models::FileChangeAccumulator {
                    path: path.into(),
                    change_kind: "modified".into(),
                    lines_added: 4,
                    modification_count: 1,
                    ..crate::models::FileChangeAccumulator::default()
                },
            );
            state.git_evidence = Some(crate::models::GitEvidence {
                available: true,
                state: "available".into(),
                branch: Some("main".into()),
                commits: Vec::new(),
            });
            state
        };
        let first = state(
            "task-session-one",
            "2026-07-21T01:00:00Z",
            "2026-07-21T01:30:00Z",
            "src/review.ts",
        );
        let second = state(
            "task-session-two",
            "2026-07-21T02:15:00Z",
            "2026-07-21T02:40:00Z",
            "src/review.ts",
        );
        let mut third = state(
            "task-session-three",
            "2026-07-21T03:00:00Z",
            "2026-07-21T03:20:00Z",
            "src/review.ts",
        );
        third.title = Some("Investigate provider quota refresh failures".into());
        database
            .persist_parse_state("one", 1, 1, 1, &first)
            .unwrap();
        database
            .persist_parse_state("two", 1, 1, 1, &second)
            .unwrap();
        assert_eq!(database.tasks("all").unwrap().len(), 1);
        assert_eq!(database.tasks("all").unwrap()[0].grouping_state, "auto");
        assert!(database.tasks("all").unwrap()[0].confidence >= 0.9);
        database
            .persist_parse_state("three", 1, 1, 1, &third)
            .unwrap();
        assert_eq!(database.tasks("all").unwrap().len(), 2);
    }

    #[test]
    fn semantic_similarity_handles_chinese_and_english_objectives() {
        assert!(
            semantic_similarity(
                "继续修复 VibeMeter 的分享导出流程",
                "修复 VibeMeter 分享导出，并运行验证"
            ) > semantic_similarity(
                "继续修复 VibeMeter 的分享导出流程",
                "调查提供商额度刷新失败"
            )
        );
        assert!(
            semantic_similarity(
                "Repair the VibeMeter share export flow",
                "Continue repairing VibeMeter share exports"
            ) > 0.35
        );
    }

    #[test]
    fn serializes_concurrent_database_access() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let database = Database::open(temporary.path().join("concurrency.sqlite"))
            .expect("database should open");
        let barrier = Arc::new(Barrier::new(8));
        let workers = (0..8)
            .map(|worker_index| {
                let database = database.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    for _ in 0..40 {
                        match worker_index % 4 {
                            0 => {
                                database
                                    .overview("all", IndexStatus::default())
                                    .expect("overview query");
                            }
                            1 => {
                                database
                                    .sessions("all", SessionListFilters::default(), 0, 100)
                                    .expect("sessions query");
                            }
                            2 => {
                                database.comparison("all").expect("comparison query");
                            }
                            _ => {
                                database.sources().expect("source query");
                            }
                        }
                    }
                })
            })
            .collect::<Vec<_>>();

        for worker in workers {
            worker.join().expect("database worker should finish");
        }
    }

    #[test]
    fn live_conversation_titles_are_bounded_and_sanitized() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let database = Database::open(temporary.path().join("live-title.sqlite"))
            .expect("database should open");
        let mut state = ParseState::new(AgentKind::Codex, "conversation-source".into());
        state.started_at = Some("2026-07-31T10:00:00Z".into());
        state.ended_at = Some("2026-07-31T10:05:00Z".into());
        state.title = Some("[$skill]([path]) Repair stable Notch ordering".into());
        state.project_label = Some("vibemeter".into());
        database
            .persist_parse_state("conversation-file", 1, 1, 1, &state)
            .expect("session title should persist");

        let titles = database
            .live_conversation_titles(&[("codex".into(), "conversation-source".into())])
            .expect("title lookup should succeed");
        assert_eq!(
            titles
                .get(&("codex".into(), "conversation-source".into()))
                .map(String::as_str),
            Some("Repair stable Notch ordering")
        );
    }

    #[test]
    fn notch_completion_history_persists_cycles_and_supports_clear_undo() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let database = Database::open(temporary.path().join("notch-history.sqlite"))
            .expect("database should open");
        let now = Utc::now();
        let started_at = (now - Duration::minutes(15)).to_rfc3339();
        let mut session = notch_session("retained", "running", &started_at, &started_at);
        session.jump_context = Some(crate::models::LiveJumpContext {
            terminal_kind: Some("cmux".into()),
            cmux_socket: Some("/tmp/cmux.sock".into()),
            cmux_workspace_id: Some("workspace:2".into()),
            cmux_surface_id: Some("surface:8".into()),
            ..crate::models::LiveJumpContext::default()
        });
        database
            .mark_notch_sessions_seen(&[session.clone()])
            .expect("running task should be marked as seen");
        session.status = "completed".into();
        session.phase = "completed".into();
        session.updated_at = (now - Duration::minutes(2)).to_rfc3339();
        assert!(
            database
                .complete_notch_session(&session)
                .expect("completion should persist")
        );

        let completed = database
            .notch_completed_sessions()
            .expect("completed task should load");
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].cycle_started_at, started_at);
        assert_eq!(completed[0].completed_at, session.updated_at);
        assert_eq!(
            completed[0]
                .session
                .jump_context
                .as_ref()
                .and_then(|context| context.cmux_surface_id.as_deref()),
            Some("surface:8")
        );

        session.status = "running".into();
        session.phase = "thinking".into();
        database
            .mark_notch_sessions_seen(&[session.clone()])
            .expect("resumed task should start a new cycle");
        assert!(
            database
                .notch_completed_sessions()
                .expect("resumed task should leave completed")
                .is_empty()
        );
        session.status = "completed".into();
        session.phase = "completed".into();
        session.updated_at = Utc::now().to_rfc3339();
        database
            .complete_notch_session(&session)
            .expect("resumed completion should persist");
        let resumed = database
            .notch_completed_sessions()
            .expect("resumed completion should load");
        assert_eq!(resumed.len(), 1);
        assert_ne!(resumed[0].cycle_started_at, started_at);

        let clear = database
            .clear_notch_completed_sessions()
            .expect("clear should succeed");
        assert_eq!(clear.count, 1);
        assert!(
            database
                .notch_completed_sessions()
                .expect("cleared list")
                .is_empty()
        );
        assert_eq!(
            database
                .undo_clear_notch_completed_sessions(&clear.token)
                .expect("undo should succeed"),
            1
        );
        assert_eq!(
            database
                .notch_completed_sessions()
                .expect("restored list")
                .len(),
            1
        );
        assert!(
            database
                .delete_notch_completed_session("retained")
                .expect("single delete should succeed")
        );
        assert!(
            database
                .notch_completed_sessions()
                .expect("deleted list")
                .is_empty()
        );
    }

    #[test]
    fn notch_completion_history_keeps_only_ten_recent_tasks_for_twenty_four_hours() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let database = Database::open(temporary.path().join("notch-history-cap.sqlite"))
            .expect("database should open");
        let now = Utc::now();
        for index in 0..11 {
            let started_at = (now - Duration::minutes(20 + index)).to_rfc3339();
            let completed_at = (now - Duration::minutes(index)).to_rfc3339();
            let mut session = notch_session(
                &format!("recent-{index}"),
                "running",
                &started_at,
                &started_at,
            );
            database
                .mark_notch_sessions_seen(&[session.clone()])
                .expect("task should be marked as seen");
            session.status = "completed".into();
            session.phase = "completed".into();
            session.updated_at = completed_at;
            database
                .complete_notch_session(&session)
                .expect("completion should persist");
        }
        let expired_started = (now - Duration::hours(26)).to_rfc3339();
        let expired_completed = (now - Duration::hours(25)).to_rfc3339();
        let mut expired = notch_session("expired", "running", &expired_started, &expired_started);
        database
            .mark_notch_sessions_seen(&[expired.clone()])
            .expect("expired task should be marked as seen");
        expired.status = "completed".into();
        expired.phase = "completed".into();
        expired.updated_at = expired_completed;
        database
            .complete_notch_session(&expired)
            .expect("expired completion should be processed");

        let completed = database
            .notch_completed_sessions()
            .expect("completed tasks should load");
        assert_eq!(completed.len(), 10);
        assert_eq!(completed[0].session.id, "recent-0");
        assert!(completed.iter().all(|item| item.session.id != "recent-10"));
        assert!(completed.iter().all(|item| item.session.id != "expired"));
    }

    #[test]
    fn skill_summary_keeps_low_frequency_and_unrecorded_installs_separate() {
        let summary = build_skill_usage_summary(
            vec![
                SkillUsageItem {
                    name: "frequent".into(),
                    invocation_count: 9,
                    session_count: 4,
                },
                SkillUsageItem {
                    name: "occasional".into(),
                    invocation_count: 2,
                    session_count: 2,
                },
                SkillUsageItem {
                    name: "rare".into(),
                    invocation_count: 1,
                    session_count: 1,
                },
            ],
            vec![
                "frequent".into(),
                "occasional".into(),
                "rare".into(),
                "unused".into(),
            ],
        );
        assert_eq!(summary.most_used[0].name, "frequent");
        assert_eq!(summary.least_used[0].name, "rare");
        assert_eq!(summary.installed_without_usage, vec!["unused"]);
        assert_eq!(summary.used_count, 3);
        assert_eq!(summary.installed_count, 4);
    }
}
