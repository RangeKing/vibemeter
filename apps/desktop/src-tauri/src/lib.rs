mod adapters;
pub mod database;
mod diagnostics;
pub mod errors;
pub mod export;
mod export_localization;
mod git_evidence;
mod ingestion;
mod live;
mod live_sources;
mod migration;
pub mod models;
mod phrases;
mod pricing;
mod privacy;
mod providers;
mod skill_usage;
mod source_capabilities;
mod tray;
mod vcti;

use crate::database::Database;
use crate::errors::{AppError, AppResult};
use crate::models::{
    AttentionEvent, AttentionQualityReport, ComparisonItem, DiagnosticClearResult,
    DiagnosticRetentionStatus, ExportRequest, ExportResult, HookStatus, IndexStatus,
    InsightsResponse, LiveActivityResponse, LiveSnapshot, MenuBarSnapshot, NotchClearResult,
    OverviewResponse, PhraseCloudResponse, PlaybookItem, ProjectControl, ProviderUsage,
    SavePlaybookRequest, SessionDetail, SessionListFilters, SessionsResponse, SharePreview,
    ShareRenderRequest, SourceStatus, TaskSummary, VctiProfile,
};
use crate::providers::ProviderStore;
use crate::source_capabilities::source_capabilities;
use chrono::Utc;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};

static BACKEND_READY: AtomicBool = AtomicBool::new(false);
static MAIN_PAGE_READY: AtomicBool = AtomicBool::new(false);

#[derive(Clone)]
struct AppState {
    database: Database,
    index_status: Arc<RwLock<IndexStatus>>,
    providers: ProviderStore,
    live: live::LiveMonitor,
    diagnostics: diagnostics::DiagnosticRetention,
}

#[tauri::command]
async fn get_overview(state: State<'_, AppState>, range: String) -> AppResult<OverviewResponse> {
    let database = state.database.clone();
    let index_status = current_index_status(&state);
    tauri::async_runtime::spawn_blocking(move || database.overview(&range, index_status))
        .await
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?
}

#[tauri::command]
async fn get_phrase_cloud(
    state: State<'_, AppState>,
    range: String,
) -> AppResult<PhraseCloudResponse> {
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || database.phrase_cloud(&range))
        .await
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?
}

#[tauri::command]
fn get_live_snapshot(state: State<'_, AppState>) -> LiveSnapshot {
    state.live.snapshot()
}

#[tauri::command]
async fn get_live_activity(state: State<'_, AppState>) -> AppResult<LiveActivityResponse> {
    let database = state.database.clone();
    let snapshot = state.live.snapshot();
    let mut activity = tauri::async_runtime::spawn_blocking(move || database.live_activity())
        .await
        .map_err(|error| AppError::InvalidRequest(error.to_string()))??;
    let title_database = state.database.clone();
    live::hydrate_attention_titles(&title_database, &mut activity.attention);
    let metrics = std::mem::take(&mut activity.concurrency);
    let agents = snapshot
        .sessions
        .iter()
        .map(|session| session.agent.clone())
        .chain(metrics.iter().map(|lane| lane.agent.clone()))
        .collect::<std::collections::BTreeSet<_>>();
    activity.concurrency = agents
        .into_iter()
        .map(|agent| {
            let live_sessions = snapshot
                .sessions
                .iter()
                .filter(|session| session.agent == agent)
                .collect::<Vec<_>>();
            let metric = metrics.iter().find(|lane| lane.agent == agent);
            let mut projects = metric.map(|lane| lane.projects.clone()).unwrap_or_default();
            for session in &live_sessions {
                if !session.project_label.is_empty()
                    && !projects
                        .iter()
                        .any(|project| project == &session.project_label)
                {
                    projects.push(session.project_label.clone());
                }
            }
            if !live_sessions.is_empty() {
                crate::models::LiveConcurrencyLane {
                    agent,
                    session_count: live_sessions.len() as u64,
                    waiting_count: live_sessions
                        .iter()
                        .filter(|session| session.status == "waiting")
                        .count() as u64,
                    error_count: live_sessions
                        .iter()
                        .filter(|session| session.status == "error")
                        .count() as u64,
                    running_count: live_sessions
                        .iter()
                        .filter(|session| session.status == "running")
                        .count() as u64,
                    completed_count: live_sessions
                        .iter()
                        .filter(|session| session.status == "completed")
                        .count() as u64,
                    projects,
                }
            } else {
                crate::models::LiveConcurrencyLane {
                    agent,
                    session_count: metric.map(|lane| lane.session_count).unwrap_or(0),
                    waiting_count: 0,
                    error_count: 0,
                    running_count: 0,
                    completed_count: metric.map(|lane| lane.completed_count).unwrap_or(0),
                    projects,
                }
            }
        })
        .collect();
    Ok(activity)
}

