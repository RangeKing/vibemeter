use crate::models::{
    BehaviorSignals, BehaviorSummary, VctiBadge, VctiCollaboration, VctiCollaborationVisual,
    VctiDetailDiversity, VctiDetailVisual, VctiEvidenceItem, VctiIdentityEvidence,
    VctiIdentityVisual, VctiOptionalMetric, VctiProcessVariation, VctiProfile, VctiRhythmPeriod,
    VctiRhythmVisual, VctiScore, VctiTrendPoint, VctiVisualInput, VctiVisualMark, VctiVisualPath,
    VctiWorkRhythm,
};
use chrono::{DateTime, Duration, Local, Timelike, Utc};
use std::collections::{BTreeMap, HashMap, HashSet};

pub const ALGORITHM_VERSION: &str = "1.6.0";
pub const IDENTITY_VISUAL_VERSION: &str = "2.0.0";
const CANONICAL_WINDOW_DAYS: i64 = 90;
const HALF_LIFE_DAYS: f64 = 45.0;

pub fn window_days_for_range(range: &str) -> i64 {
    match range {
        "today" => 1,
        "7d" => 7,
        "30d" => 30,
        "90d" => 90,
        "180d" => 180,
        "year" => 365,
        "all" => 3651,
        _ => 30,
    }
}

fn range_for_window_days(window_days: i64) -> &'static str {
    match window_days {
        1 => "today",
        7 => "7d",
        30 => "30d",
        90 => "90d",
        180 => "180d",
        365 => "year",
        3651 => "all",
        _ => "custom",
    }
}

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
    pub retries: u64,
    pub error_events_available: bool,
    pub retry_events_available: bool,
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
    pub tool_categories: Vec<String>,
    pub explicit_skills: Vec<String>,
    pub explicit_skills_available: bool,
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
    prompt_signal: bool,
    modification_signal: bool,
    lifecycle_signal: bool,
    first_tool_signal: bool,
    token_signal: bool,
}

#[derive(Clone)]
struct TypeScore {
    code: &'static str,
    guild: &'static str,
    score: f64,
    raw_score: f64,
    distinctiveness: f64,
    eligible: bool,
}

pub fn calculate(
    records: &[SessionBehaviorRecord],
    behavior: BehaviorSummary,
    available_agents: u64,
    structure_analysis_enabled: bool,
    git_evidence_enabled: bool,
    now: DateTime<Utc>,
    window_days: i64,
) -> VctiProfile {
    let window_days = window_days.max(1);
    let period_end = now.date_naive();
    let period_start = (now - Duration::days(window_days - 1)).date_naive();
    let active_days = records
        .iter()
        .filter_map(|record| record.started_at.get(..10))
        .collect::<HashSet<_>>()
        .len() as u64;
    let mut status = if records.len() >= 80 && active_days >= 21 {
        "high-confidence"
    } else if records.len() >= 30 && active_days >= 7 {
        "stable"
    } else if records.len() >= 8 && active_days >= 2 {
        "preview"
    } else {
        "collecting"
    };
    let features = derive_features(records, available_agents, now, window_days);
    let mut candidates = rank_type_scores(type_scores(
        &features,
        structure_analysis_enabled,
        git_evidence_enabled,
        behavior.orchestration_coverage,
        behavior.process_control_coverage,
        available_agents,
    ));
    candidates.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.code.cmp(right.code))
    });
    let gated_top = candidates.first().filter(|candidate| {
        status != "collecting" && candidate.raw_score >= 42.0 && candidate.distinctiveness >= 4.0
    });
    let mut temporary = false;
    let top = if gated_top.is_none()
        && records.len() >= 3
        && candidates.first().is_some_and(|candidate| {
            candidate.raw_score >= 35.0 && candidate.distinctiveness >= 3.0
        }) {
        temporary = true;
        status = "preview";
        candidates.first()
    } else {
        gated_top
    };
    if window_days < CANONICAL_WINDOW_DAYS && top.is_some() {
        temporary = true;
        if status == "stable" || status == "high-confidence" {
            status = "preview";
        }
    }
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
    let mut confidence = separation_capped_confidence(
        (cap(records.len() as f64, 80.0) * 0.25
            + cap(active_days as f64, 21.0) * 0.20
            + coverage * 100.0 * 0.20
            + cap(type_margin, 0.12) * 0.20
            + temporal_stability * 100.0 * 0.15)
            .clamp(0.0, 100.0),
        type_margin,
    );
    if temporary {
        confidence = confidence.min(55.0);
    }
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
    let dimensions = dimension_scores(
        &features,
        &behavior,
        structure_analysis_enabled,
        git_evidence_enabled,
    );
    let identity_evidence = build_identity_evidence(records, window_days, &behavior);
    let identity_visual = build_identity_visual(
        range_for_window_days(window_days),
        top.map(|candidate| candidate.code),
        top.map(|candidate| candidate.guild),
        &dimensions,
        &identity_evidence,
    );
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
        window_days: window_days as u64,
        temporary,
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
        identity_evidence,
        identity_visual,
        behavior,
        missing_capabilities,
        structure_analysis_enabled,
        git_evidence_enabled,
    }
}

