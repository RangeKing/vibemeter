use super::evidence::{EvidenceLevel, exact_coverage, merge_coverage};
use chrono::{DateTime, Duration, Utc};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

pub const DELEGATION_ALGORITHM_VERSION: &str = "delegation-trace-1.0.0";
pub const DELEGATION_ANOMALY_RULE_VERSION: &str = "delegation-anomaly-1.0.0";

const RELATION_TYPES: [&str; 5] = ["delegate", "spawn", "handoff", "resume", "join"];
const STATUS_TYPES: [&str; 6] = [
    "started",
    "running",
    "waiting",
    "failed",
    "completed",
    "unknown",
];

#[derive(Clone, Debug, PartialEq)]
pub struct CanonicalDelegationSignal {
    pub canonical_event_id: String,
    pub agent: String,
    pub source_session_id: String,
    pub parent_session_id: Option<String>,
    pub activity_cycle_id: Option<String>,
    pub work_unit_id: Option<String>,
    pub relation_type: Option<String>,
    pub event_type: String,
    pub lifecycle_status: String,
    pub occurred_at: Option<String>,
    pub observed_at: String,
    pub event_result: Option<String>,
    pub evidence_level: String,
    pub source_coverage: String,
    pub project_label: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RelationEvidenceProjection {
    pub canonical_event_id: String,
    pub evidence_role: String,
    pub observed_at: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExecutionRelationProjection {
    pub id: String,
    pub root_session_id: String,
    pub parent_session_id: Option<String>,
    pub child_session_id: String,
    pub parent_work_unit_id: Option<String>,
    pub child_work_unit_id: Option<String>,
    pub relation_type: String,
    pub status: String,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub outcome: Option<String>,
    pub confidence: f64,
    pub evidence_level: String,
    pub source_coverage: String,
    pub algorithm_version: String,
    pub evidence: Vec<RelationEvidenceProjection>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DelegationAnomalyProjection {
    pub id: String,
    pub kind: String,
    pub severity: String,
    pub node_ids: Vec<String>,
    pub edge_ids: Vec<String>,
    pub reason_key: String,
    pub evidence_ids: Vec<String>,
    pub rule_version: String,
    pub confidence: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DelegationProjection {
    pub relations: Vec<ExecutionRelationProjection>,
    pub anomalies: Vec<DelegationAnomalyProjection>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct RelationKey {
    parent_session_id: Option<String>,
    child_session_id: String,
    relation_type: String,
    parent_work_unit_id: Option<String>,
    child_work_unit_id: Option<String>,
}

pub fn valid_relation_type(value: &str) -> bool {
    RELATION_TYPES.contains(&value)
}

pub fn valid_relation_status(value: &str) -> bool {
    STATUS_TYPES.contains(&value)
}

fn timestamp(value: Option<&str>) -> Option<DateTime<Utc>> {
    value
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc))
}

fn relation_event_status(signal: &CanonicalDelegationSignal) -> &'static str {
    let event_type = signal.event_type.as_str();
    if event_type.ends_with(".failed")
        || event_type.ends_with(".error")
        || signal.lifecycle_status == "error"
        || signal.event_result.as_deref() == Some("failed")
    {
        "failed"
    } else if event_type.ends_with(".waiting") {
        "waiting"
    } else if event_type.ends_with(".completed") || event_type.ends_with(".stopped") {
        "completed"
    } else if event_type.ends_with(".started") {
        "started"
    } else if event_type.ends_with(".running") {
        "running"
    } else if signal.lifecycle_status == "waiting" {
        "waiting"
    } else if signal.lifecycle_status == "completed"
        || signal.event_result.as_deref() == Some("succeeded")
    {
        "completed"
    } else if signal.lifecycle_status == "running" {
        "running"
    } else {
        "unknown"
    }
}

fn evidence_role(signal: &CanonicalDelegationSignal) -> String {
    let status = relation_event_status(signal);
    match status {
        "started" => "start",
        "running" | "waiting" => "lifecycle",
        "completed" | "failed" => "outcome",
        _ if signal.event_type == "verification.observed" => "verification",
        _ => "supporting",
    }
    .into()
}

fn stable_hash(material: &str) -> String {
    crate::privacy::stable_hash(material)
}

fn relation_id(key: &RelationKey, root: &str, agent: &str) -> String {
    format!(
        "relation-{}",
        stable_hash(&format!(
            "{}|{}|{}|{}|{}|{}|{}|{}",
            DELEGATION_ALGORITHM_VERSION,
            agent,
            root,
            key.parent_session_id.as_deref().unwrap_or("orphan"),
            key.child_session_id,
            key.parent_work_unit_id.as_deref().unwrap_or_default(),
            key.child_work_unit_id.as_deref().unwrap_or_default(),
            key.relation_type,
        ))
    )
}

fn node_id(agent: &str, session_id: &str) -> String {
    format!("node-{}", stable_hash(&format!("{agent}|{session_id}")))
}

fn root_for(child: &str, parents: &HashMap<String, BTreeSet<String>>) -> String {
    let mut current = child.to_string();
    let mut path = Vec::new();
    let mut positions = HashMap::new();
    loop {
        if let Some(cycle_start) = positions.get(&current).copied() {
            return path[cycle_start..].iter().min().cloned().unwrap_or(current);
        }
        positions.insert(current.clone(), path.len());
        path.push(current.clone());
        let Some(parent) = parents.get(&current).and_then(|items| items.first()) else {
            return current;
        };
        current = parent.clone();
    }
}

fn merged_status(signals: &[&CanonicalDelegationSignal]) -> String {
    if signals
        .iter()
        .any(|signal| relation_event_status(signal) == "failed")
    {
        return "failed".into();
    }
    if signals
        .iter()
        .any(|signal| relation_event_status(signal) == "completed")
    {
        return "completed".into();
    }
    signals
        .iter()
        .rev()
        .map(|signal| relation_event_status(signal))
        .find(|status| *status != "unknown")
        .unwrap_or("unknown")
        .into()
}

fn merged_evidence_level(
    parent_session_id: Option<&str>,
    signals: &[&CanonicalDelegationSignal],
) -> EvidenceLevel {
    if parent_session_id.is_none() {
        return EvidenceLevel::Inferred;
    }
    signals
        .iter()
        .map(|signal| EvidenceLevel::parse(&signal.evidence_level))
        .max()
        .unwrap_or(EvidenceLevel::Unavailable)
}

fn anomaly(
    kind: &str,
    severity: &str,
    nodes: Vec<String>,
    edges: Vec<String>,
    evidence_ids: Vec<String>,
    confidence: f64,
) -> DelegationAnomalyProjection {
    let mut evidence_ids = evidence_ids;
    evidence_ids.sort();
    evidence_ids.dedup();
    let mut edges_for_id = edges.clone();
    edges_for_id.sort();
    DelegationAnomalyProjection {
        id: format!(
            "delegation-anomaly-{}",
            stable_hash(&format!(
                "{}|{}|{}|{}",
                DELEGATION_ANOMALY_RULE_VERSION,
                kind,
                edges_for_id.join(","),
                evidence_ids.join(","),
            ))
        ),
        kind: kind.into(),
        severity: severity.into(),
        node_ids: nodes,
        edge_ids: edges,
        reason_key: format!("delegation.anomaly.{kind}"),
        evidence_ids,
        rule_version: DELEGATION_ANOMALY_RULE_VERSION.into(),
        confidence,
    }
}

pub fn project_delegation(
    signals: &[CanonicalDelegationSignal],
    now: DateTime<Utc>,
) -> DelegationProjection {
    let active = signals
        .iter()
        .filter(|signal| !signal.canonical_event_id.is_empty())
        .collect::<Vec<_>>();
    let mut parents = HashMap::<String, BTreeSet<String>>::new();
    for signal in &active {
        if signal
            .relation_type
            .as_deref()
            .is_some_and(valid_relation_type)
            && let Some(parent) = signal.parent_session_id.as_deref()
            && parent != signal.source_session_id
        {
            parents
                .entry(signal.source_session_id.clone())
                .or_default()
                .insert(parent.to_string());
        }
    }

    let mut groups = BTreeMap::<RelationKey, Vec<&CanonicalDelegationSignal>>::new();
    for signal in &active {
        let Some(relation_type) = signal
            .relation_type
            .as_deref()
            .filter(|value| valid_relation_type(value))
        else {
            continue;
        };
        let key = RelationKey {
            parent_session_id: signal.parent_session_id.clone(),
            child_session_id: signal.source_session_id.clone(),
            relation_type: relation_type.into(),
            parent_work_unit_id: None,
            child_work_unit_id: signal.work_unit_id.clone(),
        };
        groups.entry(key).or_default().push(signal);
    }

    let mut relations = Vec::new();
    for (key, mut group) in groups {
        group.sort_by(|left, right| {
            timestamp(left.occurred_at.as_deref())
                .cmp(&timestamp(right.occurred_at.as_deref()))
                .then_with(|| left.observed_at.cmp(&right.observed_at))
                .then_with(|| left.canonical_event_id.cmp(&right.canonical_event_id))
        });
        let root = key
            .parent_session_id
            .as_deref()
            .map(|parent| root_for(parent, &parents))
            .unwrap_or_else(|| key.child_session_id.clone());
        let status = merged_status(&group);
        let started_at = group
            .iter()
            .filter(|signal| {
                matches!(
                    relation_event_status(signal),
                    "started" | "running" | "waiting"
                )
            })
            .filter_map(|signal| signal.occurred_at.clone())
            .min()
            .or_else(|| {
                group
                    .iter()
                    .filter_map(|signal| signal.occurred_at.clone())
                    .min()
            });
        let ended_at = group
            .iter()
            .filter(|signal| matches!(relation_event_status(signal), "failed" | "completed"))
            .filter_map(|signal| signal.occurred_at.clone())
            .max();
        let outcome = group
            .iter()
            .rev()
            .find_map(|signal| signal.event_result.clone())
            .or_else(|| match status.as_str() {
                "completed" => Some("completed".into()),
                "failed" => Some("failed".into()),
                _ => None,
            });
        let evidence_level = merged_evidence_level(key.parent_session_id.as_deref(), &group);
        let source_coverage =
            merge_coverage(group.iter().map(|signal| signal.source_coverage.as_str()));
        let confidence =
            if evidence_level != EvidenceLevel::Inferred && !exact_coverage(&source_coverage) {
                evidence_level.confidence().min(0.74)
            } else {
                evidence_level.confidence()
            };
        let mut evidence = group
            .iter()
            .map(|signal| RelationEvidenceProjection {
                canonical_event_id: signal.canonical_event_id.clone(),
                evidence_role: evidence_role(signal),
                observed_at: signal.observed_at.clone(),
            })
            .collect::<Vec<_>>();
        evidence.sort_by(|left, right| {
            left.observed_at
                .cmp(&right.observed_at)
                .then_with(|| left.canonical_event_id.cmp(&right.canonical_event_id))
        });
        evidence.dedup_by(|left, right| left.canonical_event_id == right.canonical_event_id);
        relations.push(ExecutionRelationProjection {
            id: relation_id(&key, &root, &group[0].agent),
            root_session_id: root,
            parent_session_id: key.parent_session_id,
            child_session_id: key.child_session_id,
            parent_work_unit_id: key.parent_work_unit_id,
            child_work_unit_id: key.child_work_unit_id,
            relation_type: key.relation_type,
            status,
            started_at,
            ended_at,
            outcome,
            confidence,
            evidence_level: evidence_level.as_str().into(),
            source_coverage,
            algorithm_version: DELEGATION_ALGORITHM_VERSION.into(),
            evidence,
        });
    }
    let terminal_relations = relations
        .iter()
        .filter(|relation| {
            (relation.relation_type == "resume" || relation.relation_type == "join")
                && relation.status == "completed"
                || relation.relation_type == "resume"
                    && matches!(relation.status.as_str(), "started" | "running")
        })
        .map(|relation| {
            (
                (
                    relation.parent_session_id.clone(),
                    relation.child_session_id.clone(),
                    relation.relation_type.clone(),
                ),
                (
                    relation
                        .started_at
                        .clone()
                        .or_else(|| relation.ended_at.clone()),
                    relation.evidence.clone(),
                ),
            )
        })
        .collect::<HashMap<_, _>>();
    for relation in &mut relations {
        let terminal_type = match relation.relation_type.as_str() {
            "handoff" => "resume",
            "spawn" => "join",
            _ => continue,
        };
        let Some((ended_at, evidence)) = terminal_relations.get(&(
            relation.parent_session_id.clone(),
            relation.child_session_id.clone(),
            terminal_type.to_string(),
        )) else {
            continue;
        };
        relation.status = "completed".into();
        relation.ended_at = ended_at.clone();
        relation.outcome = Some(if terminal_type == "resume" {
            "resumed".into()
        } else {
            "returned".into()
        });
        relation
            .evidence
            .extend(evidence.iter().cloned().map(|mut item| {
                item.evidence_role = "outcome".into();
                item
            }));
        relation
            .evidence
            .sort_by(|left, right| left.canonical_event_id.cmp(&right.canonical_event_id));
        relation
            .evidence
            .dedup_by(|left, right| left.canonical_event_id == right.canonical_event_id);
    }
    relations.sort_by(|left, right| left.id.cmp(&right.id));

    let successful_verification = active
        .iter()
        .filter(|signal| {
            signal.event_type == "verification.observed"
                && signal.event_result.as_deref() != Some("failed")
        })
        .map(|signal| signal.source_session_id.as_str())
        .collect::<HashSet<_>>();
    let joined_children = relations
        .iter()
        .filter(|relation| relation.relation_type == "join" && relation.status == "completed")
        .map(|relation| relation.child_session_id.as_str())
        .collect::<HashSet<_>>();
    let mut anomalies = Vec::new();
    for relation in &relations {
        let agent = signals
            .iter()
            .find(|signal| {
                signal.source_session_id == relation.child_session_id
                    && signal.relation_type.as_deref() == Some(relation.relation_type.as_str())
            })
            .map(|signal| signal.agent.as_str())
            .unwrap_or("unknown");
        let child_node = node_id(agent, &relation.child_session_id);
        let mut nodes = vec![child_node];
        if let Some(parent) = relation.parent_session_id.as_deref() {
            nodes.insert(0, node_id(agent, parent));
        }
        let evidence_ids = relation
            .evidence
            .iter()
            .map(|evidence| evidence.canonical_event_id.clone())
            .collect::<Vec<_>>();
        let exact = exact_coverage(&relation.source_coverage);
        if relation.parent_session_id.is_none()
            && matches!(relation.status.as_str(), "started" | "running" | "waiting")
        {
            anomalies.push(anomaly(
                "orphan-child",
                if relation.status == "waiting" {
                    "warning"
                } else {
                    "info"
                },
                nodes.clone(),
                vec![relation.id.clone()],
                evidence_ids.clone(),
                relation.confidence.min(0.6),
            ));
        }
        if relation.relation_type == "spawn" && relation.status == "failed" {
            anomalies.push(anomaly(
                "spawn-failed",
                if exact { "warning" } else { "info" },
                nodes.clone(),
                vec![relation.id.clone()],
                evidence_ids.clone(),
                if exact { 0.96 } else { 0.55 },
            ));
        }
        if relation.status == "waiting" {
            anomalies.push(anomaly(
                "blocked-branch",
                if exact { "warning" } else { "info" },
                nodes.clone(),
                vec![relation.id.clone()],
                evidence_ids.clone(),
                if exact { 0.95 } else { 0.55 },
            ));
        }
        if relation.relation_type == "handoff"
            && matches!(relation.status.as_str(), "started" | "running" | "waiting")
            && relation
                .started_at
                .as_deref()
                .and_then(|value| timestamp(Some(value)))
                .is_some_and(|started| now.signed_duration_since(started) >= Duration::minutes(10))
        {
            anomalies.push(anomaly(
                "handoff-unfinished",
                if exact { "warning" } else { "info" },
                nodes.clone(),
                vec![relation.id.clone()],
                evidence_ids.clone(),
                if exact { 0.92 } else { 0.5 },
            ));
        }
        if relation.status == "completed"
            && !successful_verification.contains(relation.child_session_id.as_str())
            && !joined_children.contains(relation.child_session_id.as_str())
        {
            anomalies.push(anomaly(
                "unverified-completion",
                "info",
                nodes.clone(),
                vec![relation.id.clone()],
                evidence_ids.clone(),
                if exact { 0.72 } else { 0.5 },
            ));
        }
        let start_evidence = relation
            .evidence
            .iter()
            .filter(|evidence| evidence.evidence_role == "start")
            .map(|evidence| evidence.canonical_event_id.clone())
            .collect::<Vec<_>>();
        if start_evidence.len() > 2 {
            anomalies.push(anomaly(
                "duplicate-delegation",
                "info",
                nodes,
                vec![relation.id.clone()],
                start_evidence,
                if exact { 0.85 } else { 0.5 },
            ));
        }
    }

    let mut fanout = BTreeMap::<(String, String), Vec<&ExecutionRelationProjection>>::new();
    for relation in &relations {
        if relation.relation_type == "spawn"
            && relation.parent_session_id.is_some()
            && matches!(relation.status.as_str(), "started" | "running" | "waiting")
            && exact_coverage(&relation.source_coverage)
        {
            fanout
                .entry((
                    relation.root_session_id.clone(),
                    relation.parent_session_id.clone().unwrap_or_default(),
                ))
                .or_default()
                .push(relation);
        }
    }
    for ((_root, parent), mut branches) in fanout {
        branches.sort_by(|left, right| left.started_at.cmp(&right.started_at));
        if branches.len() < 6 {
            continue;
        }
        let first = branches
            .first()
            .and_then(|relation| timestamp(relation.started_at.as_deref()));
        let last = branches
            .last()
            .and_then(|relation| timestamp(relation.started_at.as_deref()));
        if first
            .zip(last)
            .is_some_and(|(first, last)| last - first <= Duration::minutes(2))
        {
            let agent = signals
                .iter()
                .find(|signal| signal.parent_session_id.as_deref() == Some(parent.as_str()))
                .map(|signal| signal.agent.as_str())
                .unwrap_or("unknown");
            anomalies.push(anomaly(
                "fanout-spike",
                "warning",
                std::iter::once(node_id(agent, &parent))
                    .chain(
                        branches
                            .iter()
                            .map(|branch| node_id(agent, &branch.child_session_id)),
                    )
                    .collect(),
                branches.iter().map(|branch| branch.id.clone()).collect(),
                branches
                    .iter()
                    .flat_map(|branch| {
                        branch
                            .evidence
                            .iter()
                            .filter(|evidence| evidence.evidence_role == "start")
                            .map(|evidence| evidence.canonical_event_id.clone())
                    })
                    .collect(),
                0.9,
            ));
        }
    }
    anomalies.sort_by(|left, right| left.id.cmp(&right.id));
    anomalies.dedup_by(|left, right| left.id == right.id);

    DelegationProjection {
        relations,
        anomalies,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn signal(
        id: &str,
        parent: Option<&str>,
        child: &str,
        relation_type: &str,
        event_type: &str,
        at: &str,
        coverage: &str,
    ) -> CanonicalDelegationSignal {
        CanonicalDelegationSignal {
            canonical_event_id: id.into(),
            agent: "codex".into(),
            source_session_id: child.into(),
            parent_session_id: parent.map(str::to_string),
            activity_cycle_id: None,
            work_unit_id: None,
            relation_type: Some(relation_type.into()),
            event_type: event_type.into(),
            lifecycle_status: if event_type.ends_with("waiting") {
                "waiting"
            } else if event_type.ends_with("failed") {
                "error"
            } else if event_type.ends_with("completed") {
                "completed"
            } else {
                "running"
            }
            .into(),
            occurred_at: Some(at.into()),
            observed_at: at.into(),
            event_result: event_type
                .ends_with("completed")
                .then(|| "succeeded".into())
                .or_else(|| event_type.ends_with("failed").then(|| "failed".into())),
            evidence_level: if coverage.starts_with("exact") {
                "observed"
            } else {
                "derived"
            }
            .into(),
            source_coverage: coverage.into(),
            project_label: "fixture-project".into(),
        }
    }

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 30, 12, 30, 0)
            .single()
            .expect("time")
    }

    #[test]
    fn single_and_parallel_children_keep_stable_identity_across_record_reorder() {
        let mut signals = vec![
            signal(
                "a",
                Some("root"),
                "child-a",
                "spawn",
                "delegation.spawn.started",
                "2026-08-30T12:00:00Z",
                "exact-delegation",
            ),
            signal(
                "b",
                Some("root"),
                "child-b",
                "spawn",
                "delegation.spawn.started",
                "2026-08-30T12:00:01Z",
                "exact-delegation",
            ),
            signal(
                "c",
                Some("root"),
                "child-c",
                "spawn",
                "delegation.spawn.started",
                "2026-08-30T12:00:02Z",
                "exact-delegation",
            ),
        ];
        let first = project_delegation(&signals, now());
        signals.reverse();
        let reordered = project_delegation(&signals, now());
        assert_eq!(first.relations, reordered.relations);
        assert_eq!(first.relations.len(), 3);
        assert!(
            first
                .relations
                .iter()
                .all(|relation| relation.root_session_id == "root")
        );
        assert!(
            !first
                .anomalies
                .iter()
                .any(|item| item.kind == "fanout-spike")
        );
    }

    #[test]
    fn completion_failure_waiting_handoff_resume_and_join_are_deterministic() {
        let signals = vec![
            signal(
                "start",
                Some("root"),
                "child",
                "spawn",
                "delegation.spawn.started",
                "2026-08-30T12:05:00Z",
                "exact-delegation",
            ),
            signal(
                "stop",
                Some("root"),
                "child",
                "spawn",
                "delegation.spawn.completed",
                "2026-08-30T12:04:00Z",
                "exact-delegation",
            ),
            signal(
                "failed",
                Some("root"),
                "failed-child",
                "spawn",
                "delegation.spawn.failed",
                "2026-08-30T12:06:00Z",
                "exact-delegation",
            ),
            signal(
                "waiting",
                Some("root"),
                "waiting-child",
                "spawn",
                "delegation.child.waiting",
                "2026-08-30T12:07:00Z",
                "exact-delegation",
            ),
            signal(
                "handoff",
                Some("root"),
                "handoff-child",
                "handoff",
                "delegation.handoff.started",
                "2026-08-30T12:00:00Z",
                "exact-delegation",
            ),
            signal(
                "resume",
                Some("root"),
                "handoff-child",
                "resume",
                "delegation.resume.completed",
                "2026-08-30T12:20:00Z",
                "exact-delegation",
            ),
            signal(
                "join",
                Some("root"),
                "child",
                "join",
                "delegation.join.completed",
                "2026-08-30T12:10:00Z",
                "exact-delegation",
            ),
        ];
        let projection = project_delegation(&signals, now());
        assert_eq!(projection.relations.len(), 6);
        assert_eq!(
            projection
                .relations
                .iter()
                .find(|item| item.child_session_id == "failed-child")
                .map(|item| item.status.as_str()),
            Some("failed")
        );
        assert!(
            projection
                .anomalies
                .iter()
                .any(|item| item.kind == "spawn-failed")
        );
        assert!(
            projection
                .anomalies
                .iter()
                .any(|item| item.kind == "blocked-branch")
        );
        assert_eq!(
            projection
                .relations
                .iter()
                .find(|item| item.relation_type == "handoff")
                .map(|item| item.status.as_str()),
            Some("completed")
        );
        assert!(
            !projection
                .anomalies
                .iter()
                .any(|item| item.kind == "handoff-unfinished")
        );
        assert!(
            !projection
                .anomalies
                .iter()
                .any(|item| item.kind == "unverified-completion"
                    && item
                        .edge_ids
                        .iter()
                        .any(|edge| projection
                            .relations
                            .iter()
                            .any(|relation| relation.id == *edge
                                && relation.child_session_id == "child")))
        );
    }

    #[test]
    fn orphan_stays_inferred_and_partial_sources_never_create_high_confidence_alerts() {
        let projection = project_delegation(
            &[signal(
                "orphan",
                None,
                "child",
                "spawn",
                "delegation.spawn.started",
                "2026-08-30T12:00:00Z",
                "partial-delegation",
            )],
            now(),
        );
        assert_eq!(projection.relations[0].evidence_level, "inferred");
        assert!(projection.relations[0].confidence < 0.6);
        assert!(
            projection
                .anomalies
                .iter()
                .all(|item| item.confidence <= 0.6)
        );
    }

    #[test]
    fn duplicate_signals_do_not_duplicate_relations_and_fanout_requires_six_branches() {
        let mut signals = vec![
            signal(
                "first",
                Some("root"),
                "child",
                "spawn",
                "delegation.spawn.started",
                "2026-08-30T12:00:00Z",
                "exact-delegation",
            ),
            signal(
                "duplicate",
                Some("root"),
                "child",
                "spawn",
                "delegation.spawn.started",
                "2026-08-30T12:00:01Z",
                "exact-delegation",
            ),
            signal(
                "duplicate-again",
                Some("root"),
                "child",
                "spawn",
                "delegation.spawn.started",
                "2026-08-30T12:00:02Z",
                "exact-delegation",
            ),
        ];
        for index in 0..6 {
            signals.push(signal(
                &format!("fanout-{index}"),
                Some("fanout-root"),
                &format!("fanout-child-{index}"),
                "spawn",
                "delegation.spawn.started",
                &format!("2026-08-30T12:01:{index:02}Z"),
                "exact-delegation",
            ));
        }
        let projection = project_delegation(&signals, now());
        assert_eq!(
            projection
                .relations
                .iter()
                .filter(|item| item.child_session_id == "child")
                .count(),
            1
        );
        assert!(
            projection
                .anomalies
                .iter()
                .any(|item| item.kind == "duplicate-delegation")
        );
        assert!(
            projection
                .anomalies
                .iter()
                .any(|item| item.kind == "fanout-spike")
        );
    }

    #[test]
    fn successful_verification_suppresses_unverified_completion() {
        let mut verification = signal(
            "verified",
            None,
            "child",
            "spawn",
            "verification.observed",
            "2026-08-30T12:10:00Z",
            "exact-delegation",
        );
        verification.relation_type = None;
        verification.event_result = Some("succeeded".into());
        let projection = project_delegation(
            &[
                signal(
                    "complete",
                    Some("root"),
                    "child",
                    "spawn",
                    "delegation.spawn.completed",
                    "2026-08-30T12:09:00Z",
                    "exact-delegation",
                ),
                verification,
            ],
            now(),
        );
        assert!(
            !projection
                .anomalies
                .iter()
                .any(|item| item.kind == "unverified-completion")
        );
    }

    #[test]
    fn handoff_and_unverified_completion_rules_are_thresholded_and_evidence_bound() {
        let projection = project_delegation(
            &[
                signal(
                    "exact-handoff",
                    Some("root"),
                    "exact-child",
                    "handoff",
                    "delegation.handoff.started",
                    "2026-08-30T12:00:00Z",
                    "exact-delegation",
                ),
                signal(
                    "partial-handoff",
                    Some("root"),
                    "partial-child",
                    "handoff",
                    "delegation.handoff.started",
                    "2026-08-30T12:00:00Z",
                    "partial-delegation",
                ),
                signal(
                    "fresh-handoff",
                    Some("root"),
                    "fresh-child",
                    "handoff",
                    "delegation.handoff.started",
                    "2026-08-30T12:25:00Z",
                    "exact-delegation",
                ),
                signal(
                    "unverified",
                    Some("root"),
                    "completed-child",
                    "spawn",
                    "delegation.spawn.completed",
                    "2026-08-30T12:10:00Z",
                    "exact-delegation",
                ),
            ],
            now(),
        );
        let unfinished = projection
            .anomalies
            .iter()
            .filter(|item| item.kind == "handoff-unfinished")
            .collect::<Vec<_>>();
        assert_eq!(unfinished.len(), 2);
        assert!(unfinished.iter().any(|item| item.confidence >= 0.9));
        assert!(unfinished.iter().any(|item| item.confidence <= 0.5));
        assert!(
            !unfinished
                .iter()
                .any(|item| item.evidence_ids.contains(&"fresh-handoff".into()))
        );
        assert!(
            projection
                .anomalies
                .iter()
                .any(|item| item.kind == "unverified-completion"
                    && item.evidence_ids == vec!["unverified".to_string()])
        );
        assert!(
            projection
                .anomalies
                .iter()
                .all(|item| !item.evidence_ids.is_empty() && !item.rule_version.is_empty())
        );
    }
}
