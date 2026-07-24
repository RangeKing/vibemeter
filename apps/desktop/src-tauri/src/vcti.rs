use crate::models::{
    BehaviorSignals, BehaviorSummary, VctiBadge, VctiEvidenceItem, VctiProfile, VctiScore,
    VctiTrendPoint,
};
use chrono::{DateTime, Duration, Local, Timelike, Utc};
use std::collections::{BTreeMap, HashMap, HashSet};

pub const ALGORITHM_VERSION: &str = "1.2.0";
const WINDOW_DAYS: i64 = 90;
const HALF_LIFE_DAYS: f64 = 45.0;

#[derive(Debug, Clone)]
pub struct SessionBehaviorRecord {
    pub id: String,
    pub started_at: String,
    pub agent: String,
    pub model: Option<String>,
    pub active_seconds: u64,
    pub total_tokens: u64,
    pub cache_read_tokens: u64,
    pub tool_calls: u64,
    pub files_touched: u64,
    pub lines_changed: u64,
    pub errors: u64,
    pub verification_events: u64,
    pub human_interventions: u64,
    pub subagent_count: u64,
    pub model_switches: u64,
    pub longest_uninterrupted_seconds: u64,
    pub has_commit: bool,
    pub git_review_events: u64,
    pub test_events: u64,
    pub build_events: u64,
    pub lint_events: u64,
    pub typecheck_events: u64,
    pub read_events: u64,
    pub search_events: u64,
    pub edit_events: u64,
    pub shell_events: u64,
    pub behavior: BehaviorSignals,
}

#[derive(Default, Clone)]
struct Features {
    requirement_clarity: f64,
    exploration: f64,
    scope_drift: f64,
    delegation: f64,
    human_intervention: f64,
    parallel_orchestration: f64,
    diff_review: f64,
    automated_verification: f64,
    rollback_awareness: f64,
    root_cause: f64,
    local_fix: f64,
    automation: f64,
    first_result_speed: f64,
    iteration_granularity: f64,
    shipping_tendency: f64,
    tool_switching: f64,
    cost_routing: f64,
    context_reuse: f64,
    polish: f64,
    infrastructure: f64,
    dependency_reuse: f64,
    burst: f64,
    completion: f64,
    tool_success: f64,
    average_task_seconds: f64,
    prompt_structure_rate: f64,
    acceptance_rate: f64,
    file_scope_rate: f64,
    night_rate: f64,
    long_session_count: u64,
    active_day_variation: f64,
}

#[derive(Clone)]
struct TypeScore {
    code: &'static str,
    guild: &'static str,
    score: f64,
    eligible: bool,
}

pub fn calculate(
    records: &[SessionBehaviorRecord],
    behavior: BehaviorSummary,
    available_agents: u64,
    structure_analysis_enabled: bool,
    git_evidence_enabled: bool,
    now: DateTime<Utc>,
) -> VctiProfile {
    let period_end = now.date_naive();
    let period_start = (now - Duration::days(WINDOW_DAYS - 1)).date_naive();
    let active_days = records
        .iter()
        .filter_map(|record| record.started_at.get(..10))
        .collect::<HashSet<_>>()
        .len() as u64;
    let status = if records.len() >= 80 && active_days >= 21 {
        "high-confidence"
    } else if records.len() >= 30 && active_days >= 7 {
        "stable"
    } else if records.len() >= 8 && active_days >= 2 {
        "preview"
    } else {
        "collecting"
    };
    let features = derive_features(records, available_agents, now);
    let mut candidates = type_scores(
        &features,
        structure_analysis_enabled,
        git_evidence_enabled,
        behavior.orchestration_coverage,
        behavior.process_control_coverage,
        available_agents,
    );
    candidates.retain(|candidate| candidate.eligible);
    candidates.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.code.cmp(right.code))
    });
    let top = candidates
        .first()
        .filter(|candidate| status != "collecting" && candidate.score >= 42.0);
    let secondary = top.and_then(|primary| {
        candidates
            .iter()
            .skip(1)
            .find(|candidate| {
                candidate.guild != primary.guild
                    && candidate.score >= 40.0
                    && primary.score - candidate.score <= 18.0
            })
            .or_else(|| {
                candidates
                    .get(1)
                    .filter(|candidate| primary.score - candidate.score <= 10.0)
            })
    });
    let top_score = top.map_or(0.0, |candidate| candidate.score);
    let second_score = candidates.get(1).map_or(0.0, |candidate| candidate.score);
    let type_margin = ((top_score - second_score) / 100.0).clamp(0.0, 1.0);
    let coverage = average(&[
        behavior.structure_coverage,
        behavior.lifecycle_coverage,
        behavior.tool_result_coverage,
        behavior.orchestration_coverage,
        behavior.process_control_coverage,
    ]);
    let temporal_stability = temporal_stability(records);
    let confidence = separation_capped_confidence(
        (cap(records.len() as f64, 80.0) * 0.25
            + cap(active_days as f64, 21.0) * 0.20
            + coverage * 100.0 * 0.20
            + cap(type_margin, 0.12) * 0.20
            + temporal_stability * 100.0 * 0.15)
            .clamp(0.0, 100.0),
        type_margin,
    );
    let confidence_label = if confidence >= 80.0 {
        "high"
    } else if confidence >= 60.0 {
        "clear"
    } else if confidence >= 40.0 {
        "preview"
    } else {
        "collecting"
    };
    let scores = public_scores(&features, &behavior);
    let dimensions = dimension_scores(&features);
    let badges = badges(
        &features,
        &behavior,
        records,
        git_evidence_enabled,
        active_days,
    );
    let evidence = top
        .map(|candidate| evidence_for(candidate.code, &features, &behavior, records))
        .unwrap_or_default();
    let trend = build_trend(
        records,
        available_agents,
        structure_analysis_enabled,
        git_evidence_enabled,
        &behavior,
        now,
    );
    let mut missing_capabilities = Vec::new();
    if !structure_analysis_enabled {
        missing_capabilities.push("prompt-structure".into());
    }
    if !git_evidence_enabled {
        missing_capabilities.push("git-evidence".into());
    }
    if behavior.lifecycle_coverage < 0.5 {
        missing_capabilities.push("task-lifecycle".into());
    }
    if behavior.tool_result_coverage < 0.5 {
        missing_capabilities.push("tool-results".into());
    }
    if behavior.orchestration_coverage < 0.5 {
        missing_capabilities.push("agent-orchestration".into());
    }

    VctiProfile {
        status: status.into(),
        algorithm_version: ALGORITHM_VERSION.into(),
        period_start: period_start.to_string(),
        period_end: period_end.to_string(),
        session_count: records.len() as u64,
        active_days,
        primary_type: top.map(|candidate| candidate.code.into()),
        secondary_type: secondary.map(|candidate| candidate.code.into()),
        guild: top.map(|candidate| candidate.guild.into()),
        confidence,
        confidence_label: confidence_label.into(),
        type_margin,
        scores,
        dimensions,
        badges,
        evidence,
        trend,
        behavior,
        missing_capabilities,
        structure_analysis_enabled,
        git_evidence_enabled,
    }
}