#[tauri::command]
async fn get_attention_history(
    state: State<'_, AppState>,
    offset: u64,
    limit: u64,
) -> AppResult<Vec<AttentionEvent>> {
    let database = state.database.clone();
    let title_database = database.clone();
    let mut attention =
        tauri::async_runtime::spawn_blocking(move || database.attention_history(offset, limit))
            .await
            .map_err(|error| AppError::InvalidRequest(error.to_string()))??;
    live::hydrate_attention_titles(&title_database, &mut attention);
    Ok(attention)
}

#[tauri::command]
async fn get_attention_quality_report(
    state: State<'_, AppState>,
) -> AppResult<AttentionQualityReport> {
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || database.attention_quality_report())
        .await
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?
}

#[tauri::command]
fn repair_live_hooks(state: State<'_, AppState>) -> AppResult<HookStatus> {
    let status = live::install_hooks()?;
    state.database.set_setting("liveHooksEnabled", "true")?;
    Ok(status)
}

#[tauri::command]
fn uninstall_live_hooks(state: State<'_, AppState>) -> AppResult<HookStatus> {
    let status = live::uninstall_hooks()?;
    state.database.set_setting("liveHooksEnabled", "false")?;
    Ok(status)
}

#[tauri::command]
fn jump_to_live_session(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let session = state
        .live
        .session(&id)
        .ok_or_else(|| AppError::InvalidRequest("live session is no longer active".into()))?;
    live::jump_to_session(&session)
}

#[tauri::command]
fn set_attention_feedback(
    state: State<'_, AppState>,
    id: String,
    feedback: String,
) -> AppResult<AttentionEvent> {
    state.database.set_attention_feedback(&id, &feedback)
}

#[tauri::command]
fn jump_to_attention(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let attention = state
        .database
        .attention_event(&id)?
        .ok_or_else(|| AppError::InvalidRequest("attention event is unavailable".into()))?;
    let Some(session) = state
        .live
        .session_for_source(&attention.agent, &attention.source_session_id)
    else {
        state.database.record_attention_jump(&id, false)?;
        return Err(AppError::InvalidRequest(
            "attention source is no longer active".into(),
        ));
    };
    if let Err(error) = live::jump_to_session(&session) {
        state.database.record_attention_jump(&id, false)?;
        return Err(error);
    }
    state.database.record_attention_jump(&id, true)
}

#[tauri::command]
fn mark_notch_sessions_seen(state: State<'_, AppState>, ids: Vec<String>) -> AppResult<()> {
    state.live.mark_notch_sessions_seen(&ids)
}

#[tauri::command]
fn jump_to_notch_completed_session(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let completed = state
        .database
        .notch_completed_session(&id)?
        .ok_or_else(|| {
            AppError::InvalidRequest("completed session is no longer available".into())
        })?;
    live::jump_to_session(&completed.session)?;
    state.database.delete_notch_completed_session(&id)?;
    Ok(())
}

#[tauri::command]
fn delete_notch_completed_session(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.database.delete_notch_completed_session(&id)?;
    Ok(())
}

#[tauri::command]
fn clear_notch_completed_sessions(state: State<'_, AppState>) -> AppResult<NotchClearResult> {
    state.database.clear_notch_completed_sessions()
}

#[tauri::command]
fn undo_clear_notch_completed_sessions(
    state: State<'_, AppState>,
    token: String,
) -> AppResult<u64> {
    state.database.undo_clear_notch_completed_sessions(&token)
}

#[tauri::command]
fn set_notch_expanded(app: AppHandle, expanded: bool) -> AppResult<()> {
    tray::set_notch_expanded(&app, expanded)
        .map_err(|error| AppError::InvalidRequest(error.to_string()))
}

