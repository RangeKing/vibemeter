use crate::errors::AppResult;
use crate::governance::capabilities::{
    SignalCapability, SourceCapabilities, source_capability_for_name,
};
use crate::governance::evidence::merge_coverage;
use crate::governance::memory::{
    CanonicalMemorySignal, MEMORY_LEDGER_ALGORITHM_VERSION, project_memory_accesses,
    valid_memory_operation,
};
use crate::models::{
    MemoryAccess, MemoryLedgerCoverage, MemoryLedgerEvidenceReference, MemoryLedgerResponse,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use std::collections::BTreeMap;

fn load_signals(connection: &Connection) -> AppResult<Vec<CanonicalMemorySignal>> {
    let mut statement = connection.prepare(
        "SELECT canonical.id, canonical.agent,
                COALESCE(canonical.history_session_id, session.id, canonical.source_session_id),
                canonical.source_session_id, canonical.work_unit_id,
                canonical.event_type, canonical.occurred_at, canonical.observed_at,
                canonical.evidence_level, canonical.source_coverage
         FROM canonical_events canonical
         LEFT JOIN (
             SELECT agent, source_session_id, MIN(id) AS id
             FROM sessions
             GROUP BY agent, source_session_id
         ) session
           ON session.agent=canonical.agent
          AND session.source_session_id=canonical.source_session_id
         WHERE canonical.deleted_at IS NULL
           AND canonical.event_type IN('memory.read','memory.write')
         ORDER BY COALESCE(canonical.occurred_at, canonical.observed_at),
                  canonical.observed_at, canonical.id",
    )?;
    Ok(statement
        .query_map([], |row| {
            Ok(CanonicalMemorySignal {
                canonical_event_id: row.get(0)?,
                agent: row.get(1)?,
                session_id: row.get(2)?,
                source_session_id: row.get(3)?,
                work_unit_id: row.get(4)?,
                event_type: row.get(5)?,
                occurred_at: row.get(6)?,
                observed_at: row.get(7)?,
                evidence_level: row.get(8)?,
                source_coverage: row.get(9)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?)
}

pub(crate) fn rebuild(transaction: &Transaction<'_>, now: &str) -> AppResult<()> {
    let accesses = project_memory_accesses(&load_signals(transaction)?);
    transaction.execute(
        "UPDATE memory_accesses
         SET deleted_at=?1
         WHERE deleted_at IS NULL
           AND evidence_level<>'user-confirmed'",
        params![now],
    )?;

    for access in accesses {
        if access.evidence.is_empty() || !valid_memory_operation(&access.operation) {
            continue;
        }
        transaction.execute(
            "INSERT INTO memory_accesses(
                id, session_id, source_session_id, work_unit_id, operation,
                occurred_at, status, confidence, evidence_level, source_coverage,
                algorithm_version, deleted_at
             ) VALUES(
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, NULL
             )
             ON CONFLICT(id) DO UPDATE SET
                session_id=excluded.session_id,
                source_session_id=excluded.source_session_id,
                work_unit_id=excluded.work_unit_id,
                operation=excluded.operation,
                occurred_at=excluded.occurred_at,
                status=excluded.status,
                confidence=excluded.confidence,
                evidence_level=excluded.evidence_level,
                source_coverage=excluded.source_coverage,
                algorithm_version=excluded.algorithm_version,
                deleted_at=NULL
             WHERE memory_accesses.evidence_level<>'user-confirmed'",
            params![
                access.id,
                access.session_id,
                access.source_session_id,
                access.work_unit_id,
                access.operation,
                access.occurred_at,
                access.status,
                access.confidence,
                access.evidence_level,
                access.source_coverage,
                access.algorithm_version,
            ],
        )?;
        let user_confirmed: bool = transaction.query_row(
            "SELECT evidence_level='user-confirmed' FROM memory_accesses WHERE id=?1",
            params![access.id],
            |row| row.get(0),
        )?;
        if !user_confirmed {
            transaction.execute(
                "DELETE FROM memory_access_evidence WHERE memory_access_id=?1",
                params![access.id],
            )?;
        }
        for evidence in access.evidence {
            transaction.execute(
                "INSERT OR IGNORE INTO memory_access_evidence(
                    memory_access_id, canonical_event_id, evidence_role, observed_at
                 ) SELECT ?1, id, ?3, ?4
                   FROM canonical_events
                  WHERE id=?2 AND deleted_at IS NULL",
                params![
                    access.id,
                    evidence.canonical_event_id,
                    evidence.evidence_role,
                    evidence.observed_at,
                ],
            )?;
        }
    }
    Ok(())
}

fn capability_rank(capability: SignalCapability) -> u8 {
    match capability {
        SignalCapability::Exact => 4,
        SignalCapability::Derived => 3,
        SignalCapability::Partial => 2,
        SignalCapability::Unavailable => 0,
    }
}

fn operation_capability(capability: &SourceCapabilities, operation: &str) -> SignalCapability {
    match operation {
        "read" => capability.memory_read,
        "write" => capability.memory_write,
        _ => SignalCapability::Unavailable,
    }
}

fn reported_capability(capability: Option<&SourceCapabilities>) -> SignalCapability {
    let Some(capability) = capability else {
        return SignalCapability::Unavailable;
    };
    if capability_rank(capability.memory_read) >= capability_rank(capability.memory_write) {
        capability.memory_read
    } else {
        capability.memory_write
    }
}

fn unavailable_operations(capability: Option<&SourceCapabilities>) -> Vec<String> {
    let Some(capability) = capability else {
        return vec!["read".into(), "write".into()];
    };
    [
        ("read", capability.memory_read),
        ("write", capability.memory_write),
    ]
    .into_iter()
    .filter(|(_, value)| *value == SignalCapability::Unavailable)
    .map(|(operation, _)| operation.into())
    .collect()
}

fn not_recorded(capability: Option<&SourceCapabilities>) -> MemoryLedgerResponse {
    MemoryLedgerResponse {
        status: "not-recorded".into(),
        session_id: None,
        algorithm_version: MEMORY_LEDGER_ALGORITHM_VERSION.into(),
        accesses: Vec::new(),
        coverage: MemoryLedgerCoverage {
            capability: reported_capability(capability).as_str().into(),
            source_coverage: "not-recorded".into(),
            access_count: 0,
            read_count: 0,
            write_count: 0,
            evidence_count: 0,
            observed_count: 0,
            derived_count: 0,
            inferred_count: 0,
            unavailable_operations: unavailable_operations(capability),
        },
        evidence: Vec::new(),
    }
}

pub(crate) fn query_ledger(
    connection: &Connection,
    session_id: &str,
) -> AppResult<MemoryLedgerResponse> {
    let Some((agent, raw_source_session_id)) = connection
        .query_row(
            "SELECT agent, source_session_id FROM sessions WHERE id=?1",
            params![session_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
    else {
        return Ok(not_recorded(None));
    };
    let capability = source_capability_for_name(&agent);
    if capability.is_none_or(|item| {
        item.memory_read == SignalCapability::Unavailable
            && item.memory_write == SignalCapability::Unavailable
    }) {
        return Ok(not_recorded(capability));
    }
    let source_session_id = crate::privacy::safe_opaque_identifier(&raw_source_session_id);
    let mut statement = connection.prepare(
        "SELECT access.id, access.session_id, access.work_unit_id,
                access.operation, access.status, access.occurred_at,
                access.confidence, access.evidence_level, access.source_coverage,
                access.algorithm_version,
                evidence.canonical_event_id, evidence.evidence_role,
                canonical.occurred_at, canonical.event_type,
                canonical.evidence_level, canonical.source_coverage, canonical.agent
         FROM memory_accesses access
         JOIN memory_access_evidence evidence
           ON evidence.memory_access_id=access.id
         JOIN canonical_events canonical
           ON canonical.id=evidence.canonical_event_id
         WHERE access.deleted_at IS NULL
           AND (canonical.deleted_at IS NULL
                OR access.evidence_level='user-confirmed')
           AND (access.session_id=?1 OR access.source_session_id=?2)
           AND canonical.agent=?3
         ORDER BY COALESCE(access.occurred_at, evidence.observed_at),
                  access.id, evidence.canonical_event_id",
    )?;
    let rows = statement
        .query_map(params![session_id, source_session_id, agent], |row| {
            let access = MemoryAccess {
                id: row.get(0)?,
                agent: row.get(16)?,
                session_id: Some(session_id.into()),
                work_unit_id: row.get(2)?,
                operation: row.get(3)?,
                status: row.get(4)?,
                occurred_at: row.get(5)?,
                confidence: row.get(6)?,
                evidence_level: row.get(7)?,
                source_coverage: row.get(8)?,
                algorithm_version: row.get(9)?,
                evidence_ids: Vec::new(),
            };
            let canonical_event_id: String = row.get(10)?;
            let evidence = MemoryLedgerEvidenceReference {
                id: format!(
                    "memory-evidence-{}",
                    crate::privacy::stable_hash(&format!(
                        "{}|{}",
                        canonical_event_id,
                        row.get::<_, String>(11)?
                    ))
                ),
                canonical_event_id,
                session_id: Some(session_id.into()),
                role: row.get(11)?,
                occurred_at: row.get(12)?,
                event_type: row.get(13)?,
                evidence_level: row.get(14)?,
                source_coverage: row.get(15)?,
            };
            Ok((access, evidence))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if rows.is_empty() {
        return Ok(not_recorded(capability));
    }

    let mut accesses = BTreeMap::<String, MemoryAccess>::new();
    let mut evidence = Vec::new();
    for (access, reference) in rows {
        accesses
            .entry(access.id.clone())
            .or_insert(access)
            .evidence_ids
            .push(reference.canonical_event_id.clone());
        evidence.push(reference);
    }
    for access in accesses.values_mut() {
        access.evidence_ids.sort();
        access.evidence_ids.dedup();
    }
    let mut accesses = accesses.into_values().collect::<Vec<_>>();
    accesses.sort_by(|left, right| {
        left.occurred_at
            .cmp(&right.occurred_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    evidence.sort_by(|left, right| {
        left.occurred_at
            .cmp(&right.occurred_at)
            .then_with(|| left.canonical_event_id.cmp(&right.canonical_event_id))
    });
    evidence.dedup_by(|left, right| {
        left.canonical_event_id == right.canonical_event_id && left.role == right.role
    });
    let source_coverage = merge_coverage(
        accesses
            .iter()
            .map(|access| access.source_coverage.as_str()),
    );
    let source_capability = capability.expect("capability checked above");
    let ready = accesses.iter().all(|access| {
        operation_capability(source_capability, &access.operation) == SignalCapability::Exact
            && access.source_coverage.starts_with("exact-")
            && matches!(
                access.evidence_level.as_str(),
                "observed" | "user-confirmed"
            )
    });
    let weakest_capability = accesses
        .iter()
        .map(|access| operation_capability(source_capability, &access.operation))
        .min_by_key(|value| capability_rank(*value))
        .unwrap_or_else(|| reported_capability(capability));
    Ok(MemoryLedgerResponse {
        status: if ready { "ready" } else { "partial" }.into(),
        session_id: Some(session_id.into()),
        algorithm_version: MEMORY_LEDGER_ALGORITHM_VERSION.into(),
        coverage: MemoryLedgerCoverage {
            capability: weakest_capability.as_str().into(),
            source_coverage,
            access_count: accesses.len() as u64,
            read_count: accesses
                .iter()
                .filter(|access| access.operation == "read")
                .count() as u64,
            write_count: accesses
                .iter()
                .filter(|access| access.operation == "write")
                .count() as u64,
            evidence_count: evidence.len() as u64,
            observed_count: accesses
                .iter()
                .filter(|access| access.evidence_level == "observed")
                .count() as u64,
            derived_count: accesses
                .iter()
                .filter(|access| access.evidence_level == "derived")
                .count() as u64,
            inferred_count: accesses
                .iter()
                .filter(|access| access.evidence_level == "inferred")
                .count() as u64,
            unavailable_operations: unavailable_operations(capability),
        },
        accesses,
        evidence,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_sources_report_both_operations_as_unavailable() {
        let response = not_recorded(source_capability_for_name("claude-code"));
        assert_eq!(response.status, "not-recorded");
        assert_eq!(response.coverage.capability, "unavailable");
        assert_eq!(response.coverage.unavailable_operations, ["read", "write"]);
    }

    #[test]
    fn codex_truthfully_reports_partial_read_and_unavailable_write() {
        let response = not_recorded(source_capability_for_name("codex"));
        assert_eq!(response.coverage.capability, "partial");
        assert_eq!(response.coverage.unavailable_operations, ["write"]);
    }
}