fn derive_features(
    records: &[SessionBehaviorRecord],
    available_agents: u64,
    now: DateTime<Utc>,
) -> Features {
    if records.is_empty() {
        return Features::default();
    }
    let mut weighted_sessions = 0.0;
    let mut modified_sessions = 0.0;
    let mut verified_modified = 0.0;
    let mut committed_sessions = 0.0;
    let mut prompt_count = 0.0;
    let mut prompt_characters = 0.0;
    let mut structured_prompts = 0.0;
    let mut acceptance_prompts = 0.0;
    let mut scope_prompts = 0.0;
    let mut files = 0.0;
    let mut lines = 0.0;
    let mut tools = 0.0;
    let mut interventions = 0.0;
    let mut subagents = 0.0;
    let mut parallel_batches = 0.0;
    let mut plans = 0.0;
    let mut goal_changes = 0.0;
    let mut compactions = 0.0;
    let mut rollbacks = 0.0;
    let mut git_reviews = 0.0;
    let mut tests = 0.0;
    let mut reads_and_searches = 0.0;
    let mut edits = 0.0;
    let mut shell_events = 0.0;
    let mut errors = 0.0;
    let mut automation_events = 0.0;
    let mut infra_events = 0.0;
    let mut dependency_events = 0.0;
    let mut style_events = 0.0;
    let mut document_events = 0.0;
    let mut instruction_events = 0.0;
    let mut deploy_events = 0.0;
    let mut completions = 0.0;
    let mut aborts = 0.0;
    let mut success_tools = 0.0;
    let mut failed_tools = 0.0;
    let mut task_duration_ms = 0.0;
    let mut first_tool_ms = 0.0;
    let mut first_tool_weight = 0.0;
    let mut total_tokens = 0.0;
    let mut cache_tokens = 0.0;
    let mut model_switches = 0.0;
    let mut longest_seconds: f64 = 0.0;
    let mut providers = HashSet::new();
    let mut models = HashSet::new();
    let mut daily = BTreeMap::<String, f64>::new();
    let mut night_days = HashSet::new();
    let mut active_days = HashSet::new();
    let mut long_session_count = 0_u64;

    for record in records {
        debug_assert!(!record.id.is_empty());
        let parsed = DateTime::parse_from_rfc3339(&record.started_at).ok();
        let age_days = parsed
            .as_ref()
            .map(|value| {
                now.signed_duration_since(value.with_timezone(&Utc))
                    .num_seconds()
                    .max(0) as f64
                    / 86_400.0
            })
            .unwrap_or(WINDOW_DAYS as f64);
        let weight = (-std::f64::consts::LN_2 * age_days / HALF_LIFE_DAYS).exp();
        weighted_sessions += weight;
        providers.insert(record.agent.clone());
        if let Some(model) = &record.model {
            models.insert(model.clone());
        }
        if let Some(day) = record.started_at.get(..10) {
            active_days.insert(day.to_string());
            *daily.entry(day.to_string()).or_default() += 1.0;
        }
        if parsed
            .as_ref()
            .map(|value| value.with_timezone(&Local).hour())
            .is_some_and(|hour| hour < 5)
            && let Some(day) = record.started_at.get(..10)
        {
            night_days.insert(day.to_string());
        }
        if record.longest_uninterrupted_seconds >= 5_400 {
            long_session_count = long_session_count.saturating_add(1);
        }
        if record.files_touched > 0 || record.lines_changed > 0 {
            modified_sessions += weight;
            if record.verification_events > 0 || record.has_commit {
                verified_modified += weight;
            }
        }
        if record.has_commit {
            committed_sessions += weight;
        }
        files += record.files_touched as f64 * weight;
        lines += record.lines_changed as f64 * weight;
        tools += record.tool_calls as f64 * weight;
        interventions += record.human_interventions as f64 * weight;
        subagents += record.behavior.subagent_starts.max(record.subagent_count) as f64 * weight;
        parallel_batches += record.behavior.parallel_batches as f64 * weight;
        plans += record.behavior.plan_events as f64 * weight;
        goal_changes += record.behavior.goal_changes as f64 * weight;
        compactions += record.behavior.context_compactions as f64 * weight;
        rollbacks += record.behavior.rollbacks as f64 * weight;
        git_reviews += record.git_review_events as f64 * weight;
        tests += (record.test_events
            + record.build_events
            + record.lint_events
            + record.typecheck_events) as f64
            * weight;
        reads_and_searches += (record.read_events + record.search_events) as f64 * weight;
        edits += record.edit_events as f64 * weight;
        shell_events += record.shell_events as f64 * weight;
        errors += record.errors as f64 * weight;
        automation_events += record.behavior.automation_events as f64 * weight;
        infra_events += record.behavior.infrastructure_events as f64 * weight;
        dependency_events += record.behavior.dependency_events as f64 * weight;
        style_events += record.behavior.style_events as f64 * weight;
        document_events += record.behavior.document_events as f64 * weight;
        instruction_events += record.behavior.instruction_file_events as f64 * weight;
        deploy_events += record.behavior.deploy_events as f64 * weight;
        completions += record.behavior.task_completions as f64 * weight;
        aborts += record.behavior.task_aborts as f64 * weight;
        success_tools += record.behavior.successful_tools as f64 * weight;
        failed_tools += record.behavior.failed_tools as f64 * weight;
        task_duration_ms += record.behavior.completed_task_duration_ms as f64 * weight;
        if let Some(value) = record.behavior.time_to_first_tool_ms {
            first_tool_ms += value as f64 * weight;
            first_tool_weight += weight;
        }
        prompt_count += record.behavior.prompt_count as f64 * weight;
        prompt_characters += record.behavior.prompt_characters as f64 * weight;
        structured_prompts += record.behavior.structured_prompts as f64 * weight;
        acceptance_prompts += record.behavior.acceptance_criteria_prompts as f64 * weight;
        scope_prompts += record.behavior.file_scope_prompts as f64 * weight;
        total_tokens += record.total_tokens as f64 * weight;
        cache_tokens += record.cache_read_tokens as f64 * weight;
        model_switches += record.model_switches as f64 * weight;
        longest_seconds = longest_seconds.max(
            record
                .longest_uninterrupted_seconds
                .max(record.active_seconds) as f64,
        );
    }

    let ws = weighted_sessions.max(0.001);
    let prompt_structure_rate = ratio(structured_prompts, prompt_count);
    let acceptance_rate = ratio(acceptance_prompts, prompt_count);
    let file_scope_rate = ratio(scope_prompts, prompt_count);
    let plan_rate = ratio(plans, ws);
    let avg_prompt_chars = ratio(prompt_characters, prompt_count);
    let scope_drift = cap(ratio(goal_changes, ws), 0.28);
    let requirement_clarity = (prompt_structure_rate * 38.0
        + cap(acceptance_rate, 0.28) * 26.0
        + cap(file_scope_rate, 0.38) * 22.0
        + cap(plan_rate, 0.45) * 14.0)
        .clamp(0.0, 100.0);
    let exploration = (scope_drift * 0.38
        + cap(ratio(model_switches, ws), 0.20) * 0.22
        + cap(ratio(dependency_events, ws), 0.40) * 0.20
        + inverse(cap(avg_prompt_chars, 1_200.0)) * 0.20)
        .clamp(0.0, 100.0);
    let delegation = (cap(ratio(files, ws), 8.0) * 0.35
        + cap(ratio(tools, prompt_count.max(ws)), 18.0) * 0.30
        + cap(longest_seconds, 7_200.0) * 0.20
        + inverse(cap(ratio(interventions, ws), 5.0)) * 0.15)
        .clamp(0.0, 100.0);
    let human_intervention = cap(ratio(interventions, ws), 4.0);
    let parallel_orchestration = (cap(ratio(subagents, ws), 0.65) * 0.65
        + cap(ratio(parallel_batches, ws), 0.45) * 0.35)
        .clamp(0.0, 100.0);
    let diff_review = cap(ratio(git_reviews, modified_sessions.max(1.0)), 1.15);
    let automated_verification = (ratio(verified_modified, modified_sessions.max(1.0)) * 70.0
        + cap(ratio(tests, modified_sessions.max(1.0)), 1.2) * 30.0)
        .clamp(0.0, 100.0);
    let rollback_awareness = (cap(ratio(rollbacks, modified_sessions.max(1.0)), 0.16) * 0.58
        + ratio(committed_sessions, modified_sessions.max(1.0)) * 42.0)
        .clamp(0.0, 100.0);
    let root_cause = (cap(
        ratio(reads_and_searches + shell_events * 0.08, edits.max(1.0)),
        0.95,
    ) * 0.45
        + cap(ratio(tests, modified_sessions.max(1.0)), 1.0) * 0.30
        + cap(ratio(errors, ws), 0.35) * 0.25)
        .clamp(0.0, 100.0);
    let local_fix = (inverse(cap(ratio(files, modified_sessions.max(1.0)), 9.0)) * 0.58
        + inverse(cap(ratio(lines, modified_sessions.max(1.0)), 480.0)) * 0.42)
        .clamp(0.0, 100.0);
    let automation = (cap(ratio(automation_events, ws), 0.30) * 0.58
        + cap(ratio(tools, ws), 55.0) * 0.24
        + cap(plan_rate, 0.60) * 0.18)
        .clamp(0.0, 100.0);
    let average_first_tool_ms = ratio(first_tool_ms, first_tool_weight.max(0.001));
    let first_result_speed = inverse(cap(average_first_tool_ms, 12.0 * 60_000.0));
    let iteration_granularity = (cap(ratio(files, modified_sessions.max(1.0)), 10.0) * 0.58
        + cap(ratio(lines, modified_sessions.max(1.0)), 600.0) * 0.42)
        .clamp(0.0, 100.0);
    let completion = ratio(completions, completions + aborts);
    let shipping_tendency = (cap(ratio(deploy_events, ws), 0.08) * 0.34
        + ratio(committed_sessions, ws) * 34.0
        + completion * 32.0)
        .clamp(0.0, 100.0);
    let provider_signal = if available_agents >= 2 {
        cap(providers.len().saturating_sub(1) as f64, 2.0)
    } else {
        0.0
    };
    let tool_switching = (provider_signal * 0.42
        + cap(models.len().saturating_sub(1) as f64, 5.0) * 0.34
        + cap(ratio(model_switches, ws), 0.18) * 0.24)
        .clamp(0.0, 100.0);
    let cache_ratio = ratio(cache_tokens, total_tokens);
    let cost_routing = (cap(cache_ratio, 0.72) * 0.62
        + cap(ratio(model_switches, ws), 0.12) * 0.22
        + cap(models.len().saturating_sub(1) as f64, 3.0) * 0.16)
        .clamp(0.0, 100.0);
    let context_reuse = (cap(ratio(instruction_events + document_events, ws), 0.24) * 0.42
        + cap(ratio(compactions, ws), 0.18) * 0.22
        + cap(cache_ratio, 0.72) * 0.36)
        .clamp(0.0, 100.0);
    let polish = (cap(ratio(style_events, modified_sessions.max(1.0)), 1.8) * 0.48
        + cap(ratio(document_events, modified_sessions.max(1.0)), 0.65) * 0.20
        + inverse(first_result_speed) * 0.12
        + inverse(local_fix) * 0.20)
        .clamp(0.0, 100.0);
    let infrastructure = cap(ratio(infra_events, modified_sessions.max(1.0)), 0.85);
    let dependency_reuse = cap(ratio(dependency_events, modified_sessions.max(1.0)), 0.55);
    let mean_day = if daily.is_empty() {
        0.0
    } else {
        daily.values().sum::<f64>() / daily.len() as f64
    };
    let max_day = daily.values().copied().fold(0.0, f64::max);
    let burst = (cap(ratio(max_day, mean_day.max(1.0)) - 1.0, 2.0) * 0.62
        + cap(long_session_count as f64, 8.0) * 0.38)
        .clamp(0.0, 100.0);
    let variance = if daily.len() > 1 && mean_day > 0.0 {
        (daily
            .values()
            .map(|value| (value - mean_day).powi(2))
            .sum::<f64>()
            / daily.len() as f64)
            .sqrt()
            / mean_day
    } else {
        1.0
    };

    Features {
        requirement_clarity,
        exploration,
        scope_drift,
        delegation,
        human_intervention,
        parallel_orchestration,
        diff_review,
        automated_verification,
        rollback_awareness,
        root_cause,
        local_fix,
        automation,
        first_result_speed,
        iteration_granularity,
        shipping_tendency,
        tool_switching,
        cost_routing,
        context_reuse,
        polish,
        infrastructure,
        dependency_reuse,
        burst,
        completion: completion * 100.0,
        tool_success: ratio(success_tools, success_tools + failed_tools) * 100.0,
        average_task_seconds: ratio(task_duration_ms, completions.max(1.0)) / 1_000.0,
        prompt_structure_rate: prompt_structure_rate * 100.0,
        acceptance_rate: acceptance_rate * 100.0,
        file_scope_rate: file_scope_rate * 100.0,
        night_rate: ratio(night_days.len() as f64, active_days.len().max(1) as f64) * 100.0,
        long_session_count,
        active_day_variation: variance,
    }
}