fn build_identity_visual(
    range: &str,
    primary_type: Option<&str>,
    guild: Option<&str>,
    dimensions: &[VctiScore],
    evidence: &VctiIdentityEvidence,
) -> VctiIdentityVisual {
    let dimensions_available = dimensions.iter().any(|score| score.coverage > 0.0);
    let available = primary_type.is_some() && guild.is_some() && dimensions_available;
    let inputs = vec![
        visual_input("identity", primary_type.is_some() && guild.is_some()),
        visual_input("dimensions", dimensions_available),
        visual_input("rhythm", evidence.rhythm.work_periods_available),
        visual_input(
            "collaboration",
            evidence.collaboration.subagent_starts.available,
        ),
        visual_input(
            "detail-diversity",
            evidence.detail_diversity.tool_categories.available,
        ),
        visual_input(
            "process-variation",
            evidence.process_variation.errors.available,
        ),
    ];
    let rhythm = build_rhythm_visual(&evidence.rhythm, primary_type.unwrap_or("collecting"));
    let collaboration = build_collaboration_visual(
        &evidence.collaboration,
        primary_type.unwrap_or("collecting"),
    );
    let detail = build_detail_visual(
        &evidence.detail_diversity,
        primary_type.unwrap_or("collecting"),
    );
    if !available {
        return VctiIdentityVisual {
            algorithm_version: ALGORITHM_VERSION.into(),
            version: IDENTITY_VISUAL_VERSION.into(),
            range: range.into(),
            available,
            inputs,
            contours: Vec::new(),
            rhythm,
            collaboration,
            detail,
        };
    }

    let primary_type = primary_type.unwrap_or_default();
    let guild = guild.unwrap_or("start");
    let phase = stable_fraction(primary_type) * std::f64::consts::TAU;
    let aspect = 0.94 + stable_fraction(guild) * 0.12;
    let observed = dimensions
        .iter()
        .filter(|score| score.coverage > 0.0)
        .collect::<Vec<_>>();
    let contours = (0..6usize)
        .map(|layer| {
            let base_radius = 45.0 - layer as f64 * 5.1;
            let points = (0..12usize)
                .map(|point| {
                    let score = observed[(point + layer * 2) % observed.len()];
                    let signal = (score.value - 50.0) / 50.0;
                    let angle = phase
                        + stable_fraction(score.id.as_str()) * 0.18
                        + std::f64::consts::TAU * point as f64 / 12.0;
                    let radius = base_radius + signal * (2.4 + layer as f64 * 0.14);
                    (
                        (50.0 + angle.cos() * radius * aspect).clamp(2.0, 98.0),
                        (50.0 + angle.sin() * radius / aspect).clamp(2.0, 98.0),
                    )
                })
                .collect::<Vec<_>>();
            VctiVisualPath {
                d: smooth_closed_path(&points),
                stroke_width: 0.72 + (6 - layer) as f64 * 0.08,
                opacity: 0.30 + (6 - layer) as f64 * 0.075,
            }
        })
        .collect();
    VctiIdentityVisual {
        algorithm_version: ALGORITHM_VERSION.into(),
        version: IDENTITY_VISUAL_VERSION.into(),
        range: range.into(),
        available,
        inputs,
        contours,
        rhythm,
        collaboration,
        detail,
    }
}

fn build_detail_visual(detail: &VctiDetailDiversity, identity_seed: &str) -> VctiDetailVisual {
    let available = detail.tool_categories.available
        && detail.explicit_skills.available
        && detail.tool_categories.value.is_some()
        && detail.explicit_skills.value.is_some();
    if !available {
        return VctiDetailVisual {
            available: false,
            tool_intensity: None,
            skill_intensity: None,
            tool_marks: Vec::new(),
            skill_marks: Vec::new(),
        };
    }
    let tool_count = (detail.tool_categories.value.unwrap_or(0.0).round() as usize).min(10);
    let skill_count = (detail.explicit_skills.value.unwrap_or(0.0).round() as usize).min(7);
    let seed = stable_fraction(identity_seed) * std::f64::consts::TAU;
    let marks = |count: usize, radius: f64, phase: f64, is_skill: bool| {
        (0..count)
            .map(|index| {
                let angle = phase + std::f64::consts::TAU * index as f64 / count.max(1) as f64;
                let band = radius + (index % 3) as f64 * 2.8;
                VctiVisualMark {
                    cx: 50.0 + angle.cos() * band,
                    cy: 50.0 + angle.sin() * band,
                    radius: if is_skill {
                        1.25 + (index % 2) as f64 * 0.35
                    } else {
                        0.72 + (index % 3) as f64 * 0.18
                    },
                    opacity: if is_skill { 0.72 } else { 0.48 },
                }
            })
            .collect::<Vec<_>>()
    };
    VctiDetailVisual {
        available: true,
        tool_intensity: Some((tool_count as f64 / 10.0).clamp(0.0, 1.0)),
        skill_intensity: Some((skill_count as f64 / 7.0).clamp(0.0, 1.0)),
        tool_marks: marks(tool_count, 44.0, seed, false),
        skill_marks: marks(skill_count, 38.0, seed + 0.41, true),
    }
}

fn build_collaboration_visual(
    collaboration: &VctiCollaboration,
    identity_seed: &str,
) -> VctiCollaborationVisual {
    let available = collaboration.subagent_starts.available
        && collaboration.parallel_batches.available
        && collaboration.subagent_starts.value.is_some()
        && collaboration.parallel_batches.value.is_some();
    if !available {
        return VctiCollaborationVisual {
            available: false,
            branch_intensity: None,
            parallel_intensity: None,
            paths: Vec::new(),
        };
    }
    let branch_intensity =
        (collaboration.subagent_starts.value.unwrap_or(0.0) / 8.0).clamp(0.0, 1.0);
    let parallel_intensity =
        (collaboration.parallel_batches.value.unwrap_or(0.0) / 5.0).clamp(0.0, 1.0);
    let count = (collaboration.subagent_starts.value.unwrap_or(0.0).round() as usize).min(8);
    let seed_phase = stable_fraction(identity_seed) * std::f64::consts::TAU;
    let paths = (0..count)
        .map(|index| {
            let angle = seed_phase + std::f64::consts::TAU * index as f64 / count.max(1) as f64;
            let spread = 18.0 + parallel_intensity * 18.0;
            let start_angle = angle - (0.24 + parallel_intensity * 0.12);
            VctiVisualPath {
                d: format!(
                    "M{:.2},{:.2} Q{:.2},{:.2} {:.2},{:.2}",
                    50.0 + start_angle.cos() * 24.0,
                    50.0 + start_angle.sin() * 24.0,
                    50.0 + angle.cos() * spread,
                    50.0 + angle.sin() * spread,
                    50.0 + angle.cos() * 48.0,
                    50.0 + angle.sin() * 48.0,
                ),
                stroke_width: 0.48 + parallel_intensity * 0.42,
                opacity: 0.25 + branch_intensity * 0.42,
            }
        })
        .collect();
    VctiCollaborationVisual {
        available: true,
        branch_intensity: Some(branch_intensity),
        parallel_intensity: Some(parallel_intensity),
        paths,
    }
}

