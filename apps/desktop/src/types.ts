export type Locale = "en-US" | "zh-CN";
export type Theme = "light" | "dark" | "system";
export type RangeKey = "today" | "7d" | "30d" | "90d" | "180d" | "year" | "all";
export type PageKey = "live" | "data" | "sessions" | "insights" | "vcti" | "share" | "sources" | "settings";
export type ShareTemplate =
  | "usage-overview"
  | "developer-wrapped"
  | "agent-comparison"
  | "session-recap"
  | "vcti-card"
  | "catchphrases";
export type AspectRatio = "1:1" | "2:3" | "3:2" | "3:4" | "4:3" | "4:5" | "16:9" | "9:16";

export interface TokenUsage {
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
  cacheWrite1hTokens: number;
  reasoningTokens: number;
}

export interface IndexStatus {
  phase: string;
  running: boolean;
  discoveredFiles: number;
  processedFiles: number;
  indexedSessions: number;
  warningCount: number;
  startedAt?: string;
  finishedAt?: string;
  messageKey: string;
}

export interface OverviewTotals {
  sessionCount: number;
  activeSeconds: number;
  activeDays: number;
  usage: TokenUsage;
  estimatedCostUsd?: number;
  costCoverage: number;
  verificationRate?: number;
  longestUninterruptedSeconds: number;
  filesTouched: number;
  linesAdded: number;
  linesDeleted: number;
  errors: number;
  retries: number;
}

export interface DailyUsagePoint {
  date: string;
  agent: string;
  model: string;
  usage: TokenUsage;
  activeSeconds: number;
  sessionCount: number;
  toolCalls: number;
  errors: number;
  estimatedCostUsd?: number;
}

export interface HourlyUsagePoint {
  hour: string;
  agent: string;
  model: string;
  usage: TokenUsage;
}

export interface DistributionItem {
  id: string;
  label: string;
  value: number;
  secondaryValue?: number;
  provenance: "observed" | "derived" | "estimated";
}

export interface CoverageNotice {
  id: string;
  level: string;
  messageKey: string;
  agent?: string;
  value?: number;
}

export interface SessionSummary {
  id: string;
  agent: string;
  model?: string;
  title: string;
  projectLabel: string;
  startedAt?: string | null;
  endedAt?: string;
  activeSeconds: number;
  usage: TokenUsage;
  estimatedCostUsd?: number;
  costCoverage: number;
  toolCalls: number;
  filesTouched: number;
  linesAdded: number;
  linesDeleted: number;
  errors: number;
  retries: number;
  verificationState: string;
  longestUninterruptedSeconds: number;
  subagentCount: number;
  hasCommit: boolean;
  provenance: string;
}

export interface BehaviorSummary {
  sessions: number;
  activeDays: number;
  promptCount: number;
  structuredPromptRate?: number;
  acceptanceCriteriaRate?: number;
  fileScopeRate?: number;
  taskStarts: number;
  taskCompletions: number;
  taskAborts: number;
  completionRate?: number;
  averageTaskDurationSeconds?: number;
  successfulTools: number;
  failedTools: number;
  toolSuccessRate?: number;
  toolDurationSeconds: number;
  planEvents: number;
  goalChanges: number;
  contextCompactions: number;
  rollbacks: number;
  subagentStarts: number;
  subagentInteractions: number;
  subagentInterruptions: number;
  parallelBatches: number;
  deployEvents: number;
  documentEvents: number;
  styleEvents: number;
  infrastructureEvents: number;
  automationEvents: number;
  structureCapableSessions: number;
  lifecycleCapableSessions: number;
  toolResultCapableSessions: number;
  orchestrationCapableSessions: number;
  processControlCapableSessions: number;
  structureCoverage: number;
  lifecycleCoverage: number;
  toolResultCoverage: number;
  orchestrationCoverage: number;
  processControlCoverage: number;
}

