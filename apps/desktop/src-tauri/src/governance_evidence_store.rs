use crate::errors::AppResult;
use crate::models::CanonicalEvent;
use rusqlite::{Connection, params};

pub(crate) fn query_session_events(
    connection: &Connection,
    session_id: &str,
) -> AppResult<Vec<CanonicalEvent>> {
    let mut statement = connection.prepare(
        "WITH target_session AS (
            SELECT agent, source_session_id
            FROM sessions
            WHERE id=?1
         )
         SELECT COALESCE(canonical.source_sequence, 0), canonical.occurred_at,
                canonical.event_type, COALESCE(canonical.process_phase, 'execute'),
                canonical.source_event_name,
                CASE canonical.event_result
                    WHEN 'succeeded' THEN 1
                    WHEN 'failed' THEN 0
                    ELSE NULL
                END,
                canonical.event_duration_ms, canonical.evidence_level
         FROM canonical_events canonical
         WHERE canonical.deleted_at IS NULL
           AND canonical.event_type<>'file.change'
           AND (
                (canonical.source='history-index' AND canonical.history_session_id=?1)
                OR (
                    canonical.source IN('live-hook','live-observer')
                    AND (
                        canonical.event_type LIKE 'delegation.%'
                        OR canonical.event_type IN('memory.read','memory.write')
                    )
                    AND EXISTS(
                        SELECT 1
                        FROM target_session target
                        WHERE target.agent=canonical.agent
                          AND target.source_session_id=canonical.source_session_id
                    )
                    AND NOT EXISTS(
                        SELECT 1
                        FROM canonical_events history
                        WHERE history.source='history-index'
                          AND history.history_session_id=?1
                          AND history.deleted_at IS NULL
                          AND history.event_type=canonical.event_type
                          AND COALESCE(history.occurred_at, '')=
                              COALESCE(canonical.occurred_at, '')
                    )
                )
           )
         ORDER BY CASE WHEN canonical.occurred_at IS NULL THEN 1 ELSE 0 END,
                  canonical.occurred_at,
                  CASE WHEN canonical.source_sequence IS NULL THEN 1 ELSE 0 END,
                  canonical.source_sequence, canonical.id",
    )?;
    Ok(statement
        .query_map(params![session_id], |row| {
            let sequence = row.get::<_, i64>(0)?.max(0) as u64;
            Ok(CanonicalEvent {
                sequence,
                source_event_id: None,
                source_event_fingerprint: None,
                occurred_at: row.get(1)?,
                event_type: row.get(2)?,
                category: row.get(3)?,
                name: row.get(4)?,
                success: row.get::<_, Option<i64>>(5)?.map(|value| value != 0),
                duration_ms: row
                    .get::<_, Option<i64>>(6)?
                    .map(|value| value.max(0) as u64),
                provenance: row.get(7)?,
                ..CanonicalEvent::default()
            })
        })?
        .collect::<Result<Vec<_>, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestEvent<'a> {
        id: &'a str,
        source: &'a str,
        source_session_id: &'a str,
        history_session_id: Option<&'a str>,
        event_type: &'a str,
        occurred_at: &'a str,
        source_event_name: &'a str,
    }

    fn insert_event(connection: &Connection, event: TestEvent<'_>) {
        connection
            .execute(
                "INSERT INTO canonical_events(
                    id, source, agent, source_session_id, history_session_id,
                    event_type, occurred_at, source_event_name, process_phase,
                    evidence_level, deleted_at
                 ) VALUES(?1, ?2, 'codex', ?3, ?4, ?5, ?6, ?7, 'execute',
                          'observed', NULL)",
                params![
                    event.id,
                    event.source,
                    event.source_session_id,
                    event.history_session_id,
                    event.event_type,
                    event.occurred_at,
                    event.source_event_name,
                ],
            )
            .expect("canonical fixture should persist");
    }

    #[test]
    fn process_includes_matched_governance_evidence_without_live_duplicates() {
        let connection = Connection::open_in_memory().expect("in-memory database should open");
        connection
            .execute_batch(
                "CREATE TABLE sessions(
                    id TEXT PRIMARY KEY,
                    agent TEXT NOT NULL,
                    source_session_id TEXT NOT NULL
                 );
                 CREATE TABLE canonical_events(
                    id TEXT PRIMARY KEY,
                    source TEXT NOT NULL,
                    agent TEXT NOT NULL,
                    source_session_id TEXT NOT NULL,
                    history_session_id TEXT,
                    source_sequence INTEGER,
                    occurred_at TEXT,
                    event_type TEXT NOT NULL,
                    process_phase TEXT,
                    source_event_name TEXT NOT NULL,
                    event_result TEXT,
                    event_duration_ms INTEGER,
                    evidence_level TEXT NOT NULL,
                    deleted_at TEXT
                 );
                 INSERT INTO sessions(id, agent, source_session_id)
                 VALUES('session-child', 'codex', 'source-child');",
            )
            .expect("fixture schema should initialize");

        insert_event(
            &connection,
            TestEvent {
                id: "history-start",
                source: "history-index",
                source_session_id: "source-child",
                history_session_id: Some("session-child"),
                event_type: "delegation.spawn.started",
                occurred_at: "2026-08-31T10:00:00Z",
                source_event_name: "Delegation",
            },
        );
        insert_event(
            &connection,
            TestEvent {
                id: "live-duplicate",
                source: "live-hook",
                source_session_id: "source-child",
                history_session_id: None,
                event_type: "delegation.spawn.started",
                occurred_at: "2026-08-31T10:00:00Z",
                source_event_name: "Delegation",
            },
        );
        insert_event(
            &connection,
            TestEvent {
                id: "live-waiting",
                source: "live-hook",
                source_session_id: "source-child",
                history_session_id: None,
                event_type: "delegation.child.waiting",
                occurred_at: "2026-08-31T10:01:00Z",
                source_event_name: "Delegation",
            },
        );
        insert_event(
            &connection,
            TestEvent {
                id: "live-memory",
                source: "live-hook",
                source_session_id: "source-child",
                history_session_id: None,
                event_type: "memory.read",
                occurred_at: "2026-08-31T10:02:00Z",
                source_event_name: "MemoryAccess",
            },
        );
        insert_event(
            &connection,
            TestEvent {
                id: "live-ordinary",
                source: "live-hook",
                source_session_id: "source-child",
                history_session_id: None,
                event_type: "tool.start",
                occurred_at: "2026-08-31T10:03:00Z",
                source_event_name: "ToolStart",
            },
        );
        insert_event(
            &connection,
            TestEvent {
                id: "other-session",
                source: "live-hook",
                source_session_id: "source-other",
                history_session_id: None,
                event_type: "delegation.child.waiting",
                occurred_at: "2026-08-31T10:04:00Z",
                source_event_name: "Delegation",
            },
        );

        let events = query_session_events(&connection, "session-child")
            .expect("process evidence should load");
        assert_eq!(
            events
                .iter()
                .map(|event| event.event_type.as_str())
                .collect::<Vec<_>>(),
            [
                "delegation.spawn.started",
                "delegation.child.waiting",
                "memory.read",
            ]
        );
    }
}