fn build_rhythm_visual(rhythm: &VctiWorkRhythm, identity_seed: &str) -> VctiRhythmVisual {
    let available = rhythm.work_periods_available
        && rhythm.active_days.available
        && rhythm.sessions_per_day.available
        && rhythm.active_days.value.is_some()
        && rhythm.sessions_per_day.value.is_some();
    if !available {
        return VctiRhythmVisual {
            available: false,
            phase: None,
            active_intensity: None,
            session_intensity: None,
            density: None,
            paths: Vec::new(),
        };
    }
    let active_intensity = (rhythm.active_days.value.unwrap_or(0.0) / 21.0).clamp(0.0, 1.0);
    let session_intensity = (rhythm.sessions_per_day.value.unwrap_or(0.0) / 4.0).clamp(0.0, 1.0);
    let density = (active_intensity * 0.55 + session_intensity * 0.45).clamp(0.0, 1.0);
    let total_share = rhythm
        .work_periods
        .iter()
        .map(|period| period.share)
        .sum::<f64>();
    if total_share <= f64::EPSILON || density <= f64::EPSILON {
        return VctiRhythmVisual {
            available: true,
            phase: None,
            active_intensity: Some(active_intensity),
            session_intensity: Some(session_intensity),
            density: Some(density),
            paths: Vec::new(),
        };
    }
    let angle_for = |id: &str| match id {
        "night" => -2.35,
        "morning" => -0.78,
        "afternoon" => 0.76,
        _ => 2.34,
    };
    let vector = rhythm
        .work_periods
        .iter()
        .fold((0.0, 0.0), |(x, y), period| {
            let angle: f64 = angle_for(&period.id);
            (
                x + angle.cos() * period.share,
                y + angle.sin() * period.share,
            )
        });
    let phase = vector.1.atan2(vector.0) + stable_fraction(identity_seed) * 0.08;
    let count = (3.0 + active_intensity * 3.0 + session_intensity * 3.0).round() as usize;
    let paths = (0..count.min(9))
        .map(|index| {
            let spread =
                (index as f64 - (count.saturating_sub(1)) as f64 / 2.0) * (7.6 - density * 2.4);
            let perpendicular = phase + std::f64::consts::FRAC_PI_2;
            let start = (
                50.0 - phase.cos() * 49.0 + perpendicular.cos() * spread,
                50.0 - phase.sin() * 49.0 + perpendicular.sin() * spread,
            );
            let end = (
                50.0 + phase.cos() * 49.0 + perpendicular.cos() * spread,
                50.0 + phase.sin() * 49.0 + perpendicular.sin() * spread,
            );
            let bend = ((index % 3) as f64 - 1.0) * (5.0 + session_intensity * 4.0);
            VctiVisualPath {
                d: format!(
                    "M{:.2},{:.2} Q{:.2},{:.2} {:.2},{:.2}",
                    start.0,
                    start.1,
                    50.0 + perpendicular.cos() * (spread + bend),
                    50.0 + perpendicular.sin() * (spread + bend),
                    end.0,
                    end.1
                ),
                stroke_width: 0.42 + session_intensity * 0.38,
                opacity: 0.18 + active_intensity * 0.30,
            }
        })
        .collect();
    VctiRhythmVisual {
        available: true,
        phase: Some(phase),
        active_intensity: Some(active_intensity),
        session_intensity: Some(session_intensity),
        density: Some(density),
        paths,
    }
}

fn visual_input(id: &str, available: bool) -> VctiVisualInput {
    VctiVisualInput {
        id: id.into(),
        available,
    }
}

fn smooth_closed_path(points: &[(f64, f64)]) -> String {
    let first = points[0];
    let last = points[points.len() - 1];
    let mut output = format!(
        "M{:.2},{:.2}",
        (last.0 + first.0) / 2.0,
        (last.1 + first.1) / 2.0
    );
    for (index, point) in points.iter().enumerate() {
        let next = points[(index + 1) % points.len()];
        output.push_str(&format!(
            "Q{:.2},{:.2} {:.2},{:.2}",
            point.0,
            point.1,
            (point.0 + next.0) / 2.0,
            (point.1 + next.1) / 2.0
        ));
    }
    output.push('Z');
    output
}

fn stable_fraction(value: &str) -> f64 {
    let hash = value.bytes().fold(2_166_136_261u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(16_777_619)
    });
    f64::from(hash % 10_000) / 10_000.0
}

fn build_identity_evidence(
    records: &[SessionBehaviorRecord],
    window_days: i64,
    behavior: &BehaviorSummary,
) -> VctiIdentityEvidence {
    let rhythm = aggregate_work_rhythm(records, window_days);
    let collaboration = aggregate_collaboration(behavior);
    let detail_diversity = aggregate_detail(records);
    let process_variation = aggregate_process_variation(records, behavior);
    VctiIdentityEvidence {
        rhythm,
        collaboration,
        detail_diversity,
        process_variation,
    }
}

fn aggregate_collaboration(behavior: &BehaviorSummary) -> VctiCollaboration {
    let available = behavior.orchestration_capable_sessions > 0;
    let subagent_starts = available.then_some(behavior.subagent_starts as f64);
    let parallel_batches = available.then_some(behavior.parallel_batches as f64);
    VctiCollaboration {
        subagent_starts: VctiOptionalMetric {
            value: subagent_starts,
            available,
        },
        parallel_batches: VctiOptionalMetric {
            value: parallel_batches,
            available,
        },
    }
}

fn aggregate_detail(records: &[SessionBehaviorRecord]) -> VctiDetailDiversity {
    let tool_categories_available = records
        .iter()
        .all(|record| record.tool_calls == 0 || !record.tool_categories.is_empty());
    let tool_categories = records
        .iter()
        .flat_map(|record| record.tool_categories.iter().cloned())
        .collect::<HashSet<_>>()
        .len() as u64;
    let explicit_skills_available = !records.is_empty()
        && records
            .iter()
            .all(|record| record.explicit_skills_available);
    let explicit_skills = records
        .iter()
        .flat_map(|record| record.explicit_skills.iter().cloned())
        .collect::<HashSet<_>>()
        .len() as u64;
    VctiDetailDiversity {
        tool_categories: VctiOptionalMetric {
            value: tool_categories_available.then_some(tool_categories as f64),
            available: tool_categories_available,
        },
        explicit_skills: VctiOptionalMetric {
            value: explicit_skills_available.then_some(explicit_skills as f64),
            available: explicit_skills_available,
        },
    }
}

fn aggregate_process_variation(
    records: &[SessionBehaviorRecord],
    behavior: &BehaviorSummary,
) -> VctiProcessVariation {
    let errors_available = records.iter().all(|record| record.error_events_available);
    let retries_available = records.iter().all(|record| record.retry_events_available);
    let rollbacks_available = behavior.process_control_capable_sessions > 0;
    let errors =
        errors_available.then_some(records.iter().map(|record| record.errors).sum::<u64>());
    let retries =
        retries_available.then_some(records.iter().map(|record| record.retries).sum::<u64>());
    let rollbacks = rollbacks_available.then_some(behavior.rollbacks);
    VctiProcessVariation {
        errors: VctiOptionalMetric {
            value: errors.map(|value| value as f64),
            available: errors_available,
        },
        retries: VctiOptionalMetric {
            value: retries.map(|value| value as f64),
            available: retries_available,
        },
        rollbacks: VctiOptionalMetric {
            value: rollbacks.map(|value| value as f64),
            available: rollbacks_available,
        },
    }
}