#[tauri::command]
fn get_notch_state() -> tray::NotchUiState {
    tray::notch_state()
}

#[tauri::command]
fn set_notch_pinned(app: AppHandle, pinned: bool) -> AppResult<()> {
    tray::set_notch_pinned(&app, pinned)
        .map_err(|error| AppError::InvalidRequest(error.to_string()))
}

#[tauri::command]
fn set_notch_activity(app: AppHandle, has_activity: bool) -> AppResult<()> {
    tray::set_notch_activity(&app, has_activity)
        .map_err(|error| AppError::InvalidRequest(error.to_string()))
}

#[tauri::command]
fn set_notch_layout(app: AppHandle, left_wing_width: f64, expanded_height: f64) -> AppResult<()> {
    tray::set_notch_layout(&app, left_wing_width, expanded_height)
        .map_err(|error| AppError::InvalidRequest(error.to_string()))
}

#[tauri::command]
async fn get_tasks(state: State<'_, AppState>, range: String) -> AppResult<Vec<TaskSummary>> {
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || database.tasks(&range))
        .await
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?
}

#[tauri::command]
async fn merge_tasks(
    state: State<'_, AppState>,
    task_ids: Vec<String>,
    title: Option<String>,
) -> AppResult<String> {
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || database.merge_tasks(&task_ids, title.as_deref()))
        .await
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?
}

#[tauri::command]
async fn split_session(state: State<'_, AppState>, session_id: String) -> AppResult<String> {
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || database.split_session(&session_id))
        .await
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn get_sessions(
    state: State<'_, AppState>,
    range: String,
    agent: Option<String>,
    search: Option<String>,
    model: Option<String>,
    project: Option<String>,
    verification_state: Option<String>,
    attention_only: Option<bool>,
    code_only: Option<bool>,
    commit_only: Option<bool>,
    page: Option<u64>,
    page_size: Option<u64>,
) -> AppResult<SessionsResponse> {
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || {
        database.sessions(
            &range,
            SessionListFilters {
                agent: agent.as_deref(),
                search: search.as_deref(),
                model: model.as_deref(),
                project: project.as_deref(),
                verification_state: verification_state.as_deref(),
                attention_only: attention_only.unwrap_or(false),
                code_only: code_only.unwrap_or(false),
                commit_only: commit_only.unwrap_or(false),
            },
            page.unwrap_or(0),
            page_size.unwrap_or(50),
        )
    })
    .await
    .map_err(|error| AppError::InvalidRequest(error.to_string()))?
}

#[tauri::command]
async fn get_session_detail(state: State<'_, AppState>, id: String) -> AppResult<SessionDetail> {
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut detail = database.session_detail(&id)?;
        detail.content_preview = ingestion::session_content_preview(&database, &id)?;
        Ok(detail)
    })
    .await
    .map_err(|error| AppError::InvalidRequest(error.to_string()))?
}

#[tauri::command]
async fn get_comparison(
    state: State<'_, AppState>,
    range: String,
) -> AppResult<Vec<ComparisonItem>> {
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || database.comparison(&range))
        .await
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?
}

#[tauri::command]
async fn render_share_preview(
    state: State<'_, AppState>,
    request: ShareRenderRequest,
) -> AppResult<SharePreview> {
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || export::preview(&database, request))
        .await
        .map_err(|error| AppError::Render(error.to_string()))?
}

#[tauri::command]
async fn export_share(
    state: State<'_, AppState>,
    request: ExportRequest,
) -> AppResult<ExportResult> {
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || export::export(&database, request))
        .await
        .map_err(|error| AppError::Render(error.to_string()))?
}

#[tauri::command]
async fn render_share_png(
    state: State<'_, AppState>,
    request: ShareRenderRequest,
) -> AppResult<Vec<u8>> {
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || export::png_bytes(&database, request))
        .await
        .map_err(|error| AppError::Render(error.to_string()))?
}

#[tauri::command]
fn export_text_file(path: String, content: String) -> AppResult<()> {
    if path.trim().is_empty() || content.len() > 200_000 {
        return Err(AppError::InvalidRequest("invalid text export".into()));
    }
    let output = std::path::Path::new(&path);
    if output.file_name().is_none() {
        return Err(AppError::InvalidRequest("invalid text export path".into()));
    }
    std::fs::write(output, content)?;
    Ok(())
}