export interface OverviewResponse {
  range: string;
  generatedAt: string;
  pricingVersion: string;
  totals: OverviewTotals;
  daily: DailyUsagePoint[];
  hourly: HourlyUsagePoint[];
  agents: DistributionItem[];
  models: DistributionItem[];
  tools: DistributionItem[];
  skills: SkillUsageSummary;
  behavior: BehaviorSummary;
  recentSessions: SessionSummary[];
  coverage: CoverageNotice[];
  indexStatus: IndexStatus;
}

export interface SkillUsageItem {
  name: string;
  invocationCount: number;
  sessionCount: number;
}

export interface SkillUsageSummary {
  mostUsed: SkillUsageItem[];
  leastUsed: SkillUsageItem[];
  installedWithoutUsage: string[];
  installedCount: number;
  usedCount: number;
}

export interface PhraseAgentCount {
  agent: string;
  occurrences: number;
  sessionCount: number;
}

export interface PhraseModelCount {
  model: string;
  occurrences: number;
  sessionCount: number;
}

export interface PhraseCloudItem {
  phrase: string;
  occurrences: number;
  sessionCount: number;
  weight: number;
  dominantAgent?: string;
  dominantModel?: string;
  agents: PhraseAgentCount[];
  models: PhraseModelCount[];
}

export interface PhraseCloud {
  status: "ready" | "insufficient-data";
  sampleSessions: number;
  items: PhraseCloudItem[];
}

export interface PhraseLegendItem {
  agent: string;
  occurrences: number;
}

export interface PhraseCloudResponse {
  range: RangeKey;
  generatedAt: string;
  user: PhraseCloud;
  agents: PhraseCloud;
  legend: PhraseLegendItem[];
}

export interface LiveAction {
  kind: string;
  label: string;
  occurredAt: string;
}

export interface WorkPulseDimension {
  availability: "available" | "unknown" | "not-recorded";
  value?: string;
  evidenceLevel: "observed" | "derived" | "estimated" | "not-recorded" | string;
  sourceCoverage: "exact" | "experimental" | "none" | "unknown" | string;
  ageSeconds?: number;
}

export interface WorkPulse {
  lifecycle: WorkPulseDimension;
  workPhase: WorkPulseDimension;
  attentionSignal: WorkPulseDimension;
  freshness: WorkPulseDimension;
}

export interface LiveSession {
  id: string;
  sourceSessionId: string;
  agent: "claude-code" | "codex" | "kimi-code" | "zcode";
  projectLabel: string;
  conversationTitle?: string;
  status: "waiting" | "error" | "running" | "paused" | "idle" | "completed";
  phase: string;
  startedAt: string;
  updatedAt: string;
  activityEndedAt?: string;
  waitingReason?: string;
  actions: LiveAction[];
  processId?: number;
  origin?: "cli" | "desktop";
  pulse: WorkPulse;
}

export interface NotchCompletedSession {
  session: LiveSession;
  cycleStartedAt: string;
  completedAt: string;
}

export interface NotchClearResult {
  token: string;
  count: number;
}

export interface HookProviderStatus {
  provider: string;
  available: boolean;
  installed: boolean;
  detail: "ready" | "not-found" | "hook-missing" | "feature-disabled" | string;
}

export interface HookStatus {
  state: "ready" | "partial" | "unavailable";
  providers: HookProviderStatus[];
  socketReady: boolean;
}

export interface LiveSnapshot {
  generatedAt: string;
  sessions: LiveSession[];
  completedSessions: NotchCompletedSession[];
  urgentSessionId?: string;
  attentionAvailable: boolean;
  attentionQueue: AttentionEvent[];
  activeCount: number;
  hookStatus: HookStatus;
}

export interface LiveTimelinePoint {
  id: string;
  occurredAt: string;
  observedAt?: string;
  agent: string;
  projectLabel: string;
  eventName: string;
  status: string;
  sourceSessionId: string;
}

export interface LiveHistoryItem {
  id: string;
  occurredAt: string;
  observedAt?: string;
  agent: string;
  projectLabel: string;
  status: "waiting" | "error" | string;
  eventName: string;
  sourceSessionId: string;
  sessionId?: string;
}