fn type_scores(
    features: &Features,
    structure_enabled: bool,
    git_enabled: bool,
    orchestration_coverage: f64,
    process_coverage: f64,
    available_agents: u64,
) -> Vec<TypeScore> {
    let guardrail = average(&[
        features.diff_review,
        features.automated_verification,
        features.rollback_awareness,
    ]);
    let types = vec![
        (
            "VIBE",
            "start",
            weighted(&[
                (features.exploration, 0.40),
                (inverse(features.requirement_clarity), 0.34),
                (features.polish, 0.26),
            ]),
            structure_enabled,
        ),
        (
            "SPEC",
            "start",
            weighted(&[
                (features.requirement_clarity, 0.58),
                (inverse(features.scope_drift), 0.24),
                (features.context_reuse, 0.18),
            ]),
            structure_enabled,
        ),
        (
            "HACK",
            "start",
            weighted(&[
                (features.exploration, 0.40),
                (features.automation, 0.27),
                (features.dependency_reuse, 0.20),
                (inverse(features.requirement_clarity), 0.13),
            ]),
            structure_enabled,
        ),
        (
            "MIX",
            "start",
            weighted(&[
                (features.dependency_reuse, 0.55),
                (features.iteration_granularity, 0.18),
                (features.first_result_speed, 0.17),
                (inverse(features.infrastructure), 0.10),
            ]),
            structure_enabled,
        ),
        (
            "YOLO",
            "agent",
            weighted(&[
                (features.delegation, 0.46),
                (features.iteration_granularity, 0.27),
                (inverse(guardrail), 0.27),
            ]),
            true,
        ),
        (
            "LOOP",
            "agent",
            weighted(&[
                (features.human_intervention, 0.50),
                (inverse(features.iteration_granularity), 0.27),
                (features.polish, 0.23),
            ]),
            true,
        ),
        (
            "BOSS",
            "agent",
            weighted(&[
                (features.parallel_orchestration, 0.42),
                (features.requirement_clarity, 0.34),
                (features.context_reuse, 0.24),
            ]),
            orchestration_coverage >= 0.30,
        ),
        (
            "SWARM",
            "agent",
            weighted(&[
                (features.parallel_orchestration, 0.68),
                (features.tool_switching, 0.18),
                (features.delegation, 0.14),
            ]),
            orchestration_coverage >= 0.30,
        ),
        (
            "DIFF",
            "quality",
            weighted(&[
                (features.diff_review, 0.66),
                (features.human_intervention, 0.18),
                (features.rollback_awareness, 0.16),
            ]),
            true,
        ),
        (
            "TEST",
            "quality",
            weighted(&[
                (features.automated_verification, 0.72),
                (features.root_cause, 0.16),
                (features.rollback_awareness, 0.12),
            ]),
            true,
        ),
        (
            "DOCS",
            "quality",
            weighted(&[
                (features.context_reuse, 0.50),
                (features.requirement_clarity, 0.30),
                (inverse(features.scope_drift), 0.20),
            ]),
            true,
        ),
        (
            "UNDO",
            "quality",
            weighted(&[
                (features.rollback_awareness, 0.68),
                (features.iteration_granularity, 0.18),
                (features.exploration, 0.14),
            ]),
            git_enabled || process_coverage >= 0.30,
        ),
        (
            "DEBUG",
            "debug",
            weighted(&[
                (features.root_cause, 0.62),
                (features.automated_verification, 0.20),
                (inverse(features.first_result_speed), 0.18),
            ]),
            true,
        ),
        (
            "PATCH",
            "debug",
            weighted(&[
                (features.local_fix, 0.58),
                (features.first_result_speed, 0.28),
                (inverse(features.infrastructure), 0.14),
            ]),
            true,
        ),
        (
            "STACK",
            "debug",
            weighted(&[
                (features.infrastructure, 0.56),
                (features.dependency_reuse, 0.24),
                (features.iteration_granularity, 0.20),
            ]),
            true,
        ),
        (
            "AUTO",
            "debug",
            weighted(&[
                (features.automation, 0.64),
                (features.parallel_orchestration, 0.18),
                (features.context_reuse, 0.18),
            ]),
            true,
        ),
        (
            "SHIP",
            "delivery",
            weighted(&[
                (features.shipping_tendency, 0.55),
                (features.first_result_speed, 0.26),
                (features.completion, 0.19),
            ]),
            true,
        ),
        (
            "RUSH",
            "delivery",
            weighted(&[
                (features.burst, 0.58),
                (features.completion, 0.24),
                (features.first_result_speed, 0.18),
            ]),
            true,
        ),
        (
            "MVP",
            "delivery",
            weighted(&[
                (features.first_result_speed, 0.34),
                (features.local_fix, 0.28),
                (inverse(features.polish), 0.22),
                (features.shipping_tendency, 0.16),
            ]),
            structure_enabled,
        ),
        (
            "DETAIL",
            "delivery",
            weighted(&[
                (features.polish, 0.62),
                (inverse(features.first_result_speed), 0.18),
                (features.human_intervention, 0.20),
            ]),
            true,
        ),
        (
            "FORK",
            "tools",
            weighted(&[
                (features.tool_switching, 0.74),
                (features.exploration, 0.16),
                (features.cost_routing, 0.10),
            ]),
            available_agents >= 2,
        ),
        (
            "TOKEN",
            "tools",
            weighted(&[
                (features.cost_routing, 0.72),
                (features.context_reuse, 0.18),
                (inverse(features.iteration_granularity), 0.10),
            ]),
            true,
        ),
        (
            "CACHE",
            "tools",
            weighted(&[
                (features.context_reuse, 0.68),
                (features.requirement_clarity, 0.18),
                (features.cost_routing, 0.14),
            ]),
            true,
        ),
        (
            "BUDDY",
            "tools",
            weighted(&[
                (inverse(features.tool_switching), 0.62),
                (features.context_reuse, 0.22),
                (inverse(features.scope_drift), 0.16),
            ]),
            available_agents >= 2,
        ),
    ];
    types
        .into_iter()
        .map(|(code, guild, score, eligible)| TypeScore {
            code,
            guild,
            score,
            eligible,
        })
        .collect()
}

