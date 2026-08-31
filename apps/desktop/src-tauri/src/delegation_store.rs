use crate::errors::AppResult;
use crate::governance::capabilities::{
    SignalCapability, SourceCapabilities, source_capability_for_name,
};
use crate::governance::delegation::{
    CanonicalDelegationSignal, DELEGATION_ALGORITHM_VERSION, DelegationProjection,
    ExecutionRelationProjection, RelationEvidenceProjection, project_delegation,
    valid_relation_status,
};
use crate::governance::evidence::{EvidenceLevel, merge_coverage};
use crate::models::{
    DelegationAnomaly, DelegationCoverage, DelegationEdge, DelegationEvidenceReference,
    DelegationNode, DelegationTraceResponse,
};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, Transaction, params, params_from_iter};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

const ATTENTION_NEVER_EXPIRES: &str = "9999-12-31T23:59:59Z";

#[derive(Clone, Debug)]
struct SessionMetadata {
    id: String,
    started_at: Option<String>,
    ended_at: Option<String>,
}

fn load_signals(connection: &Connection) -> AppResult<Vec<CanonicalDelegationSignal>> {
    let mut statement = connection.prepare(
        "SELECT id, agent, source_session_id, parent_session_id,
                activity_cycle_id, work_unit_id, relation_type, event_type,
                lifecycle_status, occurred_at, observed_at, event_result,
                evidence_level, source_coverage, project_label
         FROM canonical_events
         WHERE deleted_at IS NULL
           AND (relation_type IS NOT NULL OR event_type='verification.observed')
         ORDER BY COALESCE(occurred_at, observed_at), observed_at, id",
    )?;
    Ok(statement
        .query_map([], |row| {
            Ok(CanonicalDelegationSignal {
                canonical_event_id: row.get(0)?,
                agent: row.get(1)?,
                source_session_id: row.get(2)?,
                parent_session_id: row.get(3)?,
                activity_cycle_id: row.get(4)?,
                work_unit_id: row.get(5)?,
                relation_type: row.get(6)?,
                event_type: row.get(7)?,
                lifecycle_status: row.get(8)?,
                occurred_at: row.get(9)?,
                observed_at: row.get(10)?,
                event_result: row.get(11)?,
                evidence_level: row.get(12)?,
                source_coverage: row.get(13)?,
                project_label: row.get(14)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?)
}

pub(crate) fn rebuild(transaction: &Transaction<'_>, now: &str) -> AppResult<()> {
    let signals = load_signals(transaction)?;
    let projection = project_delegation(
        &signals,
        DateTime::parse_from_rfc3339(now)
            .map(|value| value.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
    );

    transaction.execute(
        "UPDATE execution_relations
         SET deleted_at=?1
         WHERE deleted_at IS NULL
           AND evidence_level<>'user-confirmed'",
        params![now],
    )?;

    for relation in &projection.relations {
        if relation.evidence.is_empty() || !valid_relation_status(&relation.status) {
            continue;
        }
        transaction.execute(
            "INSERT INTO execution_relations(
                id, root_session_id, parent_session_id, child_session_id,
                parent_work_unit_id, child_work_unit_id, relation_type, status,
                started_at, ended_at, outcome, confidence, evidence_level,
                source_coverage, algorithm_version, deleted_at
             ) VALUES(
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8,
                ?9, ?10, ?11, ?12, ?13, ?14, ?15, NULL
             )
             ON CONFLICT(id) DO UPDATE SET
                root_session_id=excluded.root_session_id,
                parent_session_id=excluded.parent_session_id,
                child_session_id=excluded.child_session_id,
                parent_work_unit_id=excluded.parent_work_unit_id,
                child_work_unit_id=excluded.child_work_unit_id,
                relation_type=excluded.relation_type,
                status=excluded.status,
                started_at=excluded.started_at,
                ended_at=excluded.ended_at,
                outcome=excluded.outcome,
                confidence=excluded.confidence,
                evidence_level=excluded.evidence_level,
                source_coverage=excluded.source_coverage,
                algorithm_version=excluded.algorithm_version,
                deleted_at=NULL
             WHERE execution_relations.evidence_level<>'user-confirmed'",
            params![
                relation.id,
                relation.root_session_id,
                relation.parent_session_id,
                relation.child_session_id,
                relation.parent_work_unit_id,
                relation.child_work_unit_id,
                relation.relation_type,
                relation.status,
                relation.started_at,
                relation.ended_at,
                relation.outcome,
                relation.confidence,
                relation.evidence_level,
                relation.source_coverage,
                relation.algorithm_version,
            ],
        )?;
        let user_confirmed: bool = transaction.query_row(
            "SELECT evidence_level='user-confirmed'
             FROM execution_relations WHERE id=?1",
            params![relation.id],
            |row| row.get(0),
        )?;
        if !user_confirmed {
            transaction.execute(
                "DELETE FROM execution_relation_evidence WHERE relation_id=?1",
                params![relation.id],
            )?;
        }
        for evidence in &relation.evidence {
            transaction.execute(
                "INSERT OR IGNORE INTO execution_relation_evidence(
                    relation_id, canonical_event_id, evidence_role, observed_at
                 ) SELECT ?1, id, ?3, ?4
                   FROM canonical_events
                  WHERE id=?2 AND deleted_at IS NULL",
                params![
                    relation.id,
                    evidence.canonical_event_id,
                    evidence.evidence_role,
                    evidence.observed_at,
                ],
            )?;
        }
    }
    sync_delegation_attention(transaction, &signals, &projection, now)
}

fn sync_delegation_attention(
    transaction: &Transaction<'_>,
    signals: &[CanonicalDelegationSignal],
    projection: &DelegationProjection,
    now: &str,
) -> AppResult<()> {
    let relations = projection
        .relations
        .iter()
        .map(|relation| (relation.id.as_str(), relation))
        .collect::<HashMap<_, _>>();
    let evidence = signals
        .iter()
        .map(|signal| (signal.canonical_event_id.as_str(), signal))
        .collect::<HashMap<_, _>>();
    let repeated_spawn_failures = projection
        .anomalies
        .iter()
        .filter(|anomaly| anomaly.kind == "spawn-failed")
        .filter_map(|anomaly| anomaly.edge_ids.first())
        .filter_map(|edge_id| relations.get(edge_id.as_str()))
        .filter(|relation| relation.source_coverage.starts_with("exact"))
        .fold(
            HashMap::<(Option<&str>, &str), usize>::new(),
            |mut counts, relation| {
                *counts
                    .entry((
                        relation.parent_session_id.as_deref(),
                        relation.root_session_id.as_str(),
                    ))
                    .or_default() += 1;
                counts
            },
        );
    let mut active_ids = Vec::new();

    for anomaly in &projection.anomalies {
        let Some(edge_id) = anomaly.edge_ids.first() else {
            continue;
        };
        let Some(relation) = relations.get(edge_id.as_str()).copied() else {
            continue;
        };
        let exact = relation.source_coverage.starts_with("exact");
        let child_error = relation.evidence.iter().any(|reference| {
            evidence
                .get(reference.canonical_event_id.as_str())
                .is_some_and(|signal| signal.event_type.ends_with(".child.error"))
        });
        let spawn_failure_count = repeated_spawn_failures
            .get(&(
                relation.parent_session_id.as_deref(),
                relation.root_session_id.as_str(),
            ))
            .copied()
            .unwrap_or_default();
        let repeated_failure = spawn_failure_count >= 3;
        let affected_branch_count = if anomaly.kind == "spawn-failed" {
            spawn_failure_count.max(1)
        } else {
            anomaly.edge_ids.len().max(1)
        };
        let takeover = match anomaly.kind.as_str() {
            "blocked-branch" | "handoff-unfinished" => exact && anomaly.confidence >= 0.9,
            "spawn-failed" => exact && (child_error || repeated_failure),
            "orphan-child" => exact && matches!(relation.status.as_str(), "running" | "waiting"),
            _ => false,
        };
        if !takeover || anomaly.evidence_ids.is_empty() {
            continue;
        }
        let Some(primary_signal) = anomaly
            .evidence_ids
            .iter()
            .find_map(|id| evidence.get(id.as_str()).copied())
        else {
            continue;
        };
        let attention_kind = if anomaly.kind == "spawn-failed" {
            "error"
        } else {
            "waiting"
        };
        let existing = transaction
            .query_row(
                "SELECT id, reason_key FROM attention_events
                 WHERE agent=?1 AND source_session_id=?2 AND kind=?3
                   AND state IN('open','acknowledged','snoozed')
                 LIMIT 1",
                params![
                    primary_signal.agent,
                    relation.child_session_id,
                    attention_kind
                ],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        let preferred_id = format!("delegation-attention-{}", anomaly.id);
        let attention_id = existing
            .as_ref()
            .map(|item| item.0.clone())
            .unwrap_or(preferred_id);
        let reason_key = format!("attention.delegation.{}", anomaly.kind);
        let opened_at = relation
            .started_at
            .as_deref()
            .unwrap_or(primary_signal.observed_at.as_str());
        let latest_evidence_at = anomaly
            .evidence_ids
            .iter()
            .filter_map(|id| evidence.get(id.as_str()))
            .map(|signal| signal.observed_at.as_str())
            .max()
            .unwrap_or(primary_signal.observed_at.as_str());
        if existing
            .as_ref()
            .is_none_or(|item| item.1.starts_with("attention.delegation."))
        {
            transaction.execute(
                "INSERT INTO attention_events(
                    id, kind, state, reason_key, agent, source_session_id,
                    project_label, opened_at, latest_evidence_at, expires_at,
                    evidence_level, source_coverage, rule_version,
                    affected_branch_count, updated_at
                 ) VALUES(
                    ?1, ?2, 'open', ?3, ?4, ?5, ?6, ?7, ?8, ?9,
                    ?10, ?11, ?12, ?13, ?14
                 )
                 ON CONFLICT(id) DO UPDATE SET
                    latest_evidence_at=excluded.latest_evidence_at,
                    expires_at=excluded.expires_at,
                    evidence_level=excluded.evidence_level,
                    source_coverage=excluded.source_coverage,
                    rule_version=excluded.rule_version,
                    affected_branch_count=excluded.affected_branch_count,
                    updated_at=excluded.updated_at,
                    state=CASE
                        WHEN attention_events.feedback IS NULL
                         AND attention_events.state IN('resolved','expired') THEN 'open'
                        ELSE attention_events.state
                    END,
                    resolved_at=CASE
                        WHEN attention_events.feedback IS NULL THEN NULL
                        ELSE attention_events.resolved_at
                    END
                 WHERE attention_events.feedback IS NULL",
                params![
                    attention_id,
                    attention_kind,
                    reason_key,
                    primary_signal.agent,
                    relation.child_session_id,
                    safe_project_label(&primary_signal.project_label),
                    opened_at,
                    latest_evidence_at,
                    ATTENTION_NEVER_EXPIRES,
                    relation.evidence_level,
                    relation.source_coverage,
                    anomaly.rule_version,
                    i64::try_from(affected_branch_count).unwrap_or(i64::MAX),
                    now,
                ],
            )?;
        }
        for canonical_event_id in &anomaly.evidence_ids {
            if let Some(signal) = evidence.get(canonical_event_id.as_str()) {
                transaction.execute(
                    "INSERT OR IGNORE INTO attention_event_evidence(
                        attention_event_id, canonical_event_id, role, observed_at
                     ) VALUES(?1, ?2, 'delegation', ?3)",
                    params![attention_id, canonical_event_id, signal.observed_at],
                )?;
            }
        }
        active_ids.push(attention_id);
    }

    let mut values = vec![now.to_string()];
    let exclusion = if active_ids.is_empty() {
        String::new()
    } else {
        values.extend(active_ids.iter().cloned());
        format!(
            " AND id NOT IN ({})",
            std::iter::repeat_n("?", active_ids.len())
                .collect::<Vec<_>>()
                .join(",")
        )
    };
    transaction.execute(
        &format!(
            "UPDATE attention_events
             SET state='resolved', resolved_at=?1,
                 resolution_reason='delegation-cleared', updated_at=?1
             WHERE reason_key LIKE 'attention.delegation.%'
               AND state IN('open','acknowledged','snoozed')
               AND feedback IS NULL{exclusion}"
        ),
        params_from_iter(values.iter()),
    )?;
    Ok(())
}

fn safe_project_label(value: &str) -> String {
    if value.contains(['/', '\\']) {
        format!("private-{}", crate::privacy::stable_hash(value))
    } else {
        value.chars().take(80).collect()
    }
}

fn not_recorded(capability: Option<&SourceCapabilities>) -> DelegationTraceResponse {
    DelegationTraceResponse {
        status: "not-recorded".into(),
        root_session_id: None,
        algorithm_version: DELEGATION_ALGORITHM_VERSION.into(),
        nodes: Vec::new(),
        edges: Vec::new(),
        anomalies: Vec::new(),
        coverage: DelegationCoverage {
            capability: capability
                .map(|item| item.delegation.as_str())
                .unwrap_or("unavailable")
                .into(),
            source_coverage: "not-recorded".into(),
            relation_count: 0,
            evidence_count: 0,
            observed_count: 0,
            derived_count: 0,
            inferred_count: 0,
            unavailable_signals: unavailable_signals(capability),
        },
        evidence: Vec::new(),
    }
}

fn unavailable_signals(capability: Option<&SourceCapabilities>) -> Vec<String> {
    let Some(capability) = capability else {
        return vec!["delegation".into(), "handoff".into(), "subagent".into()];
    };
    [
        ("delegation", capability.delegation),
        ("handoff", capability.handoff),
        ("subagent", capability.subagent),
    ]
    .into_iter()
    .filter(|(_, value)| *value != SignalCapability::Exact)
    .map(|(name, _)| name.into())
    .collect()
}

pub(crate) fn query_trace(
    connection: &Connection,
    session_id: &str,
) -> AppResult<DelegationTraceResponse> {
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
    if capability.is_none_or(|item| item.delegation == SignalCapability::Unavailable) {
        return Ok(not_recorded(capability));
    }
    let source_session_id = crate::privacy::safe_opaque_identifier(&raw_source_session_id);
    let Some(root_session_id) = connection
        .query_row(
            "SELECT relation.root_session_id
             FROM execution_relations relation
             JOIN execution_relation_evidence evidence
               ON evidence.relation_id=relation.id
             JOIN canonical_events canonical
               ON canonical.id=evidence.canonical_event_id
             WHERE relation.deleted_at IS NULL
               AND (canonical.deleted_at IS NULL
                    OR relation.evidence_level='user-confirmed')
               AND canonical.agent=?1
               AND (relation.root_session_id=?2
                    OR relation.parent_session_id=?2
                    OR relation.child_session_id=?2)
             ORDER BY relation.root_session_id, relation.id
             LIMIT 1",
            params![agent, source_session_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
    else {
        return Ok(not_recorded(capability));
    };

    let (relations, mut signals, mut evidence_references) =
        load_root_relations(connection, &agent, &root_session_id)?;
    if relations.is_empty() || evidence_references.is_empty() {
        return Ok(not_recorded(capability));
    }
    let identities = relation_identities(&root_session_id, &relations);
    load_verification_signals(connection, &agent, &identities, &mut signals)?;
    let session_metadata = load_session_metadata(connection, &agent, &identities)?;
    for reference in &mut evidence_references {
        if reference.session_id.is_none()
            && let Some(signal) = signals
                .iter()
                .find(|signal| signal.canonical_event_id == reference.canonical_event_id)
            && let Some(metadata) = session_metadata.get(&signal.source_session_id)
        {
            reference.session_id = Some(metadata.id.clone());
        }
    }

    let projected = project_delegation(&signals, Utc::now());
    let relation_ids = relations
        .iter()
        .map(|relation| relation.id.as_str())
        .collect::<HashSet<_>>();
    let anomalies = projected
        .anomalies
        .into_iter()
        .filter(|anomaly| {
            anomaly
                .edge_ids
                .iter()
                .any(|edge_id| relation_ids.contains(edge_id.as_str()))
        })
        .map(|anomaly| DelegationAnomaly {
            id: anomaly.id,
            kind: anomaly.kind,
            severity: anomaly.severity,
            node_ids: anomaly.node_ids,
            edge_ids: anomaly.edge_ids,
            reason_key: anomaly.reason_key,
            evidence_ids: anomaly.evidence_ids,
            rule_version: anomaly.rule_version,
            confidence: anomaly.confidence,
        })
        .collect::<Vec<_>>();
    let nodes = build_nodes(
        capability.expect("capability checked above"),
        &agent,
        &root_session_id,
        &identities,
        &relations,
        &session_metadata,
    );
    let edges = relations
        .iter()
        .map(|relation| DelegationEdge {
            id: relation.id.clone(),
            from: relation
                .parent_session_id
                .as_deref()
                .map(|parent| stable_node_id(&agent, parent))
                .unwrap_or_else(|| stable_node_id(&agent, &root_session_id)),
            to: stable_node_id(&agent, &relation.child_session_id),
            relation_type: relation.relation_type.clone(),
            status: relation.status.clone(),
            confidence: relation.confidence,
            evidence_level: relation.evidence_level.clone(),
            source_coverage: relation.source_coverage.clone(),
            algorithm_version: relation.algorithm_version.clone(),
            evidence_ids: relation
                .evidence
                .iter()
                .map(|item| item.canonical_event_id.clone())
                .collect(),
        })
        .collect::<Vec<_>>();
    let observed_count = relations
        .iter()
        .filter(|relation| relation.evidence_level == "observed")
        .count() as u64;
    let derived_count = relations
        .iter()
        .filter(|relation| relation.evidence_level == "derived")
        .count() as u64;
    let inferred_count = relations
        .iter()
        .filter(|relation| relation.evidence_level == "inferred")
        .count() as u64;
    let source_coverage = merge_coverage(
        relations
            .iter()
            .map(|relation| relation.source_coverage.as_str()),
    );
    let ready = capability.is_some_and(|item| item.delegation == SignalCapability::Exact)
        && relations.iter().all(|relation| {
            relation.source_coverage.starts_with("exact")
                && matches!(
                    relation.evidence_level.as_str(),
                    "observed" | "user-confirmed"
                )
        });
    evidence_references.sort_by(|left, right| {
        left.occurred_at
            .cmp(&right.occurred_at)
            .then_with(|| left.canonical_event_id.cmp(&right.canonical_event_id))
    });
    evidence_references.dedup_by(|left, right| {
        left.canonical_event_id == right.canonical_event_id && left.role == right.role
    });
    Ok(DelegationTraceResponse {
        status: if ready { "ready" } else { "partial" }.into(),
        root_session_id: session_metadata
            .get(&root_session_id)
            .map(|metadata| metadata.id.clone()),
        algorithm_version: DELEGATION_ALGORITHM_VERSION.into(),
        nodes,
        edges,
        anomalies,
        coverage: DelegationCoverage {
            capability: capability
                .map(|item| item.delegation.as_str())
                .unwrap_or("unavailable")
                .into(),
            source_coverage,
            relation_count: relations.len() as u64,
            evidence_count: evidence_references.len() as u64,
            observed_count,
            derived_count,
            inferred_count,
            unavailable_signals: unavailable_signals(capability),
        },
        evidence: evidence_references,
    })
}

fn load_root_relations(
    connection: &Connection,
    agent: &str,
    root_session_id: &str,
) -> AppResult<(
    Vec<ExecutionRelationProjection>,
    Vec<CanonicalDelegationSignal>,
    Vec<DelegationEvidenceReference>,
)> {
    let mut statement = connection.prepare(
        "SELECT relation.id, relation.root_session_id, relation.parent_session_id,
                relation.child_session_id, relation.parent_work_unit_id,
                relation.child_work_unit_id, relation.relation_type, relation.status,
                relation.started_at, relation.ended_at, relation.outcome,
                relation.confidence, relation.evidence_level, relation.source_coverage,
                relation.algorithm_version,
                evidence.canonical_event_id, evidence.evidence_role, evidence.observed_at,
                canonical.agent, canonical.source_session_id, canonical.parent_session_id,
                canonical.activity_cycle_id, canonical.work_unit_id, canonical.relation_type,
                canonical.event_type, canonical.lifecycle_status, canonical.occurred_at,
                canonical.observed_at, canonical.event_result, canonical.evidence_level,
                canonical.source_coverage, canonical.project_label,
                canonical.history_session_id
         FROM execution_relations relation
         JOIN execution_relation_evidence evidence ON evidence.relation_id=relation.id
         JOIN canonical_events canonical ON canonical.id=evidence.canonical_event_id
         WHERE relation.root_session_id=?1
           AND relation.deleted_at IS NULL
           AND (canonical.deleted_at IS NULL
                OR relation.evidence_level='user-confirmed')
           AND canonical.agent=?2
         ORDER BY relation.id, evidence.observed_at, evidence.canonical_event_id",
    )?;
    let rows = statement
        .query_map(params![root_session_id, agent], |row| {
            let relation = ExecutionRelationProjection {
                id: row.get(0)?,
                root_session_id: row.get(1)?,
                parent_session_id: row.get(2)?,
                child_session_id: row.get(3)?,
                parent_work_unit_id: row.get(4)?,
                child_work_unit_id: row.get(5)?,
                relation_type: row.get(6)?,
                status: row.get(7)?,
                started_at: row.get(8)?,
                ended_at: row.get(9)?,
                outcome: row.get(10)?,
                confidence: row.get(11)?,
                evidence_level: row.get(12)?,
                source_coverage: row.get(13)?,
                algorithm_version: row.get(14)?,
                evidence: Vec::new(),
            };
            let evidence_projection = RelationEvidenceProjection {
                canonical_event_id: row.get(15)?,
                evidence_role: row.get(16)?,
                observed_at: row.get(17)?,
            };
            let signal = CanonicalDelegationSignal {
                canonical_event_id: row.get(15)?,
                agent: row.get(18)?,
                source_session_id: row.get(19)?,
                parent_session_id: row.get(20)?,
                activity_cycle_id: row.get(21)?,
                work_unit_id: row.get(22)?,
                relation_type: row.get(23)?,
                event_type: row.get(24)?,
                lifecycle_status: row.get(25)?,
                occurred_at: row.get(26)?,
                observed_at: row.get(27)?,
                event_result: row.get(28)?,
                evidence_level: row.get(29)?,
                source_coverage: row.get(30)?,
                project_label: row.get(31)?,
            };
            let reference = DelegationEvidenceReference {
                id: format!(
                    "evidence-{}",
                    crate::privacy::stable_hash(&evidence_projection.canonical_event_id)
                ),
                canonical_event_id: evidence_projection.canonical_event_id.clone(),
                session_id: row.get(32)?,
                role: evidence_projection.evidence_role.clone(),
                occurred_at: signal.occurred_at.clone(),
                event_type: signal.event_type.clone(),
                evidence_level: signal.evidence_level.clone(),
                source_coverage: signal.source_coverage.clone(),
            };
            Ok((relation, evidence_projection, signal, reference))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut relations = BTreeMap::<String, ExecutionRelationProjection>::new();
    let mut signals = BTreeMap::<String, CanonicalDelegationSignal>::new();
    let mut references = Vec::new();
    for (relation, evidence_projection, signal, reference) in rows {
        relations
            .entry(relation.id.clone())
            .or_insert(relation)
            .evidence
            .push(evidence_projection);
        signals
            .entry(signal.canonical_event_id.clone())
            .or_insert(signal);
        references.push(reference);
    }
    Ok((
        relations.into_values().collect(),
        signals.into_values().collect(),
        references,
    ))
}

fn relation_identities(
    root_session_id: &str,
    relations: &[ExecutionRelationProjection],
) -> BTreeSet<String> {
    let mut identities = BTreeSet::from([root_session_id.to_string()]);
    for relation in relations {
        if let Some(parent) = &relation.parent_session_id {
            identities.insert(parent.clone());
        }
        identities.insert(relation.child_session_id.clone());
    }
    identities
}

fn load_verification_signals(
    connection: &Connection,
    agent: &str,
    identities: &BTreeSet<String>,
    signals: &mut Vec<CanonicalDelegationSignal>,
) -> AppResult<()> {
    let mut existing = signals
        .iter()
        .map(|signal| signal.canonical_event_id.clone())
        .collect::<HashSet<_>>();
    for chunk in identities.iter().collect::<Vec<_>>().chunks(400) {
        let placeholders = std::iter::repeat_n("?", chunk.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT id, agent, source_session_id, parent_session_id,
                    activity_cycle_id, work_unit_id, relation_type, event_type,
                    lifecycle_status, occurred_at, observed_at, event_result,
                    evidence_level, source_coverage, project_label
             FROM canonical_events
             WHERE deleted_at IS NULL AND agent=?
               AND event_type='verification.observed'
               AND source_session_id IN ({placeholders})"
        );
        let values = std::iter::once(agent.to_string())
            .chain(chunk.iter().map(|value| value.to_string()))
            .collect::<Vec<_>>();
        let mut statement = connection.prepare(&sql)?;
        let rows = statement
            .query_map(params_from_iter(values.iter()), |row| {
                Ok(CanonicalDelegationSignal {
                    canonical_event_id: row.get(0)?,
                    agent: row.get(1)?,
                    source_session_id: row.get(2)?,
                    parent_session_id: row.get(3)?,
                    activity_cycle_id: row.get(4)?,
                    work_unit_id: row.get(5)?,
                    relation_type: row.get(6)?,
                    event_type: row.get(7)?,
                    lifecycle_status: row.get(8)?,
                    occurred_at: row.get(9)?,
                    observed_at: row.get(10)?,
                    event_result: row.get(11)?,
                    evidence_level: row.get(12)?,
                    source_coverage: row.get(13)?,
                    project_label: row.get(14)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        for signal in rows {
            if existing.insert(signal.canonical_event_id.clone()) {
                signals.push(signal);
            }
        }
    }
    Ok(())
}

fn load_session_metadata(
    connection: &Connection,
    agent: &str,
    identities: &BTreeSet<String>,
) -> AppResult<HashMap<String, SessionMetadata>> {
    let mut metadata = HashMap::new();
    for chunk in identities.iter().collect::<Vec<_>>().chunks(400) {
        let placeholders = std::iter::repeat_n("?", chunk.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT id, source_session_id, started_at, ended_at
             FROM sessions
             WHERE agent=? AND source_session_id IN ({placeholders})"
        );
        let values = std::iter::once(agent.to_string())
            .chain(chunk.iter().map(|value| value.to_string()))
            .collect::<Vec<_>>();
        let mut statement = connection.prepare(&sql)?;
        for item in statement
            .query_map(params_from_iter(values.iter()), |row| {
                Ok((
                    row.get::<_, String>(1)?,
                    SessionMetadata {
                        id: row.get(0)?,
                        started_at: row.get(2)?,
                        ended_at: row.get(3)?,
                    },
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?
        {
            metadata.insert(crate::privacy::safe_opaque_identifier(&item.0), item.1);
        }
    }
    Ok(metadata)
}

fn build_nodes(
    capability: &SourceCapabilities,
    agent: &str,
    root_session_id: &str,
    identities: &BTreeSet<String>,
    relations: &[ExecutionRelationProjection],
    metadata: &HashMap<String, SessionMetadata>,
) -> Vec<DelegationNode> {
    identities
        .iter()
        .map(|identity| {
            let touching = relations
                .iter()
                .filter(|relation| {
                    relation.parent_session_id.as_deref() == Some(identity.as_str())
                        || relation.child_session_id == *identity
                })
                .collect::<Vec<_>>();
            let incoming = touching
                .iter()
                .copied()
                .filter(|relation| relation.child_session_id == *identity)
                .collect::<Vec<_>>();
            let session_metadata = metadata.get(identity);
            let status = if incoming.is_empty()
                && session_metadata
                    .and_then(|item| item.ended_at.as_ref())
                    .is_some()
            {
                "completed".into()
            } else {
                node_status(
                    if incoming.is_empty() {
                        &touching
                    } else {
                        &incoming
                    },
                    session_metadata,
                )
            };
            let evidence_level = touching
                .iter()
                .map(|relation| EvidenceLevel::parse(&relation.evidence_level))
                .max()
                .unwrap_or(EvidenceLevel::Unavailable);
            let source_coverage = merge_coverage(
                touching
                    .iter()
                    .map(|relation| relation.source_coverage.as_str()),
            );
            let confidence = touching
                .iter()
                .map(|relation| relation.confidence)
                .reduce(f64::min)
                .unwrap_or_default();
            let started_at = metadata
                .get(identity)
                .and_then(|item| item.started_at.clone())
                .or_else(|| {
                    touching
                        .iter()
                        .filter_map(|item| item.started_at.clone())
                        .min()
                });
            let ended_at = metadata
                .get(identity)
                .and_then(|item| item.ended_at.clone())
                .or_else(|| {
                    touching
                        .iter()
                        .filter_map(|item| item.ended_at.clone())
                        .max()
                });
            let short_id = crate::privacy::stable_hash(identity)
                .chars()
                .take(8)
                .collect::<String>();
            DelegationNode {
                id: stable_node_id(agent, identity),
                kind: if identity == root_session_id {
                    "root-agent".into()
                } else if incoming
                    .iter()
                    .any(|relation| matches!(relation.relation_type.as_str(), "handoff" | "resume"))
                {
                    "agent".into()
                } else {
                    "subagent".into()
                },
                agent: agent.into(),
                safe_label: format!("{} · {short_id}", capability.display_name),
                session_id: metadata.get(identity).map(|item| item.id.clone()),
                work_unit_id: incoming
                    .iter()
                    .find_map(|relation| relation.child_work_unit_id.clone()),
                status,
                started_at,
                ended_at,
                outcome: incoming
                    .iter()
                    .rev()
                    .find_map(|relation| relation.outcome.clone()),
                evidence_level: evidence_level.as_str().into(),
                source_coverage,
                confidence,
            }
        })
        .collect()
}

fn node_status(
    relations: &[&ExecutionRelationProjection],
    metadata: Option<&SessionMetadata>,
) -> String {
    for status in ["failed", "waiting", "running", "started", "completed"] {
        if relations.iter().any(|relation| relation.status == status) {
            return status.into();
        }
    }
    if metadata.and_then(|item| item.ended_at.as_ref()).is_some() {
        "completed".into()
    } else if metadata.is_some() {
        "running".into()
    } else {
        "unknown".into()
    }
}

fn stable_node_id(agent: &str, source_session_id: &str) -> String {
    format!(
        "node-{}",
        crate::privacy::stable_hash(&format!("{agent}|{source_session_id}"))
    )
}