export interface LiveConcurrencyLane {
  agent: string;
  sessionCount: number;
  waitingCount: number;
  errorCount: number;
  runningCount: number;
  completedCount: number;
  projects: string[];
}

export interface LiveActivityResponse {
  generatedAt: string;
  periodStart: string;
  timeline: LiveTimelinePoint[];
  history: LiveHistoryItem[];
  concurrency: LiveConcurrencyLane[];
  attention: AttentionEvent[];
}

export interface AttentionEvent {
  id: string;
  kind: "waiting" | "error" | "stuck" | "completion-review";
  state: "open" | "acknowledged" | "snoozed" | "resolved" | "ignored" | "expired";
  reasonKey: string;
  agent: string;
  sourceSessionId: string;
  projectLabel: string;
  conversationTitle?: string;
  openedAt: string;
  latestEvidenceAt: string;
  expiresAt: string;
  resolvedAt?: string;
  evidenceLevel: string;
  sourceCoverage: string;
  ruleVersion: string;
  evidenceCount: number;
  interventionCount: number;
}

export interface AttentionQualityReport {
  reviewedSamples: number;
  stuckPrecision: number | null;
  feedbackSamples: number;
  falsePositiveRate: number | null;
  notificationSamples: number;
  notificationP95Seconds: number | null;
  jumpAttempts: number;
  jumpSuccessRate: number | null;
  realAppVerified: boolean;
  requiredSamples: number;
  requiredPrecision: number;
  maximumFalsePositiveRate: number;
  maximumNotificationP95Seconds: number;
  requiredJumpSuccessRate: number;
  passed: boolean;
}

export interface NotchUiState {
  available: boolean;
  enabled: boolean;
  expanded: boolean;
  pinned: boolean;
  hasActivity: boolean;
  hardwareWidth: number;
  hardwareHeight: number;
  leftWingWidth: number;
  rightWingWidth: number;
  expandedHeight: number;
}

export interface SessionsResponse {
  items: SessionSummary[];
  total: number;
  page: number;
  pageSize: number;
  models: string[];
  projects: string[];
}

export interface SessionListQuery {
  range: string;
  agent?: string;
  search?: string;
  model?: string;
  project?: string;
  verificationState?: string;
  attentionOnly?: boolean;
  codeOnly?: boolean;
  commitOnly?: boolean;
  page?: number;
  pageSize?: number;
}

export interface EvidenceReference {
  kind: string;
  id: string;
  label: string;
}

export interface TaskSummary {
  id: string;
  title: string;
  projectLabel: string;
  status: string;
  confidence: number;
  groupingState: "auto" | "suggested" | "separate" | "manual";
  groupingReasonKeys: string[];
  suggestedTaskId?: string;
  sessionCount: number;
  startedAt: string;
  endedAt?: string;
  agent: string;
  model?: string;
  filesChanged: number;
  linesAdded: number;
  linesDeleted: number;
  totalTokens: number;
  hasCommit: boolean;
  verificationState: string;
  worthReviewing: boolean;
  reviewReasonKeys: string[];
  primarySessionId?: string;
  attentionCount: number;
  sourceExcluded: boolean;
}

export interface CanonicalEvent {
  sequence: number;
  occurredAt?: string;
  eventType: string;
  category: string;
  name: string;
  success?: boolean;
  durationMs?: number;
  provenance: string;
}

export interface ProcessPhase {
  id: string;
  phaseKey: string;
  startedAt?: string;
  endedAt?: string;
  eventCount: number;
  provenance: string;
  events: CanonicalEvent[];
}

export interface FileChange {
  id: string;
  path: string;
  changeKind: string;
  linesAdded: number;
  linesDeleted: number;
  modificationCount: number;
  finalState: string;
  provenance: string;
}

export interface GitCommitEvidence {
  hash: string;
  subject: string;
  committedAt: string;
  files: Array<{ path: string; linesAdded: number; linesDeleted: number }>;
}

