import { invoke } from "@tauri-apps/api/core";
import type {
  ApiAccountInfo,
  AppSettings,
  EdgeState,
  AttentionEvent,
  AttentionQualityReport,
  ComparisonItem,
  DiagnosticClearResult,
  DiagnosticRetentionStatus,
  DelegationTraceResponse,
  ExportResult,
  InsightsResponse,
  IndexStatus,
  MenuBarSnapshot,
  LiveActivityResponse,
  LiveSnapshot,
  MemoryLedgerResponse,
  NotchClearResult,
  NotchUiState,
  HookStatus,
  OverviewResponse,
  PhraseCloudResponse,
  ProjectControl,
  ProjectSummary,
  ProviderUsage,
  SessionDetail,
  SessionListQuery,
  SessionsResponse,
  SharePreview,
  ShareRenderRequest,
  SessionContext,
  SourceStatus,
  TaskSummary,
  VctiProfile,
} from "../types";

export const api = {
  edgeState: () => invoke<EdgeState>("get_edge_state"),
  setEdgePlacement: (
    drag?: { startOffset: number; deltaY: number },
    notchHeight?: number,
    cardHeight?: number,
  ) =>
    invoke<void>("set_edge_placement", {
      startOffset: drag?.startOffset,
      deltaY: drag?.deltaY,
      notchHeight,
      cardHeight,
    }),
  setEdgeExpanded: (expanded: boolean, pinned?: boolean) => invoke<void>("set_edge_expanded", { expanded, pinned }),
  overview: (range: string) => invoke<OverviewResponse>("get_overview", { range }),
  phraseCloud: (range: string) => invoke<PhraseCloudResponse>("get_phrase_cloud", { range }),
  liveSnapshot: () => invoke<LiveSnapshot>("get_live_snapshot"),
  liveActivity: () => invoke<LiveActivityResponse>("get_live_activity"),
  attentionHistory: (offset: number, limit: number) =>
    invoke<AttentionEvent[]>("get_attention_history", { offset, limit }),
  attentionQuality: () => invoke<AttentionQualityReport>("get_attention_quality_report"),
  repairLiveHooks: () => invoke<HookStatus>("repair_live_hooks"),
  uninstallLiveHooks: () => invoke<HookStatus>("uninstall_live_hooks"),
  jumpToLiveSession: (id: string) => invoke<void>("jump_to_live_session", { id }),
  setAttentionFeedback: (id: string, feedback: "handled" | "not-relevant" | "not-stuck" | "snoozed") =>
    invoke<AttentionEvent>("set_attention_feedback", { id, feedback }),
  jumpToAttention: (id: string) => invoke<void>("jump_to_attention", { id }),
  markNotchSessionsSeen: (ids: string[]) => invoke<void>("mark_notch_sessions_seen", { ids }),
  jumpToNotchCompletedSession: (id: string) =>
    invoke<void>("jump_to_notch_completed_session", { id }),
  deleteNotchCompletedSession: (id: string) =>
    invoke<void>("delete_notch_completed_session", { id }),
  clearNotchCompletedSessions: () =>
    invoke<NotchClearResult>("clear_notch_completed_sessions"),
  undoClearNotchCompletedSessions: (token: string) =>
    invoke<number>("undo_clear_notch_completed_sessions", { token }),
  setNotchExpanded: (expanded: boolean) => invoke<void>("set_notch_expanded", { expanded }),
  notchState: () => invoke<NotchUiState>("get_notch_state"),
  setNotchPinned: (pinned: boolean) => invoke<void>("set_notch_pinned", { pinned }),
  setNotchActivity: (hasActivity: boolean) => invoke<void>("set_notch_activity", { hasActivity }),
  setNotchLayout: (leftWingWidth: number, expandedHeight: number) =>
    invoke<void>("set_notch_layout", { leftWingWidth, expandedHeight }),
  tasks: (range: string) => invoke<TaskSummary[]>("get_tasks", { range }),
  mergeTasks: (taskIds: string[], title?: string) => invoke<string>("merge_tasks", { taskIds, title }),
  splitSession: (sessionId: string) => invoke<string>("split_session", { sessionId }),
  sessions: (query: SessionListQuery) =>
    invoke<SessionsResponse>("get_sessions", {
      range: query.range,
      agent: query.agent,
      search: query.search,
      model: query.model,
      project: query.project,
      verificationState: query.verificationState,
      attentionOnly: query.attentionOnly ?? false,
      codeOnly: query.codeOnly ?? false,
      commitOnly: query.commitOnly ?? false,
      page: query.page ?? 0,
      pageSize: query.pageSize ?? 50,
    }),
  sessionDetail: (id: string) => invoke<SessionDetail>("get_session_detail", { id }),
  sessionContext: (id: string, offset = 0, limit = 50) =>
    invoke<SessionContext>("get_session_context", { id, offset, limit }),
  delegationTrace: (sessionId: string) =>
    invoke<DelegationTraceResponse>("get_delegation_trace", { sessionId }),
  memoryLedger: (sessionId: string) =>
    invoke<MemoryLedgerResponse>("get_memory_ledger", { sessionId }),
  comparison: (range: string) => invoke<ComparisonItem[]>("get_comparison", { range }),
  projectSummaries: (range: string, agent?: string) =>
    invoke<ProjectSummary[]>("get_project_summaries", { range, agent }),
  sources: () => invoke<SourceStatus[]>("get_sources"),
  setSourceSelected: (agent: string, selected: boolean) =>
    invoke<void>("set_source_selected", { agent, selected }),
  indexStatus: () => invoke<IndexStatus>("get_index_status"),
  refreshIndex: (force = false) => invoke<boolean>("refresh_index", { force }),
  menuSnapshot: (range: string) => invoke<MenuBarSnapshot>("get_menu_bar_snapshot", { range }),
  providers: () => invoke<ProviderUsage[]>("get_provider_usage"),
  apiAccounts: () => invoke<ApiAccountInfo[]>("list_api_accounts"),
  addApiAccount: (provider: string, label: string, key: string) =>
    invoke<ApiAccountInfo[]>("add_api_account", { provider, label, key }),
  removeApiAccount: (id: string) => invoke<ApiAccountInfo[]>("remove_api_account", { id }),
  refreshProviders: (credentialsAllowed: boolean, cursorDashboardUsageEnabled = false, useSystemProxy = false) =>
    invoke<ProviderUsage[]>("refresh_provider_data", { credentialsAllowed, cursorDashboardUsageEnabled, useSystemProxy }),
  settings: () => invoke<AppSettings>("get_app_settings"),
  setSetting: (key: keyof AppSettings, value: string) => invoke<void>("set_app_setting", { key, value }),
  previewShare: (request: ShareRenderRequest) => invoke<SharePreview>("render_share_preview", { request }),
  exportShare: (render: ShareRenderRequest, format: "png" | "svg", path: string) =>
    invoke<ExportResult>("export_share", { request: { ...render, format, path } }),
  renderSharePng: (request: ShareRenderRequest) => invoke<number[]>("render_share_png", { request }),
  exportTextFile: (path: string, content: string) => invoke<void>("export_text_file", { path, content }),
  insights: (range: string) => invoke<InsightsResponse>("get_insights", { range }),
  vctiProfile: (range: string) => invoke<VctiProfile>("get_vcti_profile", { range }),
  projects: () => invoke<ProjectControl[]>("get_projects"),
  createProjectGroup: (name: string, projectHashes: string[]) =>
    invoke<string>("create_project_group", { name, projectHashes }),
  renameProjectGroup: (groupId: string, name: string) =>
    invoke<void>("rename_project_group", { groupId, name }),
  updateProjectGroupMembers: (groupId: string, projectHashes: string[]) =>
    invoke<void>("update_project_group_members", { groupId, projectHashes }),
  deleteProjectGroup: (groupId: string) =>
    invoke<void>("delete_project_group", { groupId }),
  excludeProject: (projectHash: string) => invoke<void>("exclude_project", { projectHash }),
  includeProject: (projectHash: string) => invoke<void>("include_project", { projectHash }),
  clearLocalData: () => invoke<void>("clear_local_data"),
  diagnosticRetention: () =>
    invoke<DiagnosticRetentionStatus>("get_diagnostic_retention"),
  setDiagnosticRetention: (enabled: boolean) =>
    invoke<DiagnosticRetentionStatus>("set_diagnostic_retention", { enabled }),
  clearDiagnosticRetention: () =>
    invoke<DiagnosticClearResult>("clear_diagnostic_retention"),
  showMain: () => invoke<void>("show_main_window"),
  showSettings: () => invoke<void>("show_settings_window"),
  hideMenu: () => invoke<void>("hide_menu_bar_window"),
  openProviderStatus: (provider: string) => invoke<void>("open_provider_status", { provider }),
  quit: () => invoke<void>("quit_app"),
};
