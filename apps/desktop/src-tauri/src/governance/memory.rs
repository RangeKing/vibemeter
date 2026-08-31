use super::evidence::EvidenceLevel;
use std::collections::BTreeMap;

pub const MEMORY_LEDGER_ALGORITHM_VERSION: &str = "memory-ledger-1.0.0";

const MEMORY_OPERATIONS: [&str; 2] = ["read", "write"];

#[derive(Clone, Debug, PartialEq)]
pub struct CanonicalMemorySignal {
    pub canonical_event_id: String,
    pub agent: String,
    pub session_id: String,
    pub source_session_id: String,
    pub work_unit_id: Option<String>,
    pub event_type: String,
    pub occurred_at: Option<String>,
    pub observed_at: String,
    pub evidence_level: String,
    pub source_coverage: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MemoryEvidenceProjection {
    pub canonical_event_id: String,
    pub evidence_role: String,
    pub observed_at: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MemoryAccessProjection {
    pub id: String,
    pub session_id: String,
    pub source_session_id: String,
    pub work_unit_id: Option<String>,
    pub operation: String,
    pub status: String,
    pub occurred_at: Option<String>,
    pub confidence: f64,
    pub evidence_level: String,
    pub source_coverage: String,
    pub algorithm_version: String,
    pub evidence: Vec<MemoryEvidenceProjection>,
}

pub fn valid_memory_operation(value: &str) -> bool {
    MEMORY_OPERATIONS.contains(&value)
}

fn operation_for(event_type: &str) -> Option<&'static str> {
    match event_type {
        "memory.read" => Some("read"),
        "memory.write" => Some("write"),
        _ => None,
    }
}

fn access_id(signal: &CanonicalMemorySignal, operation: &str) -> String {
    format!(
        "memory-access-{}",
        crate::privacy::stable_hash(&format!(
            "{}|{}|{}|{}|{}",
            MEMORY_LEDGER_ALGORITHM_VERSION,
            signal.agent,
            signal.source_session_id,
            operation,
            signal.canonical_event_id,
        ))
    )
}

pub fn project_memory_accesses(signals: &[CanonicalMemorySignal]) -> Vec<MemoryAccessProjection> {
    let mut accesses = BTreeMap::new();
    for signal in signals {
        let Some(operation) = operation_for(&signal.event_type) else {
            continue;
        };
        if signal.canonical_event_id.is_empty()
            || signal.session_id.is_empty()
            || signal.source_session_id.is_empty()
        {
            continue;
        }
        let level = EvidenceLevel::parse(&signal.evidence_level);
        if level == EvidenceLevel::Unavailable {
            continue;
        }
        let confidence = if signal.source_coverage.starts_with("partial-") {
            level.confidence().min(0.72)
        } else {
            level.confidence()
        };
        let id = access_id(signal, operation);
        accesses
            .entry(id.clone())
            .or_insert_with(|| MemoryAccessProjection {
                id,
                session_id: signal.session_id.clone(),
                source_session_id: signal.source_session_id.clone(),
                work_unit_id: signal.work_unit_id.clone(),
                operation: operation.into(),
                status: "recorded".into(),
                occurred_at: signal
                    .occurred_at
                    .clone()
                    .or_else(|| Some(signal.observed_at.clone())),
                confidence,
                evidence_level: level.as_str().into(),
                source_coverage: signal.source_coverage.clone(),
                algorithm_version: MEMORY_LEDGER_ALGORITHM_VERSION.into(),
                evidence: vec![MemoryEvidenceProjection {
                    canonical_event_id: signal.canonical_event_id.clone(),
                    evidence_role: "access".into(),
                    observed_at: signal.observed_at.clone(),
                }],
            });
    }
    let mut projected = accesses.into_values().collect::<Vec<_>>();
    projected.sort_by(|left, right| {
        left.occurred_at
            .cmp(&right.occurred_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    projected
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signal(id: &str, event_type: &str, occurred_at: &str) -> CanonicalMemorySignal {
        CanonicalMemorySignal {
            canonical_event_id: id.into(),
            agent: "codex".into(),
            session_id: "session-parent".into(),
            source_session_id: "source-parent".into(),
            work_unit_id: None,
            event_type: event_type.into(),
            occurred_at: Some(occurred_at.into()),
            observed_at: occurred_at.into(),
            evidence_level: "derived".into(),
            source_coverage: "partial-memory-read".into(),
        }
    }

    #[test]
    fn partial_read_remains_derived_and_never_implies_a_write() {
        let projected = project_memory_accesses(&[
            signal("memory-evidence", "memory.read", "2026-08-31T10:00:00Z"),
            signal("ordinary-tool", "tool.start", "2026-08-31T10:00:01Z"),
        ]);
        assert_eq!(projected.len(), 1);
        assert_eq!(projected[0].operation, "read");
        assert_eq!(projected[0].evidence_level, "derived");
        assert_eq!(projected[0].source_coverage, "partial-memory-read");
        assert_eq!(projected[0].confidence, 0.72);
        assert_eq!(
            projected[0].evidence[0].canonical_event_id,
            "memory-evidence"
        );
    }

    #[test]
    fn identity_is_stable_across_duplicates_and_source_reordering() {
        let first = signal("memory-a", "memory.read", "2026-08-31T10:00:00Z");
        let second = signal("memory-b", "memory.read", "2026-08-31T10:00:01Z");
        let forward = project_memory_accesses(&[first.clone(), second.clone(), first.clone()]);
        let reverse = project_memory_accesses(&[second, first]);
        assert_eq!(forward.len(), 2);
        assert_eq!(forward, reverse);
    }

    #[test]
    fn missing_or_unavailable_evidence_is_not_projected() {
        let mut missing = signal("", "memory.read", "2026-08-31T10:00:00Z");
        let mut unavailable = signal("unavailable-memory", "memory.read", "2026-08-31T10:00:01Z");
        unavailable.evidence_level = "unavailable".into();
        assert!(project_memory_accesses(&[missing.clone(), unavailable]).is_empty());
        missing.canonical_event_id = "write-evidence".into();
        missing.event_type = "memory.write".into();
        assert_eq!(project_memory_accesses(&[missing])[0].operation, "write");
    }
}
