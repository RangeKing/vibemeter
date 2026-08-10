use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

pub const PARSER_VERSION: &str = "6.7.0";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum AgentKind {
    ClaudeCode,
    Codex,
    KimiCode,
    Cursor,
    OpenClaw,
    Hermes,
    ZCode,
}

impl AgentKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::KimiCode => "kimi-code",
            Self::Cursor => "cursor",
            Self::OpenClaw => "openclaw",
            Self::Hermes => "hermes",
            Self::ZCode => "zcode",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Provenance {
    Observed,
    Derived,
    Estimated,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub cache_write_1h_tokens: u64,
    pub reasoning_tokens: u64,
}

impl TokenUsage {
    pub fn total(&self) -> u64 {
        self.input_tokens
            .saturating_add(self.output_tokens)
            .saturating_add(self.cache_read_tokens)
            .saturating_add(self.cache_write_tokens)
            .saturating_add(self.cache_write_1h_tokens)
    }

    pub fn saturating_delta(&self, previous: &Self) -> Self {
        Self {
            input_tokens: self.input_tokens.saturating_sub(previous.input_tokens),
            output_tokens: self.output_tokens.saturating_sub(previous.output_tokens),
            cache_read_tokens: self
                .cache_read_tokens
                .saturating_sub(previous.cache_read_tokens),
            cache_write_tokens: self
                .cache_write_tokens
                .saturating_sub(previous.cache_write_tokens),
            cache_write_1h_tokens: self
                .cache_write_1h_tokens
                .saturating_sub(previous.cache_write_1h_tokens),
            reasoning_tokens: self
                .reasoning_tokens
                .saturating_sub(previous.reasoning_tokens),
        }
    }

