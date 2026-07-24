use crate::errors::{AppError, AppResult};
use crate::models::{
    AgentKind, BehaviorSignals, BehaviorSummary, CanonicalEvent, ComparisonItem, CoverageNotice,
    DailyUsagePoint, DistributionItem, EvidenceReference, FileChange, GenerateReviewRequest,
    GitCommitEvidence, GitEvidence, GitFileStat, HourlyUsagePoint, IndexStatus, InsightItem,
    InsightStat, InsightsResponse, OverviewResponse, OverviewTotals, ParseState, PlaybookItem,
    ProcessPhase, ProjectControl, Provenance, ReviewContent, ReviewDocument, ReviewFinding,
    ReviewsResponse, SavePlaybookRequest, SessionDetail, SessionSummary, SessionsResponse,
    SourceStatus, TaskSummary, TodayInsight, TodayResponse, TokenUsage, UpdateReviewRequest,
    VctiProfile,
};
use crate::review_engine::{self, ReviewEvidence};
use chrono::{DateTime, Duration, Local, NaiveDate, Utc};
use rusqlite::{Connection, OptionalExtension, Transaction, params, params_from_iter};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

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

impl Database {
    pub fn open(path: PathBuf) -> AppResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(path)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version < 1 {
            connection.execute_batch(MIGRATION_V1)?;
        }
        if version < 2 {
            connection.execute_batch(MIGRATION_V2)?;
        }
        if version < 3 {
            connection.execute_batch(MIGRATION_V3)?;
        }
        if version < 4 {
            connection.execute_batch(MIGRATION_V4)?;
        }
        if version < 5 {
            connection.execute_batch(MIGRATION_V5)?;
        }
        if version < 6 {
            connection.execute_batch(MIGRATION_V6)?;
        }
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
        let capability_level = match agent {
            AgentKind::KimiCode => "partial",
            AgentKind::Cursor | AgentKind::OpenClaw | AgentKind::Hermes => "partial",
            AgentKind::ClaudeCode | AgentKind::Codex => "full",
        };
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
        let behavior = query_behavior_summary(&connection, &start_timestamp)?;
        let recent_sessions =
            query_session_rows(&connection, &start_timestamp, None, None, 0, 8)?.0;
        let warning_count = connection.query_row(
            "SELECT COALESCE(SUM(count), 0) FROM parser_warnings",
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
            behavior,
            recent_sessions,
            coverage,
            index_status,
        })
    }

    pub fn vcti_profile(&self) -> AppResult<VctiProfile> {
        let connection = self.connect()?;
        let now = Utc::now();
        let start_timestamp = (now - Duration::days(89))
            .format("%Y-%m-%dT00:00:00Z")
            .to_string();
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
                    THEN tu.count ELSE 0 END),0)
             FROM sessions s
             LEFT JOIN session_behavior sb ON sb.session_id=s.id
             LEFT JOIN tool_usage tu ON tu.session_id=s.id
             WHERE s.started_at>=?1
             GROUP BY s.id
             ORDER BY s.started_at",
        )?;
        let records = statement
            .query_map(params![start_timestamp], |row| {
                let behavior_json = row.get::<_, Option<String>>(17)?;
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
                    errors: read_u64(row, 10)?,
                    verification_events: read_u64(row, 11)?,
                    human_interventions: read_u64(row, 12)?,
                    subagent_count: read_u64(row, 13)?,
                    model_switches: read_u64(row, 14)?,
                    longest_uninterrupted_seconds: read_u64(row, 15)?,
                    has_commit: row.get::<_, i64>(16)? != 0,
                    behavior: behavior_json
                        .as_deref()
                        .and_then(|json| serde_json::from_str(json).ok())
                        .unwrap_or_default(),
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
        );
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
        Ok(profile)
    }

    pub fn today(&self, index_status: IndexStatus) -> AppResult<TodayResponse> {
        let connection = self.connect()?;
        let date = Local::now().date_naive().format("%Y-%m-%d").to_string();
        let start_timestamp = format!("{date}T00:00:00Z");
        let tasks = query_tasks(&connection, &start_timestamp, 24)?;
        let worth_reviewing = tasks
            .iter()
            .filter(|task| task.worth_reviewing)
            .take(4)
            .cloned()
            .collect::<Vec<_>>();
        let totals = query_overview_totals(&connection, &start_timestamp, &date)?;
        let mut insights = Vec::new();
        if let Some(task) = tasks.iter().find(|task| task.has_commit) {
            insights.push(TodayInsight {
                id: "verified-delivery".into(),
                tier: "fact".into(),
                message_key: "today.insight.verifiedDelivery".into(),
                value: None,
                evidence: vec![EvidenceReference {
                    kind: "task".into(),
                    id: task.id.clone(),
                    label: task.title.clone(),
                }],
            });
        }
        if let Some(task) = worth_reviewing.first() {
            insights.push(TodayInsight {
                id: "review-focus".into(),
                tier: "inference".into(),
                message_key: task
                    .review_reason_keys
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "today.insight.reviewFocus".into()),
                value: None,
                evidence: vec![EvidenceReference {
                    kind: "task".into(),
                    id: task.id.clone(),
                    label: task.title.clone(),
                }],
            });
        }
        Ok(TodayResponse {
            date,
            tasks,
            worth_reviewing,
            insights,
            totals,
            index_status,
        })
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

    pub fn reviews(
        &self,
        review_type: Option<&str>,
        target_id: Option<&str>,
    ) -> AppResult<ReviewsResponse> {
        let connection = self.connect()?;
        let mut statement = connection.prepare(
            "SELECT id, review_type, target_id, locale, version, status, title,
                    outcome, what_happened, what_worked, friction, lessons,
                    next_run, user_edited, source_excluded, created_at, updated_at
             FROM reviews
             WHERE (?1='' OR review_type=?1) AND (?2='' OR target_id=?2)
             ORDER BY updated_at DESC, version DESC",
        )?;
        let mut items = statement
            .query_map(
                params![review_type.unwrap_or(""), target_id.unwrap_or("")],
                review_from_row,
            )?
            .collect::<Result<Vec<_>, _>>()?;
        for review in &mut items {
            review.findings = query_review_findings(&connection, &review.id)?;
        }
        Ok(ReviewsResponse { items })
    }

    pub fn generate_review(&self, request: &GenerateReviewRequest) -> AppResult<ReviewDocument> {
        if !matches!(request.locale.as_str(), "en-US" | "zh-CN") {
            return Err(AppError::InvalidRequest("unsupported review locale".into()));
        }
        if !matches!(
            request.review_type.as_str(),
            "task" | "session" | "daily" | "weekly"
        ) {
            return Err(AppError::InvalidRequest("unsupported review type".into()));
        }
        let mut connection = self.connect()?;
        let evidence =
            query_review_evidence(&connection, &request.review_type, &request.target_id)?;
        let (title, content, findings) =
            review_engine::generate(&request.locale, &request.review_type, &evidence);
        persist_review_document(
            &mut connection,
            &request.review_type,
            &request.target_id,
            &request.locale,
            title,
            content,
            findings,
        )
    }

    pub fn deep_review_payload(&self, task_id: &str, locale: &str) -> AppResult<(String, String)> {
        if !matches!(locale, "en-US" | "zh-CN") {
            return Err(AppError::InvalidRequest("unsupported review locale".into()));
        }
        let connection = self.connect()?;
        let (title, project_label, session_count) = connection
            .query_row(
                "SELECT title, project_label,
                    (SELECT COUNT(*) FROM task_sessions ts WHERE ts.task_id=tasks.id)
                 FROM tasks WHERE id=?1",
                params![task_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        read_u64(row, 2)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| AppError::InvalidRequest("task not found".into()))?;

        let mut session_statement = connection.prepare(
            "SELECT s.agent, COALESCE(s.model,''), s.started_at,
                    COALESCE(s.ended_at,''), COALESCE(s.prompt_excerpt,''),
                    COALESCE(s.result_excerpt,''), s.tool_calls,
                    s.verification_events, s.files_touched, s.lines_added,
                    s.lines_deleted, s.errors, s.retries
             FROM sessions s JOIN task_sessions ts ON ts.session_id=s.id
             WHERE ts.task_id=?1 ORDER BY s.started_at DESC LIMIT 30",
        )?;
        let sessions = session_statement
            .query_map(params![task_id], |row| {
                Ok(serde_json::json!({
                    "agent": row.get::<_, String>(0)?,
                    "model": row.get::<_, String>(1)?,
                    "startedAt": row.get::<_, String>(2)?,
                    "endedAt": row.get::<_, String>(3)?,
                    "objective": row.get::<_, String>(4)?,
                    "observedFinalResponse": row.get::<_, String>(5)?,
                    "toolCalls": read_u64(row, 6)?,
                    "verificationEvents": read_u64(row, 7)?,
                    "filesTouched": read_u64(row, 8)?,
                    "linesAdded": read_u64(row, 9)?,
                    "linesDeleted": read_u64(row, 10)?,
                    "errors": read_u64(row, 11)?,
                    "retries": read_u64(row, 12)?,
                }))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(session_statement);

        let mut tool_statement = connection.prepare(
            "SELECT tu.tool, SUM(tu.count) FROM tool_usage tu
             JOIN task_sessions ts ON ts.session_id=tu.session_id
             WHERE ts.task_id=?1 GROUP BY tu.tool ORDER BY 2 DESC LIMIT 20",
        )?;
        let tools = tool_statement
            .query_map(params![task_id], |row| {
                Ok(serde_json::json!({
                    "name": row.get::<_, String>(0)?,
                    "count": read_u64(row, 1)?,
                }))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(tool_statement);

        let mut file_statement = connection.prepare(
            "SELECT fc.path, SUM(fc.lines_added), SUM(fc.lines_deleted),
                    SUM(fc.modification_count)
             FROM file_changes fc JOIN task_sessions ts ON ts.session_id=fc.session_id
             WHERE ts.task_id=?1 GROUP BY fc.path ORDER BY 4 DESC LIMIT 30",
        )?;
        let files = file_statement
            .query_map(params![task_id], |row| {
                Ok(serde_json::json!({
                    "path": row.get::<_, String>(0)?,
                    "linesAdded": read_u64(row, 1)?,
                    "linesDeleted": read_u64(row, 2)?,
                    "observedEdits": read_u64(row, 3)?,
                }))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(file_statement);

        let mut commit_statement = connection.prepare(
            "SELECT gc.subject, gc.committed_at FROM git_commits gc
             JOIN task_sessions ts ON ts.session_id=gc.session_id
             WHERE ts.task_id=?1 ORDER BY gc.committed_at DESC LIMIT 20",
        )?;
        let commits = commit_statement
            .query_map(params![task_id], |row| {
                Ok(serde_json::json!({
                    "subject": row.get::<_, String>(0)?,
                    "committedAt": row.get::<_, String>(1)?,
                }))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let payload = serde_json::to_string_pretty(&serde_json::json!({
            "reviewLanguage": locale,
            "task": {
                "title": title.clone(),
                "project": project_label,
                "sessionCount": session_count,
                "includedSessionCount": sessions.len(),
            },
            "sessions": sessions,
            "toolSummary": tools,
            "fileSummary": files,
            "commitSummary": commits,
            "privacyBoundary": "Bounded excerpts and project-relative evidence only. No full transcripts, raw code, absolute paths, or credentials.",
        }))?;
        Ok((title, payload))
    }

    pub fn save_deep_review(
        &self,
        task_id: &str,
        locale: &str,
        title: String,
        content: ReviewContent,
    ) -> AppResult<ReviewDocument> {
        let mut connection = self.connect()?;
        persist_review_document(
            &mut connection,
            "task",
            task_id,
            locale,
            title,
            content,
            Vec::new(),
        )
    }

    pub fn accept_review(&self, id: &str) -> AppResult<()> {
        let mut connection = self.connect()?;
        let transaction = connection.transaction()?;
        let group = transaction
            .query_row(
                "SELECT review_type, target_id, locale FROM reviews WHERE id=?1",
                params![id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| AppError::InvalidRequest("review not found".into()))?;
        transaction.execute(
            "UPDATE reviews SET status='archived', updated_at=?4
             WHERE review_type=?1 AND target_id=?2 AND locale=?3 AND status='current'",
            params![group.0, group.1, group.2, Utc::now().to_rfc3339()],
        )?;
        transaction.execute(
            "UPDATE reviews SET status='current', updated_at=?2 WHERE id=?1",
            params![id, Utc::now().to_rfc3339()],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn update_review(&self, request: &UpdateReviewRequest) -> AppResult<()> {
        let connection = self.connect()?;
        let changed = connection.execute(
            "UPDATE reviews SET title=?2, outcome=?3, what_happened=?4,
                what_worked=?5, friction=?6, lessons=?7, next_run=?8,
                user_edited=1, updated_at=?9 WHERE id=?1",
            params![
                request.id,
                request.title,
                request.content.outcome,
                request.content.what_happened,
                request.content.what_worked,
                request.content.friction,
                request.content.lessons,
                request.content.next_run,
                Utc::now().to_rfc3339(),
            ],
        )?;
        if changed == 0 {
            return Err(AppError::InvalidRequest("review not found".into()));
        }
        Ok(())
    }

    pub fn delete_review(&self, id: &str) -> AppResult<()> {
        let mut connection = self.connect()?;
        let transaction = connection.transaction()?;
        let exists = transaction
            .query_row("SELECT 1 FROM reviews WHERE id=?1", params![id], |_| {
                Ok(1u8)
            })
            .optional()?
            .is_some();
        if !exists {
            return Err(AppError::InvalidRequest("review not found".into()));
        }
        transaction.execute(
            "DELETE FROM review_findings WHERE review_id=?1",
            params![id],
        )?;
        transaction.execute("DELETE FROM reviews WHERE id=?1", params![id])?;
        transaction.commit()?;
        Ok(())
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
        agent: Option<&str>,
        search: Option<&str>,
        page: u64,
        page_size: u64,
    ) -> AppResult<SessionsResponse> {
        let connection = self.connect()?;
        let start_timestamp = format!("{}T00:00:00Z", range_start(range));
        let (items, total) = query_session_rows(
            &connection,
            &start_timestamp,
            agent,
            search,
            page,
            page_size.clamp(1, 100),
        )?;
        Ok(SessionsResponse {
            items,
            total,
            page,
            page_size,
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
                    status, warning_count, path_hash
             FROM sources ORDER BY CASE agent WHEN 'claude-code' THEN 0 ELSE 1 END",
        )?;
        let mut items = statement
            .query_map([], |row| {
                let path_hash: String = row.get(7)?;
                Ok(SourceStatus {
                    agent: row.get(0)?,
                    available: row.get::<_, i64>(1)? != 0,
                    capability_level: row.get(2)?,
                    session_count: read_u64(row, 3)?,
                    last_indexed_at: row.get(4)?,
                    status: row.get(5)?,
                    warning_count: read_u64(row, 6)?,
                    path_label: path_hash.chars().take(6).collect(),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        for (agent, capability_level, status) in [
            ("claude-code", "full", "not-found"),
            ("codex", "full", "not-found"),
            ("kimi-code", "partial", "not-found"),
            ("cursor", "partial", "not-found"),
            ("openclaw", "partial", "not-found"),
            ("hermes", "partial", "not-found"),
        ] {
            if !items.iter().any(|item| item.agent == agent) {
                items.push(SourceStatus {
                    agent: agent.into(),
                    available: false,
                    capability_level: capability_level.into(),
                    session_count: 0,
                    last_indexed_at: None,
                    status: status.into(),
                    warning_count: 0,
                    path_label: String::new(),
                });
            }
        }
        Ok(items)
    }

    pub fn today_and_heatmap(
        &self,
        days: i64,
    ) -> AppResult<(TokenUsage, Option<f64>, Vec<DailyUsagePoint>)> {
        let connection = self.connect()?;
        let today = Local::now().date_naive().format("%Y-%m-%d").to_string();
        let start = (Local::now().date_naive() - Duration::days(days.max(1) - 1))
            .format("%Y-%m-%d")
            .to_string();
        let usage = connection.query_row(
            "SELECT COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
                    COALESCE(SUM(cache_read_tokens),0), COALESCE(SUM(cache_write_tokens),0),
                    COALESCE(SUM(cache_write_1h_tokens),0), COALESCE(SUM(reasoning_tokens),0),
                    SUM(estimated_cost_usd)
             FROM daily_usage WHERE date=?1",
            params![today],
            |row| {
                Ok((
                    TokenUsage {
                        input_tokens: read_u64(row, 0)?,
                        output_tokens: read_u64(row, 1)?,
                        cache_read_tokens: read_u64(row, 2)?,
                        cache_write_tokens: read_u64(row, 3)?,
                        cache_write_1h_tokens: read_u64(row, 4)?,
                        reasoning_tokens: read_u64(row, 5)?,
                    },
                    row.get(6)?,
                ))
            },
        )?;
        Ok((usage.0, usage.1, query_daily(&connection, &start)?))
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
        "DELETE FROM session_files WHERE session_id=?1",
        params![session_id],
    )?;
    transaction.execute(
        "DELETE FROM events WHERE session_id=?1",
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

fn query_overview_totals(
    connection: &Connection,
    start_timestamp: &str,
    start_date: &str,
) -> AppResult<OverviewTotals> {
    let usage_row = connection.query_row(
        "SELECT COUNT(DISTINCT session_id), COALESCE(SUM(active_seconds),0),
                COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
                COALESCE(SUM(cache_read_tokens),0), COALESCE(SUM(cache_write_tokens),0),
                COALESCE(SUM(cache_write_1h_tokens),0), COALESCE(SUM(reasoning_tokens),0),
                SUM(estimated_cost_usd),
                COALESCE(SUM(CASE WHEN estimated_cost_usd IS NOT NULL THEN
                    input_tokens + output_tokens + cache_read_tokens + cache_write_tokens + cache_write_1h_tokens
                ELSE 0 END),0), COALESCE(SUM(errors),0)
         FROM daily_usage WHERE date >= ?1",
        params![start_date],
        |row| {
            Ok((
                read_u64(row, 0)?,
                read_u64(row, 1)?,
                TokenUsage {
                    input_tokens: read_u64(row, 2)?,
                    output_tokens: read_u64(row, 3)?,
                    cache_read_tokens: read_u64(row, 4)?,
                    cache_write_tokens: read_u64(row, 5)?,
                    cache_write_1h_tokens: read_u64(row, 6)?,
                    reasoning_tokens: read_u64(row, 7)?,
                },
                row.get::<_, Option<f64>>(8)?,
                read_u64(row, 9)?,
                read_u64(row, 10)?,
            ))
        },
    )?;
    let evidence_row = connection.query_row(
        "SELECT
                COALESCE(SUM(CASE WHEN files_touched > 0 THEN 1 ELSE 0 END),0),
                COALESCE(SUM(CASE WHEN files_touched > 0 AND verification_events > 0 THEN 1 ELSE 0 END),0),
                COALESCE(MAX(longest_uninterrupted_seconds),0),
                COALESCE(SUM(lines_added),0), COALESCE(SUM(lines_deleted),0),
                COALESCE(SUM(retries),0)
         FROM sessions WHERE started_at >= ?1",
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
         FROM sessions WHERE started_at >= ?1",
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
         WHERE s.started_at >= ?1",
        params![start_timestamp],
        |result| read_u64(result, 0),
    )?;
    let total_tokens = usage_row.2.total();
    let cost_coverage = if total_tokens == 0 {
        0.0
    } else {
        (usage_row.4 as f64 / total_tokens as f64).clamp(0.0, 1.0)
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
        usage: usage_row.2,
        estimated_cost_usd: usage_row.3,
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

fn query_daily(connection: &Connection, start_date: &str) -> AppResult<Vec<DailyUsagePoint>> {
    let mut statement = connection.prepare(
        "WITH daily_rows AS (
            SELECT session_id, date, agent, model,
                   input_tokens, output_tokens, cache_read_tokens,
                   cache_write_tokens, cache_write_1h_tokens, reasoning_tokens,
                   active_seconds, tool_calls, errors, estimated_cost_usd
            FROM daily_usage WHERE date >= ?1
            UNION ALL
            SELECT s.id, substr(s.started_at,1,10), s.agent, COALESCE(NULLIF(s.model,''),'unknown'),
                   0, 0, 0, 0, 0, 0,
                   s.active_seconds, s.tool_calls, s.errors, s.estimated_cost_usd
            FROM sessions s
            WHERE s.started_at >= ?2
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
         FROM hourly_usage WHERE hour >= ?1
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
         FROM daily_usage WHERE date >= ?1
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

fn persist_review_document(
    connection: &mut Connection,
    review_type: &str,
    target_id: &str,
    locale: &str,
    title: String,
    content: ReviewContent,
    findings: Vec<ReviewFinding>,
) -> AppResult<ReviewDocument> {
    let transaction = connection.transaction()?;
    let version = transaction.query_row(
        "SELECT COALESCE(MAX(version),0)+1 FROM reviews
         WHERE review_type=?1 AND target_id=?2 AND locale=?3",
        params![review_type, target_id, locale],
        |row| read_u64(row, 0),
    )?;
    let status = if version > 1 { "draft" } else { "current" };
    let id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    transaction.execute(
        "INSERT INTO reviews(
            id, review_type, target_id, locale, version, status, title,
            outcome, what_happened, what_worked, friction, lessons,
            next_run, user_edited, created_at, updated_at
         ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 0, ?14, ?14)",
        params![
            id,
            review_type,
            target_id,
            locale,
            sql_i64(version),
            status,
            title,
            content.outcome,
            content.what_happened,
            content.what_worked,
            content.friction,
            content.lessons,
            content.next_run,
            now,
        ],
    )?;
    for finding in &findings {
        transaction.execute(
            "INSERT INTO review_findings(
                review_id, id, rule_id, tier, title, detail, evidence_json
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id,
                finding.id,
                finding.rule_id,
                finding.tier,
                finding.title,
                finding.detail,
                serde_json::to_string(&finding.evidence)?,
            ],
        )?;
    }
    transaction.commit()?;
    Ok(ReviewDocument {
        id,
        review_type: review_type.into(),
        target_id: target_id.into(),
        locale: locale.into(),
        version,
        status: status.into(),
        title,
        content,
        findings,
        user_edited: false,
        source_excluded: false,
        created_at: now.clone(),
        updated_at: now,
    })
}

fn review_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ReviewDocument> {
    Ok(ReviewDocument {
        id: row.get(0)?,
        review_type: row.get(1)?,
        target_id: row.get(2)?,
        locale: row.get(3)?,
        version: read_u64(row, 4)?,
        status: row.get(5)?,
        title: row.get(6)?,
        content: ReviewContent {
            outcome: row.get(7)?,
            what_happened: row.get(8)?,
            what_worked: row.get(9)?,
            friction: row.get(10)?,
            lessons: row.get(11)?,
            next_run: row.get(12)?,
        },
        findings: Vec::new(),
        user_edited: row.get::<_, i64>(13)? != 0,
        source_excluded: row.get::<_, i64>(14)? != 0,
        created_at: row.get(15)?,
        updated_at: row.get(16)?,
    })
}

fn query_review_findings(
    connection: &Connection,
    review_id: &str,
) -> AppResult<Vec<ReviewFinding>> {
    let mut statement = connection.prepare(
        "SELECT id, rule_id, tier, title, detail, evidence_json
         FROM review_findings WHERE review_id=?1 ORDER BY rowid",
    )?;
    Ok(statement
        .query_map(params![review_id], |row| {
            let evidence_json: String = row.get(5)?;
            Ok(ReviewFinding {
                id: row.get(0)?,
                rule_id: row.get(1)?,
                tier: row.get(2)?,
                title: row.get(3)?,
                detail: row.get(4)?,
                evidence: serde_json::from_str(&evidence_json).unwrap_or_default(),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?)
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

fn query_review_evidence(
    connection: &Connection,
    review_type: &str,
    target_id: &str,
) -> AppResult<ReviewEvidence> {
    let session_ids = match review_type {
        "session" => connection
            .query_row(
                "SELECT id FROM sessions WHERE id=?1",
                params![target_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .into_iter()
            .collect::<Vec<_>>(),
        "daily" => {
            let mut statement = connection.prepare(
                "SELECT id FROM sessions WHERE substr(started_at,1,10)=?1 ORDER BY started_at",
            )?;
            statement
                .query_map(params![target_id], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
        }
        "weekly" => {
            let start = NaiveDate::parse_from_str(target_id, "%Y-%m-%d")
                .map_err(|_| AppError::InvalidRequest("invalid weekly review date".into()))?;
            let end = (start + Duration::days(7)).format("%Y-%m-%d").to_string();
            let start = start.format("%Y-%m-%d").to_string();
            let mut statement = connection.prepare(
                "SELECT id FROM sessions WHERE started_at>=?1 AND started_at<?2 ORDER BY started_at",
            )?;
            statement
                .query_map(
                    params![format!("{start}T00:00:00Z"), format!("{end}T00:00:00Z")],
                    |row| row.get::<_, String>(0),
                )?
                .collect::<Result<Vec<_>, _>>()?
        }
        _ => {
            let mut statement = connection.prepare(
                "SELECT session_id FROM task_sessions WHERE task_id=?1 ORDER BY position",
            )?;
            statement
                .query_map(params![target_id], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
        }
    };
    if session_ids.is_empty() {
        return Err(AppError::InvalidRequest(
            "review target has no sessions".into(),
        ));
    }
    let placeholders = std::iter::repeat_n("?", session_ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let aggregate_sql = format!(
        "SELECT COUNT(*),
            COALESCE(SUM(input_tokens+output_tokens+cache_read_tokens+cache_write_tokens+cache_write_1h_tokens),0),
            COALESCE(SUM(active_seconds),0), COALESCE(SUM(files_touched),0),
            COALESCE(SUM(lines_added),0), COALESCE(SUM(lines_deleted),0),
            COALESCE(SUM(errors),0), COALESCE(SUM(retries),0),
            COALESCE(SUM(verification_events),0), COALESCE(SUM(model_switches),0),
            COALESCE(MAX(project_label),'')
         FROM sessions WHERE id IN({placeholders})"
    );
    let aggregate = connection.query_row(
        &aggregate_sql,
        params_from_iter(session_ids.iter()),
        |row| {
            Ok((
                read_u64(row, 0)?,
                read_u64(row, 1)?,
                read_u64(row, 2)?,
                read_u64(row, 3)?,
                read_u64(row, 4)?,
                read_u64(row, 5)?,
                read_u64(row, 6)?,
                read_u64(row, 7)?,
                read_u64(row, 8)?,
                read_u64(row, 9)?,
                row.get::<_, String>(10)?,
            ))
        },
    )?;
    let has_commit_sql =
        format!("SELECT EXISTS(SELECT 1 FROM git_commits WHERE session_id IN({placeholders}))");
    let has_commit = connection.query_row(
        &has_commit_sql,
        params_from_iter(session_ids.iter()),
        |row| Ok(row.get::<_, i64>(0)? != 0),
    )?;
    let file_sql = format!(
        "SELECT path, modification_count FROM file_changes
         WHERE session_id IN({placeholders})
         ORDER BY modification_count DESC LIMIT 1"
    );
    let max_file = connection
        .query_row(&file_sql, params_from_iter(session_ids.iter()), |row| {
            Ok((row.get::<_, String>(0)?, read_u64(row, 1)?))
        })
        .optional()?;
    let task_count_sql = format!(
        "SELECT COUNT(DISTINCT task_id) FROM task_sessions WHERE session_id IN({placeholders})"
    );
    let task_count = connection.query_row(
        &task_count_sql,
        params_from_iter(session_ids.iter()),
        |row| read_u64(row, 0),
    )?;
    let objective_sql = format!(
        "SELECT DISTINCT prompt_excerpt FROM sessions
         WHERE id IN({placeholders}) AND COALESCE(prompt_excerpt,'')<>''
         ORDER BY started_at LIMIT 5"
    );
    let mut objective_statement = connection.prepare(&objective_sql)?;
    let objectives = objective_statement
        .query_map(params_from_iter(session_ids.iter()), |row| {
            row.get::<_, String>(0)
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(objective_statement);
    let result_sql = format!(
        "SELECT result_excerpt FROM sessions
         WHERE id IN({placeholders}) AND COALESCE(result_excerpt,'')<>''
         ORDER BY started_at DESC LIMIT 5"
    );
    let mut result_statement = connection.prepare(&result_sql)?;
    let result_excerpts = result_statement
        .query_map(params_from_iter(session_ids.iter()), |row| {
            row.get::<_, String>(0)
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(result_statement);
    let commit_sql = format!(
        "SELECT DISTINCT subject FROM git_commits
         WHERE session_id IN({placeholders}) ORDER BY committed_at DESC LIMIT 8"
    );
    let mut commit_statement = connection.prepare(&commit_sql)?;
    let commit_subjects = commit_statement
        .query_map(params_from_iter(session_ids.iter()), |row| {
            row.get::<_, String>(0)
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(commit_statement);
    let tools_sql = format!(
        "SELECT tool, SUM(count) FROM tool_usage
         WHERE session_id IN({placeholders}) GROUP BY tool ORDER BY 2 DESC LIMIT 6"
    );
    let mut tools_statement = connection.prepare(&tools_sql)?;
    let top_tools = tools_statement
        .query_map(params_from_iter(session_ids.iter()), |row| {
            Ok((row.get::<_, String>(0)?, read_u64(row, 1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(tools_statement);
    let title = match review_type {
        "session" => connection
            .query_row(
                "SELECT COALESCE(title,'') FROM sessions WHERE id=?1",
                params![target_id],
                |row| row.get(0),
            )
            .unwrap_or_default(),
        "task" => connection
            .query_row(
                "SELECT title FROM tasks WHERE id=?1",
                params![target_id],
                |row| row.get(0),
            )
            .unwrap_or_default(),
        _ => target_id.into(),
    };
    let mut task_tokens = query_tasks(connection, "1970-01-01T00:00:00Z", 10_000)?
        .into_iter()
        .map(|task| task.total_tokens)
        .collect::<Vec<_>>();
    task_tokens.sort_unstable();
    let comparable_tasks = task_tokens.len() as u64;
    let personal_high_token_threshold = if task_tokens.len() >= 20 {
        let index = ((task_tokens.len() - 1) as f64 * 0.9).round() as usize;
        task_tokens.get(index).copied()
    } else {
        None
    };
    Ok(ReviewEvidence {
        target_id: target_id.into(),
        title,
        project_label: aggregate.10,
        session_count: aggregate.0,
        task_count,
        total_tokens: aggregate.1,
        active_seconds: aggregate.2,
        files_changed: aggregate.3,
        lines_added: aggregate.4,
        lines_deleted: aggregate.5,
        errors: aggregate.6,
        retries: aggregate.7,
        verification_events: aggregate.8,
        has_commit,
        max_file_path: max_file.as_ref().map(|value| value.0.clone()),
        max_modification_count: max_file.map_or(0, |value| value.1),
        model_switches: aggregate.9,
        comparable_tasks,
        personal_high_token_threshold,
        objectives,
        result_excerpts,
        commit_subjects,
        top_tools,
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

fn query_session_rows(
    connection: &Connection,
    start_timestamp: &str,
    agent: Option<&str>,
    search: Option<&str>,
    page: u64,
    page_size: u64,
) -> AppResult<(Vec<SessionSummary>, u64)> {
    let agent_filter = agent.unwrap_or("");
    let search_filter = search.unwrap_or("").trim();
    let search_pattern = format!("%{search_filter}%");
    let base_where = "started_at >= ?1
        AND (?2='' OR agent=?2)
        AND (?3='' OR COALESCE(title,'') LIKE ?4 OR COALESCE(model,'') LIKE ?4
            OR COALESCE(project_label,'') LIKE ?4
            OR EXISTS(SELECT 1 FROM file_changes fc WHERE fc.session_id=sessions.id AND fc.path LIKE ?4))";
    let total = connection.query_row(
        &format!("SELECT COUNT(*) FROM sessions WHERE {base_where}"),
        params![start_timestamp, agent_filter, search_filter, search_pattern],
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
         ORDER BY started_at DESC LIMIT ?5 OFFSET ?6"
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement
        .query_map(
            params![
                start_timestamp,
                agent_filter,
                search_filter,
                search_pattern,
                sql_i64(limit),
                sql_i64(offset)
            ],
            session_from_row,
        )?
        .collect::<Result<Vec<_>, _>>()?;
    Ok((rows, total))
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
    let project_label = if project_value.len() == 16
        && project_value.chars().all(|value| value.is_ascii_hexdigit())
    {
        project_value.chars().take(6).collect()
    } else {
        project_value
    };
    Ok(SessionSummary {
        id: row.get(0)?,
        agent: row.get(1)?,
        model: row.get(2)?,
        title: crate::privacy::clean_display_title(
            &row.get::<_, Option<String>>(3)?.unwrap_or_default(),
        ),
        project_label,
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
         WHERE s.started_at>=?1",
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
        let parser_current = parser_version.starts_with('4') || parser_version.starts_with('5');
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
    use crate::models::DailyAggregate;
    use std::collections::HashMap;
    use std::sync::{Arc, Barrier};

    #[test]
    fn derives_verification_state_from_observed_evidence() {
        assert_eq!(derive_verification_state(0, 0, 0, 1), "verified");
        assert_eq!(derive_verification_state(0, 12, 0, 0), "unverified");
        assert_eq!(derive_verification_state(1, 0, 0, 0), "unverified");
        assert_eq!(derive_verification_state(0, 0, 0, 0), "not-applicable");
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
        let distribution = query_usage_distribution(&connection, "agent", "agent", "2026-07-20")
            .expect("agent distribution");
        assert_eq!(distribution[0].label, "claude-code");
        assert_eq!(distribution[0].value, 200.0);
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
    fn project_exclusion_purges_source_evidence_but_marks_user_authored_material() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let database = Database::open(temporary.path().join("exclusion.sqlite"))
            .expect("database should open");
        let mut state = ParseState::new(AgentKind::Codex, "source-session".into());
        state.started_at = Some("2026-07-21T02:00:00Z".into());
        state.ended_at = Some("2026-07-21T02:30:00Z".into());
        state.title = Some("Refine review flow".into());
        state.project_hash = Some("project-hash".into());
        state.project_label = Some("aftervibe-fixture".into());
        state.current_model = Some("gpt-5".into());
        state.usage.input_tokens = 2_000;
        state.usage.output_tokens = 400;
        database
            .persist_parse_state("fixture", 1, 1, 1, &state)
            .expect("state should persist");
        let session_id = session_database_id(AgentKind::Codex, "source-session");
        let authored = database
            .generate_review(&GenerateReviewRequest {
                review_type: "session".into(),
                target_id: session_id.clone(),
                locale: "en-US".into(),
            })
            .expect("first review");
        database
            .update_review(&UpdateReviewRequest {
                id: authored.id.clone(),
                title: "My retained review".into(),
                content: authored.content.clone(),
            })
            .expect("review edit");
        database
            .generate_review(&GenerateReviewRequest {
                review_type: "session".into(),
                target_id: session_id.clone(),
                locale: "en-US".into(),
            })
            .expect("generated draft");
        database
            .save_playbook_item(&SavePlaybookRequest {
                id: None,
                title: "Keep verification close".into(),
                body: "Run the focused check after the first coherent edit.".into(),
                category: "verification".into(),
                project_label: Some("aftervibe-fixture".into()),
                task_type: None,
                source_review_id: Some(authored.id.clone()),
                source_finding_id: None,
                applied: false,
            })
            .expect("playbook item");
        database
            .split_session(&session_id)
            .expect("user task should be created");

        database
            .exclude_project("project-hash")
            .expect("project exclusion");

        assert_eq!(
            database.sessions("all", None, None, 0, 100).unwrap().total,
            0
        );
        let reviews = database
            .reviews(Some("session"), Some(&session_id))
            .unwrap();
        assert_eq!(reviews.items.len(), 1);
        assert!(reviews.items[0].user_edited);
        assert!(reviews.items[0].source_excluded);
        let playbook = database.playbook_items(None).unwrap();
        assert_eq!(playbook.len(), 1);
        assert!(playbook[0].source_excluded);
        let connection = database.connect().expect("database connection");
        let retained_tasks: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM tasks WHERE user_edited=1 AND source_excluded=1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(retained_tasks, 1);
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
            state.title = Some("Continue aftervibe review flow".into());
            state.project_hash = Some("project-hash".into());
            state.project_label = Some("aftervibe-fixture".into());
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
                "继续修复 aftervibe 的分享导出流程",
                "修复 aftervibe 分享导出，并运行验证"
            ) > semantic_similarity(
                "继续修复 aftervibe 的分享导出流程",
                "调查提供商额度刷新失败"
            )
        );
        assert!(
            semantic_similarity(
                "Repair the aftervibe share export flow",
                "Continue repairing aftervibe share exports"
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
                                    .sessions("all", None, None, 0, 100)
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
}