#[tauri::command]
async fn get_insights(state: State<'_, AppState>, range: String) -> AppResult<InsightsResponse> {
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || database.insights(&range))
        .await
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?
}

#[tauri::command]
async fn get_vcti_profile(state: State<'_, AppState>, range: String) -> AppResult<VctiProfile> {
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || database.vcti_profile(&range))
        .await
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?
}

#[tauri::command]
async fn get_playbook(
    state: State<'_, AppState>,
    search: Option<String>,
) -> AppResult<Vec<PlaybookItem>> {
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || database.playbook_items(search.as_deref()))
        .await
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?
}

#[tauri::command]
async fn save_playbook_item(
    state: State<'_, AppState>,
    request: SavePlaybookRequest,
) -> AppResult<PlaybookItem> {
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || database.save_playbook_item(&request))
        .await
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?
}

#[tauri::command]
async fn delete_playbook_item(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || database.delete_playbook_item(&id))
        .await
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?
}

#[tauri::command]
async fn get_projects(state: State<'_, AppState>) -> AppResult<Vec<ProjectControl>> {
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || database.projects())
        .await
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?
}

#[tauri::command]
async fn exclude_project(state: State<'_, AppState>, project_hash: String) -> AppResult<()> {
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || database.exclude_project(&project_hash))
        .await
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?
}

#[tauri::command]
async fn include_project(state: State<'_, AppState>, project_hash: String) -> AppResult<()> {
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || database.include_project(&project_hash))
        .await
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?
}

#[tauri::command]
async fn clear_local_data(state: State<'_, AppState>) -> AppResult<()> {
    let database = state.database.clone();
    let diagnostics = state.diagnostics.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _ = diagnostics.clear()?;
        database.clear_local_data()
    })
    .await
    .map_err(|error| AppError::InvalidRequest(error.to_string()))?
}

#[tauri::command]
async fn get_diagnostic_retention(
    state: State<'_, AppState>,
) -> AppResult<DiagnosticRetentionStatus> {
    let diagnostics = state.diagnostics.clone();
    tauri::async_runtime::spawn_blocking(move || diagnostics.status())
        .await
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?
}

#[tauri::command]
async fn set_diagnostic_retention(
    state: State<'_, AppState>,
    enabled: bool,
) -> AppResult<DiagnosticRetentionStatus> {
    let diagnostics = state.diagnostics.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if enabled {
            diagnostics.enable()
        } else {
            Ok(diagnostics.clear()?.status)
        }
    })
    .await
    .map_err(|error| AppError::InvalidRequest(error.to_string()))?
}

#[tauri::command]
async fn clear_diagnostic_retention(
    state: State<'_, AppState>,
) -> AppResult<DiagnosticClearResult> {
    let diagnostics = state.diagnostics.clone();
    tauri::async_runtime::spawn_blocking(move || diagnostics.clear())
        .await
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?
}

#[tauri::command]
async fn get_sources(state: State<'_, AppState>) -> AppResult<Vec<SourceStatus>> {
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || database.sources())
        .await
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?
}

#[tauri::command]
async fn set_source_selected(
    state: State<'_, AppState>,
    agent: String,
    selected: bool,
) -> AppResult<()> {
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || database.set_source_selected(&agent, selected))
        .await
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?
}

#[tauri::command]
fn get_index_status(state: State<'_, AppState>) -> IndexStatus {
    current_index_status(&state)
}

#[tauri::command]
fn refresh_index(app: AppHandle, state: State<'_, AppState>, force: bool) -> bool {
    ingestion::start_indexing(
        state.database.clone(),
        state.index_status.clone(),
        app,
        force,
    )
}