export interface GitEvidence {
  available: boolean;
  state: string;
  branch?: string;
  commits: GitCommitEvidence[];
}

export interface SessionDetail extends SessionSummary {
  tools: DistributionItem[];
  daily: DailyUsagePoint[];
  warnings: CoverageNotice[];
  task?: TaskSummary;
  phases: ProcessPhase[];
  fileChanges: FileChange[];
  gitEvidence: GitEvidence;
  capabilities: string[];
  attention: AttentionEvent[];
}

export interface ComparisonItem {
  id: string;
  groupKind: "agent" | "model";
  agent: string;
  model?: string;
  label: string;
  sessionCount: number;
  usage: TokenUsage;
  activeSeconds: number;
  filesTouched: number;
  linesAdded: number;
  linesDeleted: number;
  estimatedCostUsd?: number;
  costCoverage: number;
  usageShare: number;
}

export interface InsightItem {
  id: string;
  tier: string;
  titleKey: string;
  detailKey: string;
  value?: number;
  targetSessionId?: string;
  sampleSize: number;
  trend?: number;
  evidence: EvidenceReference[];
  promotable: boolean;
}

export interface InsightStat {
  id: string;
  labelKey: string;
  value: number;
  format: "number" | "percent" | "duration" | "text" | string;
  textValue?: string;
}

export interface InsightsResponse {
  items: InsightItem[];
  comparison: ComparisonItem[];
  minimumSampleSize: number;
  sampleSize: number;
  stats: InsightStat[];
  behavior: BehaviorSummary;
}

export interface VctiScore {
  id: string;
  value: number;
  coverage: number;
}

export interface VctiEvidenceItem {
  id: string;
  labelKey: string;
  value: number;
  format: string;
  provenance: string;
  structural: boolean;
}

export interface VctiBadge {
  code: string;
  labelKey: string;
  descriptionKey: string;
}

export interface VctiTrendPoint {
  periodStart: string;
  scores: VctiScore[];
  dominantType?: string;
}

export interface VctiOptionalMetric {
  value?: number;
  available: boolean;
}

export interface VctiRhythmPeriod {
  id: "night" | "morning" | "afternoon" | "evening";
  sessions: number;
  share: number;
}

export interface VctiWorkRhythm {
  workPeriods: VctiRhythmPeriod[];
  workPeriodsAvailable: boolean;
  activeDays: VctiOptionalMetric;
  sessionsPerDay: VctiOptionalMetric;
}

export interface VctiCollaboration {
  subagentStarts: VctiOptionalMetric;
  parallelBatches: VctiOptionalMetric;
}

export interface VctiDetailDiversity {
  toolCategories: VctiOptionalMetric;
  explicitSkills: VctiOptionalMetric;
}

export interface VctiProcessVariation {
  errors: VctiOptionalMetric;
  retries: VctiOptionalMetric;
  rollbacks: VctiOptionalMetric;
}

export interface VctiIdentityEvidence {
  rhythm: VctiWorkRhythm;
  collaboration: VctiCollaboration;
  detailDiversity: VctiDetailDiversity;
  processVariation: VctiProcessVariation;
}

export interface VctiVisualInput {
  id: string;
  available: boolean;
}

export interface VctiVisualPath {
  d: string;
  strokeWidth: number;
  opacity: number;
}

export interface VctiIdentityVisual {
  algorithmVersion: string;
  version: string;
  range: string;
  available: boolean;
  inputs: VctiVisualInput[];
  contours: VctiVisualPath[];
  rhythm: {
    available: boolean;
    phase?: number;
    activeIntensity?: number;
    sessionIntensity?: number;
    density?: number;
    paths: VctiVisualPath[];
  };
}