fn public_scores(features: &Features, behavior: &BehaviorSummary) -> Vec<VctiScore> {
    vec![
        score(
            "startStructure",
            features.requirement_clarity,
            behavior.structure_coverage,
        ),
        score(
            "delegation",
            features.delegation,
            behavior.orchestration_coverage.max(0.55),
        ),
        score(
            "guardrail",
            average(&[
                features.diff_review,
                features.automated_verification,
                features.rollback_awareness,
            ]),
            average(&[
                behavior.tool_result_coverage,
                behavior.process_control_coverage,
            ]),
        ),
        score(
            "debugDepth",
            features.root_cause,
            behavior.tool_result_coverage,
        ),
        score(
            "shipping",
            features.shipping_tendency,
            behavior.lifecycle_coverage,
        ),
        score("toolNomad", features.tool_switching, 1.0),
    ]
}

fn dimension_scores(features: &Features) -> Vec<VctiScore> {
    [
        ("requirementClarity", features.requirement_clarity),
        ("exploration", features.exploration),
        ("scopeDrift", features.scope_drift),
        ("delegation", features.delegation),
        ("humanIntervention", features.human_intervention),
        ("parallelOrchestration", features.parallel_orchestration),
        ("diffReview", features.diff_review),
        ("automatedVerification", features.automated_verification),
        ("rollbackAwareness", features.rollback_awareness),
        ("rootCause", features.root_cause),
        ("localFix", features.local_fix),
        ("automation", features.automation),
        ("firstResultSpeed", features.first_result_speed),
        ("iterationGranularity", features.iteration_granularity),
        ("shippingTendency", features.shipping_tendency),
        ("toolSwitching", features.tool_switching),
        ("costRouting", features.cost_routing),
        ("contextReuse", features.context_reuse),
    ]
    .into_iter()
    .map(|(id, value)| score(id, value, 1.0))
    .collect()
}