#[tauri::command]
async fn get_menu_bar_snapshot(
    state: State<'_, AppState>,
    range: String,
) -> AppResult<MenuBarSnapshot> {
    if !matches!(
        range.as_str(),
        "today" | "7d" | "30d" | "90d" | "180d" | "year"
    ) {
        return Err(AppError::InvalidRequest("unsupported menu range".into()));
    }
    let database = state.database.clone();
    let mut providers = state.providers.snapshot();
    for provider in &mut providers {
        // The menu bar only needs quota and service health. Keep the range
        // history on the full data surfaces instead of serializing it every
        // time the popover refreshes.
        provider.account_usage = None;
    }
    let index_status = current_index_status(&state);
    tauri::async_runtime::spawn_blocking(move || {
        let (usage, cost_usd, heatmap, hourly) = database.range_usage_and_activity(&range)?;
        Ok(MenuBarSnapshot {
            generated_at: Utc::now().to_rfc3339(),
            range,
            usage,
            cost_usd,
            heatmap,
            hourly,
            providers,
            index_status,
        })
    })
    .await
    .map_err(|error| AppError::InvalidRequest(error.to_string()))?
}

#[tauri::command]
fn get_provider_usage(state: State<'_, AppState>) -> Vec<ProviderUsage> {
    state.providers.snapshot()
}

#[tauri::command]
async fn refresh_provider_data(
    state: State<'_, AppState>,
    credentials_allowed: bool,
    cursor_dashboard_usage_enabled: bool,
    use_system_proxy: bool,
) -> AppResult<Vec<ProviderUsage>> {
    let providers = state.providers.clone();
    let providers_for_task = providers.clone();
    tauri::async_runtime::spawn_blocking(move || {
        providers_for_task.refresh(
            credentials_allowed,
            cursor_dashboard_usage_enabled,
            use_system_proxy,
        )
    })
    .await
    .map_err(|error| AppError::ProviderUnavailable(error.to_string()))?;
    Ok(providers.snapshot())
}

#[tauri::command]
async fn get_app_settings(state: State<'_, AppState>) -> AppResult<BTreeMap<String, String>> {
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut settings = BTreeMap::new();
        for (key, fallback) in [
            ("locale", "system"),
            ("theme", "system"),
            ("onboardingComplete", "false"),
            ("credentialsAllowed", "false"),
            ("cursorDashboardUsage", "false"),
            ("useSystemProxy", "false"),
            ("gitReadAllowed", "false"),
            ("vctiPromptStructure", "true"),
            ("retentionDays", "365"),
            ("launchAtLogin", "false"),
            ("liveHooksEnabled", "true"),
            ("notchEnabled", "true"),
            ("menuBarEnabled", "true"),
            ("dataPageAgents", "auto"),
            ("iaMigrationTipSeen", "false"),
        ] {
            settings.insert(
                key.into(),
                database.setting(key)?.unwrap_or_else(|| fallback.into()),
            );
        }
        Ok(settings)
    })
    .await
    .map_err(|error| AppError::InvalidRequest(error.to_string()))?
}

#[tauri::command]
async fn set_app_setting(
    app: AppHandle,
    state: State<'_, AppState>,
    key: String,
    value: String,
) -> AppResult<()> {
    validate_setting(&key, &value)?;
    if key == "notchEnabled" {
        tray::set_notch_enabled(&app, value == "true")
            .map_err(|error| AppError::InvalidRequest(error.to_string()))?;
    }
    if key == "menuBarEnabled" {
        tray::set_menu_bar_enabled(&app, value == "true")
            .map_err(|error| AppError::InvalidRequest(error.to_string()))?;
    }
    if key == "liveHooksEnabled" {
        if value == "true" {
            live::install_hooks()?;
        } else {
            live::uninstall_hooks()?;
        }
    }
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || database.set_setting(&key, &value))
        .await
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?
}

fn validate_setting(key: &str, value: &str) -> AppResult<()> {
    let valid = match key {
        "locale" => matches!(value, "system" | "zh-CN" | "en-US"),
        "theme" => matches!(value, "system" | "light" | "dark"),
        "onboardingComplete"
        | "credentialsAllowed"
        | "cursorDashboardUsage"
        | "useSystemProxy"
        | "gitReadAllowed"
        | "vctiPromptStructure"
        | "launchAtLogin"
        | "liveHooksEnabled"
        | "notchEnabled"
        | "menuBarEnabled"
        | "iaMigrationTipSeen" => {
            matches!(value, "true" | "false")
        }
        "retentionDays" => matches!(value, "30" | "90" | "180" | "365" | "730"),
        "dataPageAgents" => {
            value == "auto"
                || serde_json::from_str::<Vec<String>>(value).is_ok_and(|agents| {
                    agents.iter().all(|agent| {
                        source_capabilities()
                            .iter()
                            .any(|capability| capability.agent == agent.as_str())
                    })
                })
        }
        _ => return Err(AppError::InvalidRequest("unknown setting".into())),
    };
    if valid {
        Ok(())
    } else {
        Err(AppError::InvalidRequest("invalid setting value".into()))
    }
}

