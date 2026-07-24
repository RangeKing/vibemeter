use crate::models::{EvidenceReference, ReviewContent, ReviewFinding};
use crate::review_localization;

#[derive(Debug, Clone, Default)]
pub struct ReviewEvidence {
    pub target_id: String,
    pub title: String,
    pub project_label: String,
    pub session_count: u64,
    pub task_count: u64,
    pub total_tokens: u64,
    pub active_seconds: u64,
    pub files_changed: u64,
    pub lines_added: u64,
    pub lines_deleted: u64,
    pub errors: u64,
    pub retries: u64,
    pub verification_events: u64,
    pub has_commit: bool,
    pub max_file_path: Option<String>,
    pub max_modification_count: u64,
    pub model_switches: u64,
    pub comparable_tasks: u64,
    pub personal_high_token_threshold: Option<u64>,
    pub objectives: Vec<String>,
    pub result_excerpts: Vec<String>,
    pub commit_subjects: Vec<String>,
    pub top_tools: Vec<(String, u64)>,
}

pub fn generate(
    locale: &str,
    review_type: &str,
    evidence: &ReviewEvidence,
) -> (String, ReviewContent, Vec<ReviewFinding>) {
    let mut findings = Vec::new();
    let high_token_threshold = evidence
        .personal_high_token_threshold
        .filter(|_| evidence.comparable_tasks >= 20)
        .unwrap_or(150_000);
    let low_output = evidence.files_changed == 0
        || evidence.lines_added.saturating_add(evidence.lines_deleted) < 20;

    if evidence.has_commit || evidence.verification_events > 0 {
        findings.push(finding(locale, "verified-result", "fact", evidence));
    }
    if evidence.total_tokens >= high_token_threshold && low_output {
        findings.push(finding(
            locale,
            "high-token-low-output",
            "inference",
            evidence,
        ));
        findings.push(finding(
            locale,
            "high-token-low-output-action",
            "suggestion",
            evidence,
        ));
    }
    if evidence.max_modification_count >= 5 {
        findings.push(finding(locale, "repeated-file-edits", "fact", evidence));
        findings.push(finding(
            locale,
            "repeated-file-edits-action",
            "suggestion",
            evidence,
        ));
    }
    if evidence.errors >= 3 || evidence.retries >= 2 {
        findings.push(finding(locale, "repeated-errors", "fact", evidence));
        findings.push(finding(
            locale,
            "repeated-errors-action",
            "suggestion",
            evidence,
        ));
    }
    if evidence.active_seconds >= 30 * 60
        && evidence.files_changed > 0
        && evidence.verification_events == 0
        && !evidence.has_commit
    {
        findings.push(finding(
            locale,
            "long-without-verification",
            "inference",
            evidence,
        ));
        findings.push(finding(
            locale,
            "long-without-verification-action",
            "suggestion",
            evidence,
        ));
    }
    if evidence.model_switches >= 2 {
        findings.push(finding(locale, "model-switches", "fact", evidence));
    }
    findings.truncate(8);

    let title = review_localization::review_title(locale, review_type, evidence);
    let content = review_localization::review_content(locale, evidence, &findings);
    (title, content, findings)
}

fn finding(locale: &str, rule_id: &str, tier: &str, evidence: &ReviewEvidence) -> ReviewFinding {
    let (title, detail) = review_localization::finding(locale, rule_id, evidence);
    let mut references = vec![EvidenceReference {
        kind: "target".into(),
        id: evidence.target_id.clone(),
        label: evidence.title.clone(),
    }];
    if rule_id.contains("file-edits")
        && let Some(path) = &evidence.max_file_path
    {
        references.push(EvidenceReference {
            kind: "file".into(),
            id: crate::privacy::stable_hash(path),
            label: path.clone(),
        });
    }
    ReviewFinding {
        id: crate::privacy::stable_hash(&format!("{}:{rule_id}:{tier}", evidence.target_id)),
        rule_id: rule_id.into(),
        tier: tier.into(),
        title,
        detail,
        evidence: references,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fires_only_rules_supported_by_evidence() {
        let evidence = ReviewEvidence {
            target_id: "task-1".into(),
            title: "Repair export".into(),
            total_tokens: 210_000,
            files_changed: 1,
            lines_added: 4,
            lines_deleted: 2,
            ..ReviewEvidence::default()
        };
        let (_, _, findings) = generate("en-US", "session", &evidence);
        assert!(
            findings
                .iter()
                .any(|item| item.rule_id == "high-token-low-output")
        );
        assert!(
            !findings
                .iter()
                .any(|item| item.rule_id == "repeated-errors")
        );
    }

    #[test]
    fn does_not_pad_an_evidence_sparse_review() {
        let evidence = ReviewEvidence {
            target_id: "task-2".into(),
            title: "Inspect logs".into(),
            ..ReviewEvidence::default()
        };
        let (_, _, findings) = generate("zh-CN", "session", &evidence);
        assert!(findings.is_empty());
    }
}