export interface VctiProfile {
  status: "collecting" | "preview" | "stable" | "high-confidence";
  algorithmVersion: string;
  periodStart: string;
  periodEnd: string;
  windowDays: number;
  temporary: boolean;
  sessionCount: number;
  activeDays: number;
  primaryType?: string;
  secondaryType?: string;
  guild?: string;
  confidence: number;
  confidenceLabel: string;
  typeMargin: number;
  scores: VctiScore[];
  dimensions: VctiScore[];
  badges: VctiBadge[];
  evidence: VctiEvidenceItem[];
  trend: VctiTrendPoint[];
  identityEvidence: VctiIdentityEvidence;
  identityVisual: VctiIdentityVisual;
  behavior: BehaviorSummary;
  missingCapabilities: string[];
  structureAnalysisEnabled: boolean;
  gitEvidenceEnabled: boolean;
}

export interface ProjectControl {
  projectHash: string;
  projectLabel: string;
  sessionCount: number;
  excluded: boolean;
}

export interface SourceStatus {
  agent: string;
  available: boolean;
  selected: boolean;
  capabilityLevel: string;
  liveCapability: SourceLiveCapability;
  parserVersion: string;
  sessionCount: number;
  lastIndexedAt?: string;
  status: string;
  warningCount: number;
  pathLabel: string;
}

export type SourceLiveCapability = "exact" | "experimental" | "none";

export interface RateWindow {
  id: string;
  label: string;
  usedPercent?: number;
  resetAt?: string;
  resetDescription?: string;
  provenance: string;
}

export interface ProviderDailyAccountUsage {
  date: string;
  model: string;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
  apiCostUsd?: number | null;
  meteredCostUsd?: number | null;
  requestCount: number;
  tokenRequestCount: number;
}

export interface ProviderAccountUsage {
  periodStart: string;
  periodEnd: string;
  fetchedAt: string;
  scope: "account";
  daily: ProviderDailyAccountUsage[];
}

export interface ProviderUsage {
  provider: string;
  available: boolean;
  source: string;
  windows: RateWindow[];
  credits?: number;
  accountUsage?: ProviderAccountUsage | null;
  health: { state: string; description: string; checkedAt?: string; statusUrl: string };
  refreshedAt?: string;
  stale: boolean;
  errorKey?: string;
}

export interface MenuBarSnapshot {
  generatedAt: string;
  range: RangeKey;
  usage: TokenUsage;
  costUsd?: number;
  heatmap: DailyUsagePoint[];
  hourly: HourlyUsagePoint[];
  providers: ProviderUsage[];
  indexStatus: IndexStatus;
}

export interface ShareMetricInput { id: string; visible: boolean }

export interface ShareRenderRequest {
  templateId: ShareTemplate;
  locale: Locale;
  aspectRatio: AspectRatio;
  theme: "light" | "dark";
  range: RangeKey;
  sessionId?: string;
  compareIds: string[];
  title: string;
  summary: string;
  projectName: string;
  metrics: ShareMetricInput[];
  showBrand: boolean;
  showModel: boolean;
  showCost: boolean;
  showProject: boolean;
  showBehaviorEvidence: boolean;
  privacyReviewed: boolean;
}

export interface SharePreview {
  svg: string;
  width: number;
  height: number;
  findings: Array<{ id: string; level: "safe" | "review" | "block"; messageKey: string }>;
  canExport: boolean;
  modelHash: string;
}

export interface ExportResult {
  path: string;
  format: string;
  width: number;
  height: number;
  bytesWritten: number;
  modelHash: string;
}

export interface AppSettings {
  locale: string;
  theme: string;
  onboardingComplete: string;
  iaMigrationTipSeen: string;
  credentialsAllowed: string;
  cursorDashboardUsage: string;
  launchAtLogin: string;
  gitReadAllowed: string;
  vctiPromptStructure: string;
  retentionDays: string;
  liveHooksEnabled: string;
  notchEnabled: string;
  menuBarEnabled: string;
}

export interface DiagnosticRetentionStatus {
  state: "disabled" | "active" | "expired" | "unavailable" | string;
  enabled: boolean;
  startedAt?: string;
  expiresAt?: string;
  storageLocation: string;
  retainedEnvelopes: number;
}

export interface DiagnosticClearResult {
  removed: number;
  status: DiagnosticRetentionStatus;
}