    pub fn add_assign(&mut self, other: &Self) {
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
        self.cache_read_tokens = self
            .cache_read_tokens
            .saturating_add(other.cache_read_tokens);
        self.cache_write_tokens = self
            .cache_write_tokens
            .saturating_add(other.cache_write_tokens);
        self.cache_write_1h_tokens = self
            .cache_write_1h_tokens
            .saturating_add(other.cache_write_1h_tokens);
        self.reasoning_tokens = self.reasoning_tokens.saturating_add(other.reasoning_tokens);
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyAggregate {
    pub usage: TokenUsage,
    pub active_seconds: u64,
    pub events: u64,
    pub tool_calls: u64,
    pub errors: u64,
    pub verification_events: u64,
    pub estimated_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalEvent {
    pub sequence: u64,
    #[serde(default)]
    pub source_event_id: Option<String>,
    #[serde(default)]
    pub source_event_fingerprint: Option<String>,
    pub occurred_at: Option<String>,
    pub event_type: String,
    pub category: String,
    pub name: String,
    pub success: Option<bool>,
    pub duration_ms: Option<u64>,
    pub provenance: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PhraseAggregate {
    pub date: String,
    pub role: String,
    pub phrase: String,
    pub occurrences: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileChangeAccumulator {
    pub path: String,
    pub change_kind: String,
    pub lines_added: u64,
    pub lines_deleted: u64,
    pub modification_count: u64,
    pub first_observed_at: Option<String>,
    pub last_observed_at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BehaviorSignals {
    pub prompt_count: u64,
    pub prompt_characters: u64,
    pub structured_prompts: u64,
    pub acceptance_criteria_prompts: u64,
    pub file_scope_prompts: u64,
    pub goal_changes: u64,
    pub plan_events: u64,
    pub task_starts: u64,
    pub task_completions: u64,
    pub task_aborts: u64,
    pub completed_task_duration_ms: u64,
    pub time_to_first_tool_ms: Option<u64>,
    pub successful_tools: u64,
    pub failed_tools: u64,
    pub tool_duration_ms: u64,
    pub context_compactions: u64,
    pub rollbacks: u64,
    pub subagent_starts: u64,
    pub subagent_interactions: u64,
    pub subagent_interruptions: u64,
    pub parallel_batches: u64,
    pub deploy_events: u64,
    pub dependency_events: u64,
    pub preview_events: u64,
    pub document_events: u64,
    pub style_events: u64,
    pub test_file_events: u64,
    pub infrastructure_events: u64,
    pub automation_events: u64,
    pub instruction_file_events: u64,
    pub prompt_structure_enabled: bool,
    pub first_prompt_at: Option<String>,
    pub first_tool_at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GitFileStat {
    pub path: String,
    pub lines_added: u64,
    pub lines_deleted: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitEvidence {
    pub hash: String,
    pub subject: String,
    pub committed_at: String,
    pub files: Vec<GitFileStat>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GitEvidence {
    pub available: bool,
    pub state: String,
    pub branch: Option<String>,
    pub commits: Vec<GitCommitEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParseState {
    pub parser_version: String,
    pub agent: AgentKind,
    pub source_session_id: String,
    #[serde(default)]
    pub source_session_observed: bool,
    pub project_hash: Option<String>,
    #[serde(default)]
    pub project_label: Option<String>,
    #[serde(default)]
    pub prompt_excerpt: Option<String>,
    #[serde(default)]
    pub result_excerpt: Option<String>,
    #[serde(skip)]
    pub project_root: Option<PathBuf>,
    #[serde(skip)]
    pub ignore_patterns: Vec<String>,
    pub title: Option<String>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub last_timestamp: Option<String>,
    pub current_model: Option<String>,
    pub model_counts: HashMap<String, u64>,
    pub usage: TokenUsage,
    pub previous_codex_total: TokenUsage,
    pub last_claude_message_id: Option<String>,
    pub last_claude_message_usage: TokenUsage,
    pub seen_tool_ids: HashSet<String>,
    #[serde(skip)]
    pub source_record_ids: HashSet<String>,
    #[serde(skip)]
    pub new_source_record_ids: HashSet<String>,
    #[serde(skip)]
    pub replace_source_record_ids: bool,
    #[serde(skip)]
    pub source_record_receipt_key: [u8; 32],
    pub touched_file_hashes: HashSet<String>,
    pub tool_counts: HashMap<String, u64>,
    #[serde(default)]
    pub skill_counts: HashMap<String, u64>,
    #[serde(default)]
    pub events: Vec<CanonicalEvent>,
    #[serde(default)]
    pub phrase_counts: HashMap<String, PhraseAggregate>,
    #[serde(default)]
    pub last_phrase_fingerprints: HashMap<String, String>,
    #[serde(default)]
    pub file_changes: HashMap<String, FileChangeAccumulator>,
    pub daily: HashMap<String, DailyAggregate>,
    #[serde(default)]
    pub hourly: HashMap<String, TokenUsage>,
    pub event_count: u64,
    pub active_seconds: u64,
    pub longest_uninterrupted_seconds: u64,
    pub current_run_started_at: Option<String>,
    pub tool_calls: u64,
    pub lines_added: u64,
    pub lines_deleted: u64,
    #[serde(default)]
    pub codex_patch_result_events: u64,
    #[serde(default)]
    pub codex_requested_lines_added: u64,
    #[serde(default)]
    pub codex_requested_lines_deleted: u64,
    #[serde(default)]
    pub codex_requested_file_hashes: HashSet<String>,
    pub errors: u64,
    pub retries: u64,
    pub verification_events: u64,
    pub human_interventions: u64,
    pub subagent_count: u64,
    #[serde(default)]
    pub model_switches: u64,
    #[serde(default)]
    pub behavior: BehaviorSignals,
    pub malformed_records: u64,
    pub unknown_records: u64,
    pub cost_coverage_tokens: u64,
    pub estimated_cost_usd: f64,
    #[serde(skip)]
    pub git_evidence: Option<GitEvidence>,
}

impl ParseState {
    pub fn new(agent: AgentKind, fallback_session_id: String) -> Self {
        Self {
            parser_version: PARSER_VERSION.to_string(),
            agent,
            source_session_id: fallback_session_id,
            source_session_observed: false,
            project_hash: None,
            project_label: None,
            prompt_excerpt: None,
            result_excerpt: None,
            project_root: None,
            ignore_patterns: Vec::new(),
            title: None,
            started_at: None,
            ended_at: None,
            last_timestamp: None,
            current_model: None,
            model_counts: HashMap::new(),
            usage: TokenUsage::default(),
            previous_codex_total: TokenUsage::default(),
            last_claude_message_id: None,
            last_claude_message_usage: TokenUsage::default(),
            seen_tool_ids: HashSet::new(),
            source_record_ids: HashSet::new(),
            new_source_record_ids: HashSet::new(),
            replace_source_record_ids: false,
            source_record_receipt_key: [0; 32],
            touched_file_hashes: HashSet::new(),
            tool_counts: HashMap::new(),
            skill_counts: HashMap::new(),
            events: Vec::new(),
            phrase_counts: HashMap::new(),
            last_phrase_fingerprints: HashMap::new(),
            file_changes: HashMap::new(),
            daily: HashMap::new(),
            hourly: HashMap::new(),
            event_count: 0,
            active_seconds: 0,
            longest_uninterrupted_seconds: 0,
            current_run_started_at: None,
            tool_calls: 0,
            lines_added: 0,
            lines_deleted: 0,
            codex_patch_result_events: 0,
            codex_requested_lines_added: 0,
            codex_requested_lines_deleted: 0,
            codex_requested_file_hashes: HashSet::new(),
            errors: 0,
            retries: 0,
            verification_events: 0,
            human_interventions: 0,
            subagent_count: 0,
            model_switches: 0,
            behavior: BehaviorSignals {
                prompt_structure_enabled: true,
                ..BehaviorSignals::default()
            },
            malformed_records: 0,
            unknown_records: 0,
            cost_coverage_tokens: 0,
            estimated_cost_usd: 0.0,
            git_evidence: None,
        }
    }

    pub fn primary_model(&self) -> Option<String> {
        self.model_counts
            .iter()
            .max_by(|left, right| left.1.cmp(right.1).then_with(|| right.0.cmp(left.0)))
            .map(|(model, _)| model.clone())
            .or_else(|| self.current_model.clone())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexStatus {
    pub phase: String,
    pub running: bool,
    pub discovered_files: u64,
    pub processed_files: u64,
    pub indexed_sessions: u64,
    pub warning_count: u64,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub message_key: String,
}

impl Default for IndexStatus {
    fn default() -> Self {
        Self {
            phase: "idle".into(),
            running: false,
            discovered_files: 0,
            processed_files: 0,
            indexed_sessions: 0,
            warning_count: 0,
            started_at: None,
            finished_at: None,
            message_key: "index.idle".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverviewTotals {
    pub session_count: u64,
    pub active_seconds: u64,
    pub active_days: u64,
    pub usage: TokenUsage,
    pub estimated_cost_usd: Option<f64>,
    pub cost_coverage: f64,
    pub verification_rate: Option<f64>,
    pub longest_uninterrupted_seconds: u64,
    pub files_touched: u64,
    pub lines_added: u64,
    pub lines_deleted: u64,
    pub errors: u64,
    pub retries: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyUsagePoint {
    pub date: String,
    pub agent: String,
    pub model: String,
    pub usage: TokenUsage,
    pub active_seconds: u64,
    pub session_count: u64,
    pub tool_calls: u64,
    pub errors: u64,
    pub estimated_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HourlyUsagePoint {
    pub hour: String,
    pub agent: String,
    pub model: String,
    pub usage: TokenUsage,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DistributionItem {
    pub id: String,
    pub label: String,
    pub value: f64,
    pub secondary_value: Option<f64>,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: String,
    pub agent: String,
    pub model: Option<String>,
    pub title: String,
    pub project_label: String,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub active_seconds: u64,
    pub usage: TokenUsage,
    pub estimated_cost_usd: Option<f64>,
    pub cost_coverage: f64,
    pub tool_calls: u64,
    pub files_touched: u64,
    pub lines_added: u64,
    pub lines_deleted: u64,
    pub errors: u64,
    pub retries: u64,
    pub verification_state: String,
    pub longest_uninterrupted_seconds: u64,
    pub subagent_count: u64,
    pub has_commit: bool,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverageNotice {
    pub id: String,
    pub level: String,
    pub message_key: String,
    pub agent: Option<String>,
    pub value: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BehaviorSummary {
    pub sessions: u64,
    pub active_days: u64,
    pub prompt_count: u64,
    pub structured_prompt_rate: Option<f64>,
    pub acceptance_criteria_rate: Option<f64>,
    pub file_scope_rate: Option<f64>,
    pub task_starts: u64,
    pub task_completions: u64,
    pub task_aborts: u64,
    pub completion_rate: Option<f64>,
    pub average_task_duration_seconds: Option<f64>,
    pub successful_tools: u64,
    pub failed_tools: u64,
    pub tool_success_rate: Option<f64>,
    pub tool_duration_seconds: u64,
    pub plan_events: u64,
    pub goal_changes: u64,
    pub context_compactions: u64,
    pub rollbacks: u64,
    pub subagent_starts: u64,
    pub subagent_interactions: u64,
    pub subagent_interruptions: u64,
    pub parallel_batches: u64,
    pub deploy_events: u64,
    pub document_events: u64,
    pub style_events: u64,
    pub infrastructure_events: u64,
    pub automation_events: u64,
    pub structure_capable_sessions: u64,
    pub lifecycle_capable_sessions: u64,
    pub tool_result_capable_sessions: u64,
    pub orchestration_capable_sessions: u64,
    pub process_control_capable_sessions: u64,
    pub structure_coverage: f64,
    pub lifecycle_coverage: f64,
    pub tool_result_coverage: f64,
    pub orchestration_coverage: f64,
    pub process_control_coverage: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverviewResponse {
    pub range: String,
    pub generated_at: String,
    pub pricing_version: String,
    pub totals: OverviewTotals,
    pub daily: Vec<DailyUsagePoint>,
    pub hourly: Vec<HourlyUsagePoint>,
    pub agents: Vec<DistributionItem>,
    pub models: Vec<DistributionItem>,
    pub tools: Vec<DistributionItem>,
    pub skills: SkillUsageSummary,
    pub behavior: BehaviorSummary,
    pub recent_sessions: Vec<SessionSummary>,
    pub coverage: Vec<CoverageNotice>,
    pub index_status: IndexStatus,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillUsageItem {
    pub name: String,
    pub invocation_count: u64,
    pub session_count: u64,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillUsageSummary {
    pub most_used: Vec<SkillUsageItem>,
    pub least_used: Vec<SkillUsageItem>,
    pub installed_without_usage: Vec<String>,
    pub installed_count: u64,
    pub used_count: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhraseAgentCount {
    pub agent: String,
    pub occurrences: u64,
    pub session_count: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhraseModelCount {
    pub model: String,
    pub occurrences: u64,
    pub session_count: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhraseCloudItem {
    pub phrase: String,
    pub occurrences: u64,
    pub session_count: u64,
    pub weight: f64,
    pub dominant_agent: Option<String>,
    pub dominant_model: Option<String>,
    pub agents: Vec<PhraseAgentCount>,
    pub models: Vec<PhraseModelCount>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhraseCloud {
    pub status: String,
    pub sample_sessions: u64,
    pub items: Vec<PhraseCloudItem>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhraseLegendItem {
    pub agent: String,
    pub occurrences: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhraseCloudResponse {
    pub range: String,
    pub generated_at: String,
    pub user: PhraseCloud,
    pub agents: PhraseCloud,
    pub legend: Vec<PhraseLegendItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LiveAction {
    pub kind: String,
    pub label: String,
    pub occurred_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LiveJumpContext {
    pub tty: Option<String>,
    pub terminal_kind: Option<String>,
    pub host_app_name: Option<String>,
    pub process_started_at: Option<String>,
    pub tmux_socket: Option<String>,
    pub tmux_pane: Option<String>,
    pub tmux_executable: Option<String>,
    pub cmux_socket: Option<String>,
    pub cmux_workspace_id: Option<String>,
    pub cmux_surface_id: Option<String>,
    pub cmux_executable: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkPulseDimension {
    pub availability: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub evidence_level: String,
    pub source_coverage: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub age_seconds: Option<u64>,
}

impl Default for WorkPulseDimension {
    fn default() -> Self {
        Self {
            availability: "not-recorded".into(),
            value: None,
            evidence_level: "not-recorded".into(),
            source_coverage: "unknown".into(),
            age_seconds: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkPulse {
    pub lifecycle: WorkPulseDimension,
    pub work_phase: WorkPulseDimension,
    pub attention_signal: WorkPulseDimension,
    pub freshness: WorkPulseDimension,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LiveSession {
    pub id: String,
    pub source_session_id: String,
    pub agent: String,
    pub project_label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_title: Option<String>,
    pub status: String,
    pub phase: String,
    pub started_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity_ended_at: Option<String>,
    #[serde(default, skip_serializing)]
    pub event_order_key: String,
    pub waiting_reason: Option<String>,
    pub actions: Vec<LiveAction>,
    pub process_id: Option<u32>,
    pub origin: Option<String>,
    #[serde(default)]
    pub pulse: WorkPulse,
    #[serde(default, skip_serializing)]
    pub jump_context: Option<LiveJumpContext>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NotchCompletedSession {
    pub session: LiveSession,
    pub cycle_started_at: String,
    pub completed_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotchClearResult {
    pub token: String,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticRetentionStatus {
    pub state: String,
    pub enabled: bool,
    pub started_at: Option<String>,
    pub expires_at: Option<String>,
    pub storage_location: String,
    pub retained_envelopes: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticClearResult {
    pub removed: u64,
    pub status: DiagnosticRetentionStatus,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookProviderStatus {
    pub provider: String,
    pub available: bool,
    pub installed: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookStatus {
    pub state: String,
    pub providers: Vec<HookProviderStatus>,
    pub socket_ready: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveSnapshot {
    pub generated_at: String,
    pub sessions: Vec<LiveSession>,
    pub completed_sessions: Vec<NotchCompletedSession>,
    pub urgent_session_id: Option<String>,
    pub active_count: u64,
    pub hook_status: HookStatus,
}

#[derive(Debug, Clone)]
pub struct ObservedLiveEvent {
    pub occurred_at: String,
    pub observed_at: String,
    pub agent: String,
    pub source_session_id: String,
    pub source_event_id: Option<String>,
    pub source_sequence: Option<i64>,
    pub source_event_fingerprint: Option<String>,
    pub event_name: String,
    pub project_label: String,
    pub payload_json: String,
    pub status: String,
    pub phase: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveTimelinePoint {
    pub id: String,
    pub occurred_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<String>,
    pub agent: String,
    pub project_label: String,
    pub event_name: String,
    pub status: String,
    pub source_session_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveHistoryItem {
    pub id: String,
    pub occurred_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<String>,
    pub agent: String,
    pub project_label: String,
    pub status: String,
    pub event_name: String,
    pub source_session_id: String,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveConcurrencyLane {
    pub agent: String,
    pub session_count: u64,
    pub waiting_count: u64,
    pub error_count: u64,
    pub running_count: u64,
    pub completed_count: u64,
    pub projects: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveActivityResponse {
    pub generated_at: String,
    pub period_start: String,
    pub timeline: Vec<LiveTimelinePoint>,
    pub history: Vec<LiveHistoryItem>,
    pub concurrency: Vec<LiveConcurrencyLane>,
    pub attention: Vec<AttentionEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AttentionEvent {
    pub id: String,
    pub kind: String,
    pub state: String,
    pub reason_key: String,
    pub agent: String,
    pub source_session_id: String,
    pub project_label: String,
    pub opened_at: String,
    pub latest_evidence_at: String,
    pub expires_at: String,
    pub resolved_at: Option<String>,
    pub evidence_level: String,
    pub source_coverage: String,
    pub rule_version: String,
    pub evidence_count: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionsResponse {
    pub items: Vec<SessionSummary>,
    pub total: u64,
    pub page: u64,
    pub page_size: u64,
    pub models: Vec<String>,
    pub projects: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SessionListFilters<'a> {
    pub agent: Option<&'a str>,
    pub search: Option<&'a str>,
    pub model: Option<&'a str>,
    pub project: Option<&'a str>,
    pub verification_state: Option<&'a str>,
    pub attention_only: bool,
    pub code_only: bool,
    pub commit_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceReference {
    pub kind: String,
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSummary {
    pub id: String,
    pub title: String,
    pub project_label: String,
    pub status: String,
    pub confidence: f64,
    pub grouping_state: String,
    pub grouping_reason_keys: Vec<String>,
    pub suggested_task_id: Option<String>,
    pub session_count: u64,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub agent: String,
    pub model: Option<String>,
    pub files_changed: u64,
    pub lines_added: u64,
    pub lines_deleted: u64,
    pub total_tokens: u64,
    pub has_commit: bool,
    pub verification_state: String,
    pub worth_reviewing: bool,
    pub review_reason_keys: Vec<String>,
    pub primary_session_id: Option<String>,
    pub source_excluded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessPhase {
    pub id: String,
    pub phase_key: String,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub event_count: u64,
    pub provenance: String,
    pub events: Vec<CanonicalEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    pub id: String,
    pub path: String,
    pub change_kind: String,
    pub lines_added: u64,
    pub lines_deleted: u64,
    pub modification_count: u64,
    pub final_state: String,
    pub provenance: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComparisonItem {
    pub id: String,
    pub group_kind: String,
    pub agent: String,
    pub model: Option<String>,
    pub label: String,
    pub session_count: u64,
    pub usage: TokenUsage,
    pub active_seconds: u64,
    pub files_touched: u64,
    pub lines_added: u64,
    pub lines_deleted: u64,
    pub estimated_cost_usd: Option<f64>,
    pub cost_coverage: f64,
    pub usage_share: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDetail {
    #[serde(flatten)]
    pub summary: SessionSummary,
    pub tools: Vec<DistributionItem>,
    pub daily: Vec<DailyUsagePoint>,
    pub warnings: Vec<CoverageNotice>,
    pub task: Option<TaskSummary>,
    pub phases: Vec<ProcessPhase>,
    pub file_changes: Vec<FileChange>,
    pub git_evidence: GitEvidence,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InsightItem {
    pub id: String,
    pub tier: String,
    pub title_key: String,
    pub detail_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_session_id: Option<String>,
    pub sample_size: u64,
    pub trend: Option<f64>,
    pub evidence: Vec<EvidenceReference>,
    pub promotable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InsightStat {
    pub id: String,
    pub label_key: String,
    pub value: f64,
    pub format: String,
    pub text_value: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InsightsResponse {
    pub items: Vec<InsightItem>,
    pub comparison: Vec<ComparisonItem>,
    pub minimum_sample_size: u64,
    pub sample_size: u64,
    pub stats: Vec<InsightStat>,
    pub behavior: BehaviorSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VctiScore {
    pub id: String,
    pub value: f64,
    pub coverage: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VctiEvidenceItem {
    pub id: String,
    pub label_key: String,
    pub value: f64,
    pub format: String,
    pub provenance: String,
    pub structural: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VctiBadge {
    pub code: String,
    pub label_key: String,
    pub description_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VctiTrendPoint {
    pub period_start: String,
    pub scores: Vec<VctiScore>,
    pub dominant_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VctiProfile {
    pub status: String,
    pub algorithm_version: String,
    pub period_start: String,
    pub period_end: String,
    pub window_days: u64,
    pub temporary: bool,
    pub session_count: u64,
    pub active_days: u64,
    pub primary_type: Option<String>,
    pub secondary_type: Option<String>,
    pub guild: Option<String>,
    pub confidence: f64,
    pub confidence_label: String,
    pub type_margin: f64,
    pub scores: Vec<VctiScore>,
    pub dimensions: Vec<VctiScore>,
    pub badges: Vec<VctiBadge>,
    pub evidence: Vec<VctiEvidenceItem>,
    pub trend: Vec<VctiTrendPoint>,
    pub behavior: BehaviorSummary,
    pub missing_capabilities: Vec<String>,
    pub structure_analysis_enabled: bool,
    pub git_evidence_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybookItem {
    pub id: String,
    pub title: String,
    pub body: String,
    pub category: String,
    pub project_label: Option<String>,
    pub task_type: Option<String>,
    pub source_review_id: Option<String>,
    pub source_finding_id: Option<String>,
    pub source_excluded: bool,
    pub applied: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavePlaybookRequest {
    pub id: Option<String>,
    pub title: String,
    pub body: String,
    pub category: String,
    pub project_label: Option<String>,
    pub task_type: Option<String>,
    pub source_review_id: Option<String>,
    pub source_finding_id: Option<String>,
    pub applied: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectControl {
    pub project_hash: String,
    pub project_label: String,
    pub session_count: u64,
    pub excluded: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceStatus {
    pub agent: String,
    pub available: bool,
    pub selected: bool,
    pub capability_level: String,
    pub live_capability: String,
    pub parser_version: String,
    pub session_count: u64,
    pub last_indexed_at: Option<String>,
    pub status: String,
    pub warning_count: u64,
    pub path_label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateWindow {
    pub id: String,
    pub label: String,
    pub used_percent: Option<f64>,
    pub reset_at: Option<String>,
    pub reset_description: Option<String>,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderHealth {
    pub state: String,
    pub description: String,
    pub checked_at: Option<String>,
    pub status_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDailyAccountUsage {
    pub date: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub api_cost_usd: Option<f64>,
    pub metered_cost_usd: Option<f64>,
    pub request_count: u64,
    pub token_request_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderAccountUsage {
    pub period_start: String,
    pub period_end: String,
    pub fetched_at: String,
    pub scope: String,
    pub daily: Vec<ProviderDailyAccountUsage>,
    #[serde(default, skip_serializing)]
    pub account_fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUsage {
    pub provider: String,
    pub available: bool,
    pub source: String,
    pub windows: Vec<RateWindow>,
    pub credits: Option<f64>,
    pub account_usage: Option<ProviderAccountUsage>,
    pub health: ProviderHealth,
    pub refreshed_at: Option<String>,
    pub stale: bool,
    pub error_key: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuBarSnapshot {
    pub generated_at: String,
    pub range: String,
    pub usage: TokenUsage,
    pub cost_usd: Option<f64>,
    pub heatmap: Vec<DailyUsagePoint>,
    pub hourly: Vec<HourlyUsagePoint>,
    pub providers: Vec<ProviderUsage>,
    pub index_status: IndexStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareMetricInput {
    pub id: String,
    pub visible: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareRenderRequest {
    pub template_id: String,
    pub locale: String,
    pub aspect_ratio: String,
    pub theme: String,
    pub range: String,
    pub session_id: Option<String>,
    pub compare_ids: Vec<String>,
    pub title: String,
    pub summary: String,
    pub project_name: String,
    pub metrics: Vec<ShareMetricInput>,
    pub show_brand: bool,
    pub show_model: bool,
    pub show_cost: bool,
    pub show_project: bool,
    #[serde(default)]
    pub show_behavior_evidence: bool,
    pub privacy_reviewed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareGuardFinding {
    pub id: String,
    pub level: String,
    pub message_key: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharePreview {
    pub svg: String,
    pub width: u32,
    pub height: u32,
    pub findings: Vec<ShareGuardFinding>,
    pub can_export: bool,
    pub model_hash: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportRequest {
    #[serde(flatten)]
    pub render: ShareRenderRequest,
    pub format: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub path: String,
    pub format: String,
    pub width: u32,
    pub height: u32,
    pub bytes_written: u64,
    pub model_hash: String,
}