fn aggregate_work_rhythm(records: &[SessionBehaviorRecord], window_days: i64) -> VctiWorkRhythm {
    let parsed = records
        .iter()
        .filter_map(|record| DateTime::parse_from_rfc3339(&record.started_at).ok())
        .collect::<Vec<_>>();
    let timestamps_available = records.is_empty() || parsed.len() == records.len();
    let mut period_counts = BTreeMap::from([
        ("night", 0_u64),
        ("morning", 0_u64),
        ("afternoon", 0_u64),
        ("evening", 0_u64),
    ]);
    let mut active_days = HashSet::new();
    for timestamp in &parsed {
        let local = timestamp.with_timezone(&Local);
        *period_counts.entry(work_period(local.hour())).or_default() += 1;
        active_days.insert(local.date_naive());
    }
    let total = parsed.len() as f64;
    let work_periods = ["night", "morning", "afternoon", "evening"]
        .into_iter()
        .map(|id| {
            let sessions = period_counts[id];
            VctiRhythmPeriod {
                id: id.into(),
                sessions,
                share: if total == 0.0 {
                    0.0
                } else {
                    sessions as f64 / total
                },
            }
        })
        .collect::<Vec<_>>();
    let sessions_per_day = records.len() as f64 / window_days.max(1) as f64;
    let active_day_value = timestamps_available.then_some(active_days.len() as f64);
    VctiWorkRhythm {
        work_periods,
        work_periods_available: timestamps_available,
        active_days: VctiOptionalMetric {
            value: active_day_value,
            available: timestamps_available,
        },
        sessions_per_day: VctiOptionalMetric {
            value: Some(sessions_per_day),
            available: true,
        },
    }
}

fn work_period(hour: u32) -> &'static str {
    match hour {
        0..=5 => "night",
        6..=11 => "morning",
        12..=17 => "afternoon",
        _ => "evening",
    }
}