fn startup_can_reveal_main_window(backend_ready: bool, page_ready: bool, preview: bool) -> bool {
    backend_ready && page_ready && !preview
}

fn is_surface_preview() -> bool {
    std::env::var_os("VIBEMETER_PREVIEW_MENUBAR").is_some()
        || std::env::var_os("AFTERVIBE_PREVIEW_MENUBAR").is_some()
        || std::env::var_os("TOKENGRAPH_PREVIEW_MENUBAR").is_some()
        || std::env::var_os("VIBEMETER_PREVIEW_NOTCH").is_some()
}

fn reveal_main_window(app: &AppHandle) -> AppResult<()> {
    app.set_activation_policy(tauri::ActivationPolicy::Regular)
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?;
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| AppError::InvalidRequest("main window is unavailable".into()))?;
    window
        .unminimize()
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?;
    window
        .show()
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?;
    window
        .set_focus()
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?;
    Ok(())
}

fn reveal_main_window_if_ready(app: &AppHandle) {
    if startup_can_reveal_main_window(
        BACKEND_READY.load(Ordering::SeqCst),
        MAIN_PAGE_READY.load(Ordering::SeqCst),
        is_surface_preview(),
    ) {
        let _ = reveal_main_window(app);
    }
}

#[tauri::command]
fn show_main_window(app: AppHandle) -> AppResult<()> {
    reveal_main_window(&app)?;
    app.emit_to("main", "navigate", "data")
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?;
    Ok(())
}

#[tauri::command]
fn show_settings_window(app: AppHandle) -> AppResult<()> {
    reveal_main_window(&app)?;
    app.emit_to("main", "navigate", "settings")
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?;
    Ok(())
}

#[tauri::command]
fn hide_menu_bar_window(app: AppHandle) -> AppResult<()> {
    if let Some(window) = app.get_webview_window("menubar") {
        window
            .hide()
            .map_err(|error| AppError::InvalidRequest(error.to_string()))?;
    }
    Ok(())
}