fn badges(
    features: &Features,
    behavior: &BehaviorSummary,
    records: &[SessionBehaviorRecord],
    git_enabled: bool,
    active_days: u64,
) -> Vec<VctiBadge> {
    let mut candidates = Vec::<(&str, f64)>::new();
    let guardrail = average(&[
        features.diff_review,
        features.automated_verification,
        features.rollback_awareness,
    ]);
    if guardrail >= 70.0
        && features.automated_verification >= 65.0
        && (git_enabled || behavior.process_control_coverage >= 0.6)
    {
        candidates.push(("GUARD", guardrail));
    }
    if features.first_result_speed >= 72.0
        && features.tool_success >= 78.0
        && features.completion >= 65.0
    {
        candidates.push(("TURBO", features.first_result_speed));
    }
    if features.cost_routing >= 72.0 && records.len() >= 30 {
        candidates.push(("BUDGET", features.cost_routing));
    }
    if features.human_intervention >= 55.0 && features.tool_success >= 72.0 && records.len() >= 20 {
        candidates.push((
            "LIVE",
            average(&[features.human_intervention, features.tool_success]),
        ));
    }
    if features.night_rate >= 35.0 && active_days >= 7 {
        candidates.push(("NIGHT", features.night_rate));
    }
    if features.long_session_count >= 8 {
        candidates.push(("MARATHON", cap(features.long_session_count as f64, 14.0)));
    }
    if behavior.orchestration_coverage >= 0.6
        && features.parallel_orchestration <= 8.0
        && records.len() >= 30
    {
        candidates.push(("SOLO", inverse(features.parallel_orchestration)));
    }
    if features.completion >= 80.0
        && features.automated_verification >= 55.0
        && behavior.lifecycle_coverage >= 0.6
    {
        candidates.push((
            "FINISH",
            average(&[features.completion, features.automated_verification]),
        ));
    }
    if active_days >= 42 && features.active_day_variation <= 0.55 {
        candidates.push(("STEADY", inverse(cap(features.active_day_variation, 1.2))));
    }
    candidates.sort_by(|left, right| {
        right
            .1
            .partial_cmp(&left.1)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    candidates
        .into_iter()
        .take(2)
        .map(|(code, _)| VctiBadge {
            code: code.into(),
            label_key: format!("vcti.badges.{code}.name"),
            description_key: format!("vcti.badges.{code}.description"),
        })
        .collect()
}

fn evidence_for(
    code: &str,
    features: &Features,
    behavior: &BehaviorSummary,
    records: &[SessionBehaviorRecord],
) -> Vec<VctiEvidenceItem> {
    let mut catalog = HashMap::<&str, VctiEvidenceItem>::new();
    catalog.insert(
        "structure",
        evidence(
            "structure",
            "vcti.evidence.structure",
            features.prompt_structure_rate,
            "percent",
            true,
        ),
    );
    catalog.insert(
        "acceptance",
        evidence(
            "acceptance",
            "vcti.evidence.acceptance",
            features.acceptance_rate,
            "percent",
            true,
        ),
    );
    catalog.insert(
        "scope",
        evidence(
            "scope",
            "vcti.evidence.fileScope",
            features.file_scope_rate,
            "percent",
            true,
        ),
    );
    catalog.insert(
        "verification",
        evidence(
            "verification",
            "vcti.evidence.verification",
            features.automated_verification,
            "percent",
            false,
        ),
    );
    catalog.insert(
        "diff",
        evidence(
            "diff",
            "vcti.evidence.diffReview",
            features.diff_review,
            "percent",
            false,
        ),
    );
    catalog.insert(
        "completion",
        evidence(
            "completion",
            "vcti.evidence.completion",
            features.completion,
            "percent",
            false,
        ),
    );
    catalog.insert(
        "subagents",
        evidence(
            "subagents",
            "vcti.evidence.subagents",
            behavior.subagent_starts as f64,
            "number",
            false,
        ),
    );
    catalog.insert(
        "rollbacks",
        evidence(
            "rollbacks",
            "vcti.evidence.rollbacks",
            behavior.rollbacks as f64,
            "number",
            false,
        ),
    );
    catalog.insert(
        "plans",
        evidence(
            "plans",
            "vcti.evidence.plans",
            behavior.plan_events as f64,
            "number",
            false,
        ),
    );
    catalog.insert(
        "duration",
        evidence(
            "duration",
            "vcti.evidence.averageTaskDuration",
            features.average_task_seconds,
            "duration",
            false,
        ),
    );
    catalog.insert(
        "sessions",
        evidence(
            "sessions",
            "vcti.evidence.sessions",
            records.len() as f64,
            "number",
            false,
        ),
    );
    catalog.insert(
        "automation",
        evidence(
            "automation",
            "vcti.evidence.automation",
            behavior.automation_events as f64,
            "number",
            false,
        ),
    );
    catalog.insert(
        "style",
        evidence(
            "style",
            "vcti.evidence.styleEdits",
            behavior.style_events as f64,
            "number",
            false,
        ),
    );
    catalog.insert(
        "context",
        evidence(
            "context",
            "vcti.evidence.contextEvents",
            (behavior.context_compactions + behavior.document_events) as f64,
            "number",
            false,
        ),
    );
    let keys: &[&str] = match code {
        "VIBE" => &["structure", "style", "scope", "sessions"],
        "SPEC" => &["structure", "acceptance", "scope", "plans"],
        "HACK" => &["automation", "scope", "duration", "sessions"],
        "MIX" => &["automation", "style", "duration", "sessions"],
        "YOLO" => &["verification", "diff", "completion", "sessions"],
        "LOOP" => &["duration", "style", "completion", "sessions"],
        "BOSS" | "SWARM" => &["subagents", "plans", "completion", "sessions"],
        "DIFF" => &["diff", "verification", "rollbacks", "sessions"],
        "TEST" => &["verification", "completion", "diff", "sessions"],
        "DOCS" | "CACHE" => &["context", "structure", "plans", "sessions"],
        "UNDO" => &["rollbacks", "diff", "verification", "sessions"],
        "DEBUG" => &["verification", "duration", "completion", "sessions"],
        "PATCH" | "MVP" => &["duration", "completion", "verification", "sessions"],
        "STACK" | "AUTO" => &["automation", "plans", "duration", "sessions"],
        "SHIP" | "RUSH" => &["completion", "duration", "verification", "sessions"],
        "DETAIL" => &["style", "duration", "verification", "sessions"],
        "FORK" | "TOKEN" | "BUDDY" => &["sessions", "context", "completion", "duration"],
        _ => &["sessions", "completion", "verification"],
    };
    let mut selected = keys
        .iter()
        .filter_map(|key| catalog.get(key).cloned())
        .take(4)
        .collect::<Vec<_>>();
    if behavior.structure_coverage >= 0.5 {
        for key in ["structure", "acceptance"] {
            if selected.iter().any(|item| item.id == key) {
                continue;
            }
            if let Some(item) = catalog.get(key) {
                selected.push(item.clone());
            }
        }
    }
    selected
}

fn build_trend(
    records: &[SessionBehaviorRecord],
    available_agents: u64,
    structure_enabled: bool,
    git_enabled: bool,
    behavior: &BehaviorSummary,
    now: DateTime<Utc>,
) -> Vec<VctiTrendPoint> {
    let mut points = Vec::new();
    for offset in (0..6).rev() {
        let end = now - Duration::days(offset * 14);
        let start = end - Duration::days(13);
        let slice = records
            .iter()
            .filter(|record| {
                DateTime::parse_from_rfc3339(&record.started_at)
                    .map(|value| {
                        let value = value.with_timezone(&Utc);
                        value >= start && value <= end
                    })
                    .unwrap_or(false)
            })
            .cloned()
            .collect::<Vec<_>>();
        if slice.len() < 2 {
            continue;
        }
        let features = derive_features(&slice, available_agents, end);
        let mut types = type_scores(
            &features,
            structure_enabled,
            git_enabled,
            behavior.orchestration_coverage,
            behavior.process_control_coverage,
            available_agents,
        );
        types.retain(|item| item.eligible);
        types.sort_by(|left, right| {
            right
                .score
                .partial_cmp(&left.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        points.push(VctiTrendPoint {
            period_start: start.date_naive().to_string(),
            scores: public_scores(&features, behavior),
            dominant_type: types.first().map(|item| item.code.into()),
        });
    }
    points
}

fn temporal_stability(records: &[SessionBehaviorRecord]) -> f64 {
    if records.len() < 8 {
        return 0.25;
    }
    let mut weeks = HashMap::<String, u64>::new();
    for record in records {
        let week = DateTime::parse_from_rfc3339(&record.started_at)
            .map(|value| value.format("%G-%V").to_string())
            .unwrap_or_default();
        *weeks.entry(week).or_default() += 1;
    }
    cap(weeks.len() as f64, 8.0) / 100.0
}

fn score(id: &str, value: f64, coverage: f64) -> VctiScore {
    VctiScore {
        id: id.into(),
        value: value.clamp(0.0, 100.0),
        coverage: coverage.clamp(0.0, 1.0),
    }
}

fn evidence(
    id: &str,
    label_key: &str,
    value: f64,
    format: &str,
    structural: bool,
) -> VctiEvidenceItem {
    VctiEvidenceItem {
        id: id.into(),
        label_key: label_key.into(),
        value,
        format: format.into(),
        provenance: "derived".into(),
        structural,
    }
}

fn weighted(values: &[(f64, f64)]) -> f64 {
    values
        .iter()
        .map(|(value, weight)| value.clamp(0.0, 100.0) * weight)
        .sum::<f64>()
        .clamp(0.0, 100.0)
}

fn average(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

fn ratio(numerator: f64, denominator: f64) -> f64 {
    if denominator <= f64::EPSILON {
        0.0
    } else {
        (numerator / denominator).max(0.0)
    }
}

fn cap(value: f64, target: f64) -> f64 {
    if target <= f64::EPSILON {
        0.0
    } else {
        (value / target * 100.0).clamp(0.0, 100.0)
    }
}

fn separation_capped_confidence(confidence: f64, type_margin: f64) -> f64 {
    if type_margin < 0.02 {
        confidence.min(72.0)
    } else if type_margin < 0.04 {
        confidence.min(79.0)
    } else {
        confidence
    }
}

fn inverse(value: f64) -> f64 {
    (100.0 - value).clamp(0.0, 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(day: &str) -> SessionBehaviorRecord {
        SessionBehaviorRecord {
            id: day.into(),
            started_at: format!("{day}T10:00:00Z"),
            agent: "codex".into(),
            model: Some("gpt-5".into()),
            active_seconds: 3_600,
            total_tokens: 20_000,
            cache_read_tokens: 10_000,
            tool_calls: 20,
            files_touched: 4,
            lines_changed: 120,
            errors: 0,
            verification_events: 2,
            human_interventions: 2,
            subagent_count: 0,
            model_switches: 0,
            longest_uninterrupted_seconds: 1_200,
            has_commit: true,
            git_review_events: 1,
            test_events: 1,
            build_events: 1,
            lint_events: 0,
            typecheck_events: 0,
            read_events: 2,
            search_events: 1,
            edit_events: 4,
            shell_events: 8,
            behavior: BehaviorSignals {
                prompt_count: 2,
                prompt_characters: 400,
                structured_prompts: 2,
                acceptance_criteria_prompts: 1,
                file_scope_prompts: 1,
                task_starts: 1,
                task_completions: 1,
                successful_tools: 8,
                completed_task_duration_ms: 600_000,
                prompt_structure_enabled: true,
                ..BehaviorSignals::default()
            },
        }
    }

    #[test]
    fn produces_a_stable_evidence_bound_profile() {
        let now = DateTime::parse_from_rfc3339("2026-07-23T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let records = (0..35)
            .map(|offset| record(&(now - Duration::days(offset)).date_naive().to_string()))
            .collect::<Vec<_>>();
        let profile = calculate(
            &records,
            BehaviorSummary {
                sessions: 35,
                structure_coverage: 1.0,
                lifecycle_coverage: 1.0,
                tool_result_coverage: 1.0,
                orchestration_coverage: 1.0,
                process_control_coverage: 1.0,
                ..BehaviorSummary::default()
            },
            3,
            true,
            true,
            now,
        );
        assert_eq!(profile.status, "stable");
        assert!(profile.primary_type.is_some());
        assert!(profile.evidence.len() >= 3);
        assert_eq!(profile.scores.len(), 6);
        assert_eq!(profile.dimensions.len(), 18);
    }

    #[test]
    fn does_not_force_a_type_when_data_is_thin() {
        let now = Utc::now();
        let profile = calculate(
            &[record(&now.date_naive().to_string())],
            BehaviorSummary::default(),
            1,
            true,
            false,
            now,
        );
        assert_eq!(profile.status, "collecting");
        assert!(profile.primary_type.is_none());
    }

    #[test]
    fn ambiguous_types_cannot_claim_high_match_confidence() {
        assert_eq!(separation_capped_confidence(96.0, 0.011), 72.0);
        assert_eq!(separation_capped_confidence(96.0, 0.031), 79.0);
        assert_eq!(separation_capped_confidence(96.0, 0.061), 96.0);
    }
}