fn derive_features(
    records: &[SessionBehaviorRecord],
    available_agents: u64,
    now: DateTime<Utc>,
    window_days: i64,
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
    let mut uninterrupted_seconds = 0.0;
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
            .unwrap_or(window_days as f64);
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
        uninterrupted_seconds += record
            .longest_uninterrupted_seconds
            .max(record.active_seconds) as f64
            * weight;
    }

    let ws = weighted_sessions.max(0.001);
    let prompt_structure_rate = ratio(structured_prompts, prompt_count);
    let acceptance_rate = ratio(acceptance_prompts, prompt_count);
    let file_scope_rate = ratio(scope_prompts, prompt_count);
    let plan_rate = ratio(plans, ws);
    let avg_prompt_chars = ratio(prompt_characters, prompt_count);
    let prompt_signal = prompt_count > f64::EPSILON;
    let modification_signal = modified_sessions > f64::EPSILON;
    let lifecycle_signal = completions + aborts > f64::EPSILON;
    let first_tool_signal = first_tool_weight > f64::EPSILON;
    let token_signal = total_tokens > f64::EPSILON;
    let scope_drift = cap(ratio(goal_changes, ws), 0.28);
    let requirement_clarity = if prompt_signal {
        weighted(&[
            (prompt_structure_rate * 100.0, 0.38),
            (cap(acceptance_rate, 0.28), 0.26),
            (cap(file_scope_rate, 0.38), 0.22),
            (cap(plan_rate, 0.45), 0.14),
        ])
    } else {
        50.0
    };
    let exploration = (scope_drift * 0.38
        + cap(ratio(model_switches, ws), 0.20) * 0.22
        + cap(ratio(dependency_events, ws), 0.40) * 0.20
        + if prompt_signal {
            inverse(cap(avg_prompt_chars, 1_200.0)) * 0.20
        } else {
            50.0 * 0.20
        })
    .clamp(0.0, 100.0);
    let delegation = (cap(ratio(files, ws), 8.0) * 0.35
        + cap(ratio(tools, prompt_count.max(ws)), 18.0) * 0.30
        + cap(ratio(uninterrupted_seconds, ws), 7_200.0) * 0.20
        + inverse(cap(ratio(interventions, ws), 5.0)) * 0.15)
        .clamp(0.0, 100.0);
    let human_intervention = cap(ratio(interventions, ws), 4.0);
    let parallel_orchestration = (cap(ratio(subagents, ws), 0.65) * 0.65
        + cap(ratio(parallel_batches, ws), 0.45) * 0.35)
        .clamp(0.0, 100.0);
    let diff_review = if modification_signal {
        cap(ratio(git_reviews, modified_sessions), 1.15)
    } else {
        50.0
    };
    let automated_verification = if modification_signal {
        weighted(&[
            (ratio(verified_modified, modified_sessions) * 100.0, 0.70),
            (cap(ratio(tests, modified_sessions), 1.2), 0.30),
        ])
    } else {
        50.0
    };
    let rollback_awareness = if modification_signal {
        (cap(ratio(rollbacks, modified_sessions), 0.16) * 0.58
            + ratio(committed_sessions, modified_sessions) * 42.0)
            .clamp(0.0, 100.0)
    } else {
        50.0
    };
    let root_cause = (cap(
        ratio(reads_and_searches + shell_events * 0.08, edits.max(1.0)),
        0.95,
    ) * 0.45
        + cap(ratio(tests, modified_sessions.max(1.0)), 1.0) * 0.30
        + cap(ratio(errors, ws), 0.35) * 0.25)
        .clamp(0.0, 100.0);
    let local_fix = if modification_signal {
        (inverse(cap(ratio(files, modified_sessions), 9.0)) * 0.58
            + inverse(cap(ratio(lines, modified_sessions), 480.0)) * 0.42)
            .clamp(0.0, 100.0)
    } else {
        50.0
    };
    let automation = (cap(ratio(automation_events, ws), 0.30) * 0.58
        + cap(ratio(tools, ws), 55.0) * 0.24
        + cap(plan_rate, 0.60) * 0.18)
        .clamp(0.0, 100.0);
    let first_result_speed = if first_tool_signal {
        let average_first_tool_ms = ratio(first_tool_ms, first_tool_weight);
        inverse(cap(average_first_tool_ms, 12.0 * 60_000.0))
    } else {
        50.0
    };
    let iteration_granularity = if modification_signal {
        (cap(ratio(files, modified_sessions), 10.0) * 0.58
            + cap(ratio(lines, modified_sessions), 600.0) * 0.42)
            .clamp(0.0, 100.0)
    } else {
        50.0
    };
    let completion = if lifecycle_signal {
        ratio(completions, completions + aborts)
    } else {
        0.5
    };
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
    let polish = if modification_signal {
        (cap(ratio(style_events, modified_sessions), 1.8) * 0.48
            + cap(ratio(document_events, modified_sessions), 0.65) * 0.20
            + inverse(first_result_speed) * 0.12
            + inverse(local_fix) * 0.20)
            .clamp(0.0, 100.0)
    } else {
        50.0
    };
    let infrastructure = if modification_signal {
        cap(ratio(infra_events, modified_sessions), 0.85)
    } else {
        50.0
    };
    let dependency_reuse = if modification_signal {
        cap(ratio(dependency_events, modified_sessions), 0.55)
    } else {
        50.0
    };
    let mean_day = if daily.is_empty() {
        0.0
    } else {
        daily.values().sum::<f64>() / daily.len() as f64
    };
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
    let long_session_rate = ratio(long_session_count as f64, records.len() as f64);
    let burst = (cap(variance, 1.2) * 0.62 + cap(long_session_rate, 0.18) * 0.38).clamp(0.0, 100.0);

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
        prompt_signal,
        modification_signal,
        lifecycle_signal,
        first_tool_signal,
        token_signal,
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
            structure_enabled && features.prompt_signal,
        ),
        (
            "SPEC",
            "start",
            weighted(&[
                (features.requirement_clarity, 0.58),
                (inverse(features.scope_drift), 0.24),
                (features.context_reuse, 0.18),
            ]),
            structure_enabled && features.prompt_signal,
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
            structure_enabled && features.prompt_signal,
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
            structure_enabled && features.prompt_signal && features.modification_signal,
        ),
        (
            "YOLO",
            "agent",
            weighted(&[
                (features.delegation, 0.46),
                (features.iteration_granularity, 0.27),
                (inverse(guardrail), 0.27),
            ]),
            features.modification_signal,
        ),
        (
            "LOOP",
            "agent",
            weighted(&[
                (features.human_intervention, 0.50),
                (inverse(features.iteration_granularity), 0.27),
                (features.polish, 0.23),
            ]),
            features.modification_signal || process_coverage >= 0.30,
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
            features.modification_signal,
        ),
        (
            "TEST",
            "quality",
            weighted(&[
                (features.automated_verification, 0.72),
                (features.root_cause, 0.16),
                (features.rollback_awareness, 0.12),
            ]),
            features.modification_signal,
        ),
        (
            "DOCS",
            "quality",
            weighted(&[
                (features.context_reuse, 0.50),
                (features.requirement_clarity, 0.30),
                (inverse(features.scope_drift), 0.20),
            ]),
            features.modification_signal,
        ),
        (
            "UNDO",
            "quality",
            weighted(&[
                (features.rollback_awareness, 0.68),
                (features.iteration_granularity, 0.18),
                (features.exploration, 0.14),
            ]),
            features.modification_signal && (git_enabled || process_coverage >= 0.30),
        ),
        (
            "DEBUG",
            "debug",
            weighted(&[
                (features.root_cause, 0.62),
                (features.automated_verification, 0.20),
                (inverse(features.first_result_speed), 0.18),
            ]),
            features.modification_signal,
        ),
        (
            "PATCH",
            "debug",
            weighted(&[
                (features.local_fix, 0.58),
                (features.first_result_speed, 0.28),
                (inverse(features.infrastructure), 0.14),
            ]),
            features.modification_signal,
        ),
        (
            "STACK",
            "debug",
            weighted(&[
                (features.infrastructure, 0.56),
                (features.dependency_reuse, 0.24),
                (features.iteration_granularity, 0.20),
            ]),
            features.modification_signal,
        ),
        (
            "AUTO",
            "debug",
            weighted(&[
                (features.automation, 0.64),
                (features.parallel_orchestration, 0.18),
                (features.context_reuse, 0.18),
            ]),
            features.modification_signal,
        ),
        (
            "SHIP",
            "delivery",
            weighted(&[
                (features.shipping_tendency, 0.55),
                (features.first_result_speed, 0.26),
                (features.completion, 0.19),
            ]),
            features.lifecycle_signal || features.modification_signal,
        ),
        (
            "RUSH",
            "delivery",
            weighted(&[
                (features.burst, 0.58),
                (features.completion, 0.24),
                (features.first_result_speed, 0.18),
            ]),
            features.lifecycle_signal,
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
            structure_enabled && features.modification_signal && features.first_tool_signal,
        ),
        (
            "DETAIL",
            "delivery",
            weighted(&[
                (features.polish, 0.62),
                (inverse(features.first_result_speed), 0.18),
                (features.human_intervention, 0.20),
            ]),
            features.modification_signal,
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
            features.token_signal,
        ),
        (
            "CACHE",
            "tools",
            weighted(&[
                (features.context_reuse, 0.68),
                (features.requirement_clarity, 0.18),
                (features.cost_routing, 0.14),
            ]),
            features.token_signal || features.prompt_signal,
        ),
        (
            "BUDDY",
            "tools",
            weighted(&[
                (inverse(features.tool_switching), 0.62),
                (features.context_reuse, 0.22),
                (inverse(features.scope_drift), 0.16),
            ]),
            available_agents >= 2 && (features.token_signal || features.prompt_signal),
        ),
    ];
    types
        .into_iter()
        .map(|(code, guild, raw_score, eligible)| TypeScore {
            code,
            guild,
            score: raw_score,
            raw_score,
            distinctiveness: 0.0,
            eligible,
        })
        .collect()
}