#[tauri::command]
fn open_provider_status(provider: String) -> AppResult<()> {
    let url = match provider.as_str() {
        "claude" => "https://status.claude.com",
        "codex" => "https://status.openai.com",
        "cursor" => "https://status.cursor.com",
        _ => {
            return Err(AppError::InvalidRequest(
                "unknown provider status page".into(),
            ));
        }
    };
    std::process::Command::new("open")
        .arg(url)
        .spawn()
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?;
    Ok(())
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

fn current_index_status(state: &State<'_, AppState>) -> IndexStatus {
    state
        .index_status
        .read()
        .map(|status| status.clone())
        .unwrap_or_default()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
        reveal_main_window_if_ready(app);
    }));
    let builder = builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init());
    #[cfg(target_os = "macos")]
    let builder = builder.plugin(tauri_nspanel::init());
    let builder = builder
        .setup(|app| {
            #[cfg(desktop)]
            app.handle().plugin(tauri_plugin_autostart::init(
                tauri_plugin_autostart::MacosLauncher::LaunchAgent,
                Some(vec!["--autostart"]),
            ))?;
            let data_dir = app.path().app_data_dir()?;
            #[cfg(debug_assertions)]
            let database_path = std::env::var("VIBEMETER_TEST_DB")
                .or_else(|_| std::env::var("AFTERVIBE_TEST_DB"))
                .or_else(|_| std::env::var("TOKEN_GRAPH_TEST_DB"))
                .map(std::path::PathBuf::from)
                .unwrap_or(migration::prepare_database(&data_dir)?);
            #[cfg(not(debug_assertions))]
            let database_path = migration::prepare_database(&data_dir)?;
            let database = Database::open_for_startup(database_path.clone())?;
            let index_status = Arc::new(RwLock::new(IndexStatus::default()));
            let providers = ProviderStore::new(data_dir.join("ProviderProbe"))?;
            let diagnostics = diagnostics::DiagnosticRetention::new(
                database.clone(),
                database_path.to_string_lossy().to_string(),
            );
            if diagnostics.expire_if_needed().is_err() {
                eprintln!("VibeMeter diagnostic cleanup requires attention in Settings");
            }
            let live = live::LiveMonitor::start(
                database.clone(),
                app.handle().clone(),
                diagnostics.clone(),
            )?;
            let onboarding_complete = database
                .setting("onboardingComplete")?
                .is_some_and(|value| value == "true");
            let hooks_enabled = database
                .setting("liveHooksEnabled")?
                .is_none_or(|value| value == "true");
            if onboarding_complete && hooks_enabled {
                let _ = live::install_hooks();
                database.set_setting("liveHooksEnabled", "true")?;
            }
            let state = AppState {
                database: database.clone(),
                index_status: index_status.clone(),
                providers: providers.clone(),
                live,
                diagnostics: diagnostics.clone(),
            };
            app.manage(state);
            let notch_enabled = onboarding_complete
                && database
                    .setting("notchEnabled")?
                    .is_none_or(|value| value == "true");
            let menu_bar_enabled = database
                .setting("menuBarEnabled")?
                .is_none_or(|value| value == "true");
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            tray::setup(app, notch_enabled, menu_bar_enabled)?;

            #[cfg(debug_assertions)]
            if std::env::var_os("VIBEMETER_PREVIEW_MENUBAR").is_some()
                || std::env::var_os("AFTERVIBE_PREVIEW_MENUBAR").is_some()
                || std::env::var_os("TOKENGRAPH_PREVIEW_MENUBAR").is_some()
            {
                if let Some(main) = app.get_webview_window("main") {
                    let _ = main.hide();
                }
                if let Some(menubar) = app.get_webview_window("menubar") {
                    let _ = menubar.set_position(tauri::PhysicalPosition::new(500, 80));
                    let _ = menubar.show();
                    let _ = menubar.set_focus();
                }
            }
            #[cfg(debug_assertions)]
            if std::env::var_os("VIBEMETER_PREVIEW_NOTCH").is_some() {
                if let Some(main) = app.get_webview_window("main") {
                    let _ = main.hide();
                }
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
                let _ = tray::set_notch_activity(app.handle(), true);
                let _ = tray::set_notch_expanded(app.handle(), true);
                if std::env::var_os("VIBEMETER_PREVIEW_NOTCH_PINNED").is_some() {
                    let _ = tray::set_notch_pinned(app.handle(), true);
                }
            }

            let provider_copy = providers.clone();
            let provider_database = database.clone();
            std::thread::spawn(move || {
                loop {
                    let credentials_allowed = provider_database
                        .setting("credentialsAllowed")
                        .ok()
                        .flatten()
                        .is_some_and(|value| value == "true");
                    let cursor_dashboard_usage_enabled = provider_database
                        .setting("cursorDashboardUsage")
                        .ok()
                        .flatten()
                        .is_some_and(|value| value == "true");
                    let use_system_proxy = provider_database
                        .setting("useSystemProxy")
                        .ok()
                        .flatten()
                        .is_some_and(|value| value == "true");
                    provider_copy.refresh(
                        credentials_allowed,
                        cursor_dashboard_usage_enabled,
                        use_system_proxy,
                    );
                    std::thread::sleep(std::time::Duration::from_secs(5 * 60));
                }
            });

            let index_database = database.clone();
            let index_status_copy = index_status.clone();
            let index_app = app.handle().clone();
            std::thread::spawn(move || {
                loop {
                    let _ = ingestion::start_indexing(
                        index_database.clone(),
                        index_status_copy.clone(),
                        index_app.clone(),
                        false,
                    );
                    std::thread::sleep(std::time::Duration::from_secs(60));
                }
            });
            let diagnostic_expiry = diagnostics;
            std::thread::spawn(move || {
                loop {
                    let _ = diagnostic_expiry.expire_if_needed();
                    std::thread::sleep(std::time::Duration::from_secs(60));
                }
            });
            BACKEND_READY.store(true, Ordering::SeqCst);
            reveal_main_window_if_ready(app.handle());
            Ok(())
        })
        .on_page_load(|webview, payload| {
            if webview.label() == "main" && payload.event() == PageLoadEvent::Finished {
                MAIN_PAGE_READY.store(true, Ordering::SeqCst);
                reveal_main_window_if_ready(webview.app_handle());
            }
        })
        .on_window_event(|window, event| {
            if window.label() == "main"
                && let WindowEvent::CloseRequested { api, .. } = event
            {
                api.prevent_close();
                let _ = window.hide();
                let _ = window
                    .app_handle()
                    .set_activation_policy(tauri::ActivationPolicy::Accessory);
            }
            if window.label() == "menubar"
                && let WindowEvent::Focused(false) = event
            {
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_overview,
            get_phrase_cloud,
            get_live_snapshot,
            get_live_activity,
            get_attention_history,
            get_attention_quality_report,
            repair_live_hooks,
            uninstall_live_hooks,
            jump_to_live_session,
            set_attention_feedback,
            jump_to_attention,
            mark_notch_sessions_seen,
            jump_to_notch_completed_session,
            delete_notch_completed_session,
            clear_notch_completed_sessions,
            undo_clear_notch_completed_sessions,
            set_notch_expanded,
            get_notch_state,
            set_notch_pinned,
            set_notch_activity,
            set_notch_layout,
            get_tasks,
            merge_tasks,
            split_session,
            get_sessions,
            get_session_detail,
            get_comparison,
            render_share_preview,
            export_share,
            render_share_png,
            export_text_file,
            get_insights,
            get_vcti_profile,
            get_playbook,
            save_playbook_item,
            delete_playbook_item,
            get_projects,
            exclude_project,
            include_project,
            clear_local_data,
            get_diagnostic_retention,
            set_diagnostic_retention,
            clear_diagnostic_retention,
            get_sources,
            set_source_selected,
            get_index_status,
            refresh_index,
            get_menu_bar_snapshot,
            get_provider_usage,
            refresh_provider_data,
            get_app_settings,
            set_app_setting,
            show_main_window,
            show_settings_window,
            hide_menu_bar_window,
            open_provider_status,
            quit_app,
        ]);

    match builder.build(tauri::generate_context!()) {
        Ok(app) => app.run(|app_handle, event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen {
                has_visible_windows: false,
                ..
            } = event
            {
                reveal_main_window_if_ready(app_handle);
            }
        }),
        Err(error) => eprintln!("VibeMeter failed to start: {error}"),
    }
}