fn rank_type_scores(mut types: Vec<TypeScore>) -> Vec<TypeScore> {
    let mut guild_totals = HashMap::<&str, (f64, usize)>::new();
    for candidate in &types {
        let total = guild_totals.entry(candidate.guild).or_default();
        total.0 += candidate.raw_score;
        total.1 += 1;
    }
    for candidate in &mut types {
        let (total, count) = guild_totals[candidate.guild];
        let guild_mean = total / count.max(1) as f64;
        candidate.distinctiveness = candidate.raw_score - guild_mean;
        candidate.score =
            (50.0 + candidate.distinctiveness * 1.35 + (candidate.raw_score - 50.0) * 0.20)
                .clamp(0.0, 100.0);
    }
    types.retain(|candidate| candidate.eligible);
    types
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

fn dimension_scores(
    features: &Features,
    behavior: &BehaviorSummary,
    structure_enabled: bool,
    git_enabled: bool,
) -> Vec<VctiScore> {
    let session_coverage = f64::from(behavior.sessions > 0);
    let structure_coverage = if structure_enabled && features.prompt_signal {
        behavior.structure_coverage
    } else {
        0.0
    };
    let modification_coverage = if features.modification_signal {
        session_coverage
    } else {
        0.0
    };
    let first_result_coverage = if features.first_tool_signal {
        session_coverage
    } else {
        0.0
    };
    let token_coverage = if features.token_signal {
        session_coverage
    } else {
        0.0
    };
    [
        (
            "requirementClarity",
            features.requirement_clarity,
            structure_coverage,
        ),
        ("exploration", features.exploration, session_coverage),
        (
            "scopeDrift",
            features.scope_drift,
            behavior.process_control_coverage,
        ),
        (
            "delegation",
            features.delegation,
            behavior.orchestration_coverage,
        ),
        (
            "humanIntervention",
            features.human_intervention,
            behavior.lifecycle_coverage,
        ),
        (
            "parallelOrchestration",
            features.parallel_orchestration,
            behavior.orchestration_coverage,
        ),
        (
            "diffReview",
            features.diff_review,
            if git_enabled {
                modification_coverage
            } else {
                modification_coverage.min(behavior.tool_result_coverage)
            },
        ),
        (
            "automatedVerification",
            features.automated_verification,
            modification_coverage.min(behavior.tool_result_coverage),
        ),
        (
            "rollbackAwareness",
            features.rollback_awareness,
            modification_coverage.min(behavior.process_control_coverage),
        ),
        (
            "rootCause",
            features.root_cause,
            behavior.tool_result_coverage,
        ),
        ("localFix", features.local_fix, modification_coverage),
        (
            "automation",
            features.automation,
            behavior.tool_result_coverage,
        ),
        (
            "firstResultSpeed",
            features.first_result_speed,
            first_result_coverage,
        ),
        (
            "iterationGranularity",
            features.iteration_granularity,
            modification_coverage,
        ),
        (
            "shippingTendency",
            features.shipping_tendency,
            behavior.lifecycle_coverage.max(if git_enabled {
                modification_coverage
            } else {
                0.0
            }),
        ),
        ("toolSwitching", features.tool_switching, session_coverage),
        ("costRouting", features.cost_routing, token_coverage),
        (
            "contextReuse",
            features.context_reuse,
            token_coverage.max(behavior.process_control_coverage),
        ),
    ]
    .into_iter()
    .map(|(id, value, coverage)| score(id, value, coverage))
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
        let features = derive_features(&slice, available_agents, end, 14);
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
        confidence.min(65.0)
    } else if type_margin < 0.04 {
        confidence.min(72.0)
    } else if type_margin < 0.08 {
        confidence.min(80.0)
    } else if type_margin < 0.12 {
        confidence.min(86.0)
    } else {
        confidence.min(92.0)
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
            retries: 0,
            error_events_available: true,
            retry_events_available: true,
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
            tool_categories: vec!["shell".into(), "edit".into(), "test".into()],
            explicit_skills: Vec::new(),
            explicit_skills_available: true,
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
            90,
        );
        assert_eq!(profile.status, "stable");
        assert!(!profile.temporary);
        assert_eq!(profile.window_days, 90);
        assert!(profile.primary_type.is_some());
        assert!(profile.evidence.len() >= 3);
        assert_eq!(profile.scores.len(), 6);
        assert_eq!(profile.dimensions.len(), 18);
    }

    #[test]
    fn identity_art_foundation_is_deterministic_and_keeps_a_versioned_range() {
        let now = DateTime::parse_from_rfc3339("2026-07-23T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let records = (0..35)
            .map(|offset| record(&(now - Duration::days(offset)).date_naive().to_string()))
            .collect::<Vec<_>>();
        let behavior = BehaviorSummary {
            sessions: 35,
            structure_coverage: 1.0,
            lifecycle_coverage: 1.0,
            tool_result_coverage: 1.0,
            orchestration_coverage: 1.0,
            process_control_coverage: 1.0,
            ..BehaviorSummary::default()
        };

        let first = calculate(&records, behavior.clone(), 3, true, true, now, 90);
        let replay = calculate(&records, behavior, 3, true, true, now, 90);

        assert_eq!(
            serde_json::to_value(&first.identity_evidence).unwrap(),
            serde_json::to_value(&replay.identity_evidence).unwrap()
        );
        assert_eq!(first.identity_visual.version, IDENTITY_VISUAL_VERSION);
        assert_eq!(first.identity_visual.range, "90d");
        assert!(first.identity_visual.available);
        assert!(!first.identity_visual.contours.is_empty());
        assert_eq!(
            serde_json::to_value(&first.identity_visual).unwrap(),
            serde_json::to_value(&replay.identity_visual).unwrap()
        );
    }

    #[test]
    fn work_rhythm_preserves_zero_activity_and_missing_timestamps() {
        assert_eq!(work_period(2), "night");
        assert_eq!(work_period(9), "morning");
        assert_eq!(work_period(15), "afternoon");
        assert_eq!(work_period(21), "evening");
        let empty = aggregate_work_rhythm(&[], 30);
        assert!(empty.work_periods_available);
        assert_eq!(empty.active_days.value, Some(0.0));
        assert_eq!(empty.sessions_per_day.value, Some(0.0));

        let mut invalid = record("2026-07-23");
        invalid.started_at = "not-recorded".into();
        let partial = aggregate_work_rhythm(&[invalid], 30);
        assert!(!partial.work_periods_available);
        assert!(!partial.active_days.available);
        assert_eq!(partial.active_days.value, None);
        assert!(partial.sessions_per_day.available);
        assert!(partial.sessions_per_day.value.is_some());
    }

    #[test]
    fn rhythm_visual_distinguishes_period_density_zero_and_missing() {
        let mut day = record("2026-07-23");
        day.started_at = "2026-07-23T10:00:00Z".into();
        let morning = aggregate_work_rhythm(&[day.clone()], 7);
        day.started_at = "2026-07-23T23:00:00Z".into();
        let night = aggregate_work_rhythm(&[day], 7);
        let morning_visual = build_rhythm_visual(&morning, "SPEC");
        let night_visual = build_rhythm_visual(&night, "SPEC");
        assert_ne!(morning_visual.phase, night_visual.phase);
        assert_ne!(morning_visual.paths, night_visual.paths);

        let zero = aggregate_work_rhythm(&[], 7);
        let zero_visual = build_rhythm_visual(&zero, "SPEC");
        assert!(zero_visual.available);
        assert_eq!(zero_visual.density, Some(0.0));
        assert!(zero_visual.paths.is_empty());

        let mut invalid = record("2026-07-23");
        invalid.started_at = "not-recorded".into();
        let missing_visual = build_rhythm_visual(&aggregate_work_rhythm(&[invalid], 7), "SPEC");
        assert!(!missing_visual.available);
        assert_eq!(missing_visual.density, None);
    }

    #[test]
    fn collaboration_visual_caps_branches_and_separates_zero_from_missing() {
        let zero = VctiCollaboration {
            subagent_starts: VctiOptionalMetric {
                value: Some(0.0),
                available: true,
            },
            parallel_batches: VctiOptionalMetric {
                value: Some(0.0),
                available: true,
            },
        };
        let high = VctiCollaboration {
            subagent_starts: VctiOptionalMetric {
                value: Some(100.0),
                available: true,
            },
            parallel_batches: VctiOptionalMetric {
                value: Some(100.0),
                available: true,
            },
        };
        let missing = VctiCollaboration {
            subagent_starts: VctiOptionalMetric {
                value: None,
                available: false,
            },
            parallel_batches: VctiOptionalMetric {
                value: None,
                available: false,
            },
        };
        assert!(build_collaboration_visual(&zero, "SPEC").paths.is_empty());
        assert!(build_collaboration_visual(&high, "SPEC").paths.len() <= 8);
        assert!(!build_collaboration_visual(&missing, "SPEC").available);
    }

    #[test]
    fn detail_visual_uses_aggregate_counts_without_names_and_caps_marks() {
        let detail = VctiDetailDiversity {
            tool_categories: VctiOptionalMetric {
                value: Some(99.0),
                available: true,
            },
            explicit_skills: VctiOptionalMetric {
                value: Some(99.0),
                available: true,
            },
        };
        let visual = build_detail_visual(&detail, "SPEC");
        assert!(visual.available);
        assert!(visual.tool_marks.len() <= 10);
        assert!(visual.skill_marks.len() <= 7);
        let serialized = serde_json::to_string(&visual).unwrap();
        for private in ["shell", "edit", "tdd", "/Users/"] {
            assert!(!serialized.contains(private));
        }
        let missing = VctiDetailDiversity {
            tool_categories: VctiOptionalMetric {
                value: None,
                available: false,
            },
            explicit_skills: VctiOptionalMetric {
                value: None,
                available: false,
            },
        };
        assert!(!build_detail_visual(&missing, "SPEC").available);
    }

    #[test]
    fn collaboration_counts_distinguish_missing_from_zero() {
        let observed_zero = aggregate_collaboration(&BehaviorSummary {
            sessions: 4,
            orchestration_capable_sessions: 4,
            orchestration_coverage: 1.0,
            ..BehaviorSummary::default()
        });
        assert_eq!(observed_zero.subagent_starts.value, Some(0.0));
        assert_eq!(observed_zero.parallel_batches.value, Some(0.0));

        let observed = aggregate_collaboration(&BehaviorSummary {
            sessions: 4,
            subagent_starts: 1,
            orchestration_capable_sessions: 4,
            orchestration_coverage: 1.0,
            ..BehaviorSummary::default()
        });
        assert_eq!(observed.subagent_starts.value, Some(1.0));

        let missing = aggregate_collaboration(&BehaviorSummary {
            sessions: 4,
            ..BehaviorSummary::default()
        });
        assert!(!missing.subagent_starts.available);
        assert_eq!(missing.subagent_starts.value, None);
        assert_eq!(missing.parallel_batches.value, None);
    }

    #[test]
    fn explicit_tool_and_skill_counts_preserve_missing_without_names() {
        let mut one = record("2026-07-23");
        one.tool_categories = vec!["edit".into()];
        one.explicit_skills = vec!["tdd".into()];
        let observed = aggregate_detail(&[one]);
        assert_eq!(observed.tool_categories.value, Some(1.0));
        assert_eq!(observed.explicit_skills.value, Some(1.0));

        let mut missing = record("2026-07-23");
        missing.tool_calls = 3;
        missing.tool_categories.clear();
        let partial = aggregate_detail(&[missing]);
        assert!(!partial.tool_categories.available);
        assert_eq!(partial.tool_categories.value, None);
        assert!(partial.explicit_skills.available);
        assert_eq!(partial.explicit_skills.value, Some(0.0));

        let mut legacy = record("2026-07-23");
        legacy.explicit_skills_available = false;
        legacy.explicit_skills.clear();
        let legacy_detail = aggregate_detail(&[legacy]);
        assert!(!legacy_detail.explicit_skills.available);
        assert_eq!(legacy_detail.explicit_skills.value, None);
    }

    #[test]
    fn process_counts_are_descriptive_and_missing_aware() {
        let behavior = BehaviorSummary {
            sessions: 1,
            process_control_capable_sessions: 1,
            process_control_coverage: 1.0,
            ..BehaviorSummary::default()
        };
        let observed_zero = aggregate_process_variation(&[record("2026-07-23")], &behavior);
        assert_eq!(observed_zero.errors.value, Some(0.0));
        assert_eq!(observed_zero.retries.value, Some(0.0));
        assert_eq!(observed_zero.rollbacks.value, Some(0.0));

        let mut missing = record("2026-07-23");
        missing.error_events_available = false;
        missing.retry_events_available = false;
        let unavailable = aggregate_process_variation(&[missing], &BehaviorSummary::default());
        assert_eq!(unavailable.errors.value, None);
        assert_eq!(unavailable.retries.value, None);
        assert_eq!(unavailable.rollbacks.value, None);
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
            90,
        );
        assert_eq!(profile.status, "collecting");
        assert!(profile.primary_type.is_none());
        assert!(!profile.temporary);
    }

    #[test]
    fn short_windows_mark_temporary_profiles() {
        let now = DateTime::parse_from_rfc3339("2026-07-23T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let records = (0..12)
            .map(|offset| record(&(now - Duration::days(offset)).date_naive().to_string()))
            .collect::<Vec<_>>();
        let profile = calculate(
            &records,
            BehaviorSummary {
                sessions: 12,
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
            7,
        );
        assert!(profile.temporary);
        assert_eq!(profile.window_days, 7);
        assert_eq!(profile.status, "preview");
        assert!(profile.confidence <= 55.0);
    }

    #[test]
    fn ambiguous_types_cannot_claim_high_match_confidence() {
        assert_eq!(separation_capped_confidence(96.0, 0.011), 65.0);
        assert_eq!(separation_capped_confidence(96.0, 0.031), 72.0);
        assert_eq!(separation_capped_confidence(96.0, 0.061), 80.0);
        assert_eq!(separation_capped_confidence(96.0, 0.101), 86.0);
        assert_eq!(separation_capped_confidence(96.0, 0.161), 92.0);
    }

    #[test]
    fn percentage_components_keep_their_declared_weight() {
        let now = DateTime::parse_from_rfc3339("2026-07-23T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut sample = record("2026-07-23");
        sample.has_commit = false;
        sample.verification_events = 0;
        sample.test_events = 1;
        sample.build_events = 0;
        sample.behavior.prompt_count = 4;
        sample.behavior.structured_prompts = 1;
        sample.behavior.acceptance_criteria_prompts = 1;
        sample.behavior.file_scope_prompts = 1;
        sample.behavior.plan_events = 1;

        let features = derive_features(&[sample], 2, now, 90);

        assert!((features.requirement_clarity - 61.19).abs() < 0.02);
        assert!((features.automated_verification - 25.0).abs() < 0.01);
    }

    #[test]
    fn absent_behavior_is_neutral_not_opposite_evidence() {
        let now = DateTime::parse_from_rfc3339("2026-07-23T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut sample = record("2026-07-23");
        sample.files_touched = 0;
        sample.lines_changed = 0;
        sample.verification_events = 0;
        sample.has_commit = false;
        sample.test_events = 0;
        sample.build_events = 0;
        sample.read_events = 0;
        sample.search_events = 0;
        sample.edit_events = 0;
        sample.behavior = BehaviorSignals::default();

        let features = derive_features(&[sample], 2, now, 90);
        let candidates = type_scores(&features, true, false, 0.0, 0.0, 2);

        assert_eq!(features.requirement_clarity, 50.0);
        assert_eq!(features.automated_verification, 50.0);
        assert_eq!(features.local_fix, 50.0);
        assert_eq!(features.first_result_speed, 50.0);
        assert!(!features.prompt_signal);
        assert!(!features.modification_signal);
        assert!(
            candidates
                .iter()
                .filter(|candidate| {
                    matches!(candidate.guild, "start" | "quality" | "debug" | "delivery")
                })
                .all(|candidate| !candidate.eligible)
        );
    }

    #[test]
    fn badges_are_independent_instead_of_competing_for_two_slots() {
        let records = std::iter::repeat_with(|| record("2026-07-01"))
            .take(50)
            .collect::<Vec<_>>();
        let features = Features {
            diff_review: 92.0,
            automated_verification: 90.0,
            rollback_awareness: 88.0,
            first_result_speed: 90.0,
            tool_success: 92.0,
            completion: 91.0,
            cost_routing: 90.0,
            human_intervention: 80.0,
            night_rate: 50.0,
            long_session_count: 10,
            parallel_orchestration: 5.0,
            active_day_variation: 0.25,
            ..Features::default()
        };
        let behavior = BehaviorSummary {
            process_control_coverage: 1.0,
            orchestration_coverage: 1.0,
            lifecycle_coverage: 1.0,
            ..BehaviorSummary::default()
        };
        let earned = badges(&features, &behavior, &records, true, 50);
        let codes = earned
            .iter()
            .map(|badge| badge.code.as_str())
            .collect::<HashSet<_>>();

        assert_eq!(earned.len(), 9);
        assert_eq!(
            codes,
            HashSet::from([
                "GUARD", "TURBO", "LIVE", "BUDGET", "NIGHT", "MARATHON", "SOLO", "FINISH",
                "STEADY",
            ])
        );
    }

    #[test]
    fn synthetic_population_has_no_single_default_persona() {
        fn sample(seed: &mut u64) -> f64 {
            *seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((*seed >> 32) as u32 as f64 / u32::MAX as f64) * 100.0
        }

        fn draw(seed: &mut u64, centered: bool) -> f64 {
            if centered {
                (0..4).map(|_| sample(seed)).sum::<f64>() / 4.0
            } else {
                sample(seed)
            }
        }

        fn random_features(seed: &mut u64, centered: bool) -> Features {
            Features {
                requirement_clarity: draw(seed, centered),
                exploration: draw(seed, centered),
                scope_drift: draw(seed, centered),
                delegation: draw(seed, centered),
                human_intervention: draw(seed, centered),
                parallel_orchestration: draw(seed, centered),
                diff_review: draw(seed, centered),
                automated_verification: draw(seed, centered),
                rollback_awareness: draw(seed, centered),
                root_cause: draw(seed, centered),
                local_fix: draw(seed, centered),
                automation: draw(seed, centered),
                first_result_speed: draw(seed, centered),
                iteration_granularity: draw(seed, centered),
                shipping_tendency: draw(seed, centered),
                tool_switching: draw(seed, centered),
                cost_routing: draw(seed, centered),
                context_reuse: draw(seed, centered),
                polish: draw(seed, centered),
                infrastructure: draw(seed, centered),
                dependency_reuse: draw(seed, centered),
                burst: draw(seed, centered),
                completion: draw(seed, centered),
                prompt_signal: true,
                modification_signal: true,
                lifecycle_signal: true,
                first_tool_signal: true,
                token_signal: true,
                ..Features::default()
            }
        }

        fn audit_population(centered: bool) -> (usize, HashMap<&'static str, usize>) {
            let mut seed = if centered { 0xc3e7e_u64 } else { 0x5eed_u64 };
            let mut assigned = 0_usize;
            let mut counts = HashMap::<&'static str, usize>::new();
            for _ in 0..20_000 {
                let features = random_features(&mut seed, centered);
                let mut candidates =
                    rank_type_scores(type_scores(&features, true, true, 1.0, 1.0, 3));
                candidates.sort_by(|left, right| {
                    right
                        .score
                        .total_cmp(&left.score)
                        .then_with(|| left.code.cmp(right.code))
                });
                if let Some(winner) = candidates.first().filter(|candidate| {
                    candidate.raw_score >= 42.0 && candidate.distinctiveness >= 4.0
                }) {
                    assigned += 1;
                    *counts.entry(winner.code).or_default() += 1;
                }
            }
            (assigned, counts)
        }

        for centered in [false, true] {
            let (assigned, counts) = audit_population(centered);
            let represented = counts.len();
            let largest_share =
                counts.values().copied().max().unwrap_or_default() as f64 / assigned.max(1) as f64;
            println!(
                "VCTI synthetic audit: centered={centered} assigned={assigned} represented={represented} largest_share={:.1}%",
                largest_share * 100.0
            );
            assert!(
                assigned >= 17_000,
                "too many synthetic profiles were left unassigned: centered={centered}"
            );
            assert!(
                represented >= 20,
                "only {represented} of 24 personas appeared: centered={centered} {counts:?}"
            );
            assert!(
                largest_share <= 0.18,
                "one persona captured {:.1}% of profiles: centered={centered} {counts:?}",
                largest_share * 100.0
            );
        }
    }
}