#[cfg(test)]
mod startup_tests {
    use super::{startup_can_reveal_main_window, validate_setting};

    #[test]
    fn configured_main_window_starts_hidden_until_the_page_is_ready() {
        let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json"))
            .expect("Tauri configuration should be valid JSON");
        let main_window = config["app"]["windows"]
            .as_array()
            .and_then(|windows| windows.iter().find(|window| window["label"] == "main"))
            .expect("main window should be configured");
        assert_eq!(
            main_window["visible"].as_bool(),
            Some(false),
            "the native shell must stay hidden while setup and the first page load are incomplete"
        );
    }

    #[test]
    fn startup_gate_requires_both_backend_and_page_readiness() {
        assert!(!startup_can_reveal_main_window(false, false, false));
        assert!(!startup_can_reveal_main_window(true, false, false));
        assert!(!startup_can_reveal_main_window(false, true, false));
        assert!(!startup_can_reveal_main_window(true, true, true));
        assert!(startup_can_reveal_main_window(true, true, false));
    }

    #[test]
    fn system_proxy_setting_accepts_only_boolean_values() {
        assert!(validate_setting("useSystemProxy", "true").is_ok());
        assert!(validate_setting("useSystemProxy", "false").is_ok());
        assert!(validate_setting("useSystemProxy", "automatic").is_err());
    }

    #[test]
    fn data_page_agent_setting_accepts_auto_and_known_agents_only() {
        assert!(validate_setting("dataPageAgents", "auto").is_ok());
        assert!(validate_setting("dataPageAgents", "[]").is_ok());
        assert!(validate_setting("dataPageAgents", r#"["codex","grok-build"]"#).is_ok());
        assert!(validate_setting("dataPageAgents", r#"["not-an-agent"]"#).is_err());
        assert!(validate_setting("dataPageAgents", "not-json").is_err());
    }
}
