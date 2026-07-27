mod adapters;
pub mod database;
pub mod errors;
pub mod export;
mod export_localization;
mod git_evidence;
mod ingestion;
mod live;
mod migration;
pub mod models;
mod phrases;
mod pricing;
mod privacy;
mod providers;
mod tray;
mod vcti;

use crate::database::Database;
use crate::errors::{AppError, AppResult};
use crate::models::{
    ComparisonItem, ExportRequest, ExportResult, HookStatus, IndexStatus, InsightsResponse,
    LiveSnapshot, MenuBarSnapshot, OverviewResponse, PhraseCloudResponse, PlaybookItem,
    ProjectControl, ProviderUsage, SavePlaybookRequest, SessionDetail, SessionsResponse,
    SharePreview, ShareRenderRequest, SourceStatus, TaskSummary, VctiProfile,
};
use crate::providers::ProviderStore;
use chrono::Utc;
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};

#[derive(Clone)]
struct AppState {
    database: Database,
    index_status: Arc<RwLock<IndexStatus>>,
    providers: ProviderStore,
    live: live::LiveMonitor,
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
async fn get_sessions(
    state: State<'_, AppState>,
    range: String,
    agent: Option<String>,
    search: Option<String>,
    page: u64,
    page_size: u64,
) -> AppResult<SessionsResponse> {
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || {
        database.sessions(&range, agent.as_deref(), search.as_deref(), page, page_size)
    })
    .await
    .map_err(|error| AppError::InvalidRequest(error.to_string()))?
}

#[tauri::command]
async fn get_session_detail(state: State<'_, AppState>, id: String) -> AppResult<SessionDetail> {
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || database.session_detail(&id))
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
async fn get_vcti_profile(state: State<'_, AppState>) -> AppResult<VctiProfile> {
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || database.vcti_profile())
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
    tauri::async_runtime::spawn_blocking(move || database.clear_local_data())
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
async fn get_menu_bar_snapshot(state: State<'_, AppState>) -> AppResult<MenuBarSnapshot> {
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
        let (today_usage, today_cost_usd, heatmap) = database.today_and_heatmap(28)?;
        Ok(MenuBarSnapshot {
            generated_at: Utc::now().to_rfc3339(),
            today_usage,
            today_cost_usd,
            heatmap,
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
) -> AppResult<Vec<ProviderUsage>> {
    let providers = state.providers.clone();
    let providers_for_task = providers.clone();
    tauri::async_runtime::spawn_blocking(move || {
        providers_for_task.refresh(credentials_allowed, cursor_dashboard_usage_enabled)
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
            ("gitReadAllowed", "false"),
            ("vctiPromptStructure", "true"),
            ("retentionDays", "365"),
            ("launchAtLogin", "false"),
            ("liveHooksEnabled", "true"),
            ("notchEnabled", "true"),
            ("menuBarEnabled", "true"),
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
        | "gitReadAllowed"
        | "vctiPromptStructure"
        | "launchAtLogin"
        | "liveHooksEnabled"
        | "notchEnabled"
        | "menuBarEnabled" => {
            matches!(value, "true" | "false")
        }
        "retentionDays" => matches!(value, "30" | "90" | "180" | "365" | "730"),
        _ => return Err(AppError::InvalidRequest("unknown setting".into())),
    };
    if valid {
        Ok(())
    } else {
        Err(AppError::InvalidRequest("invalid setting value".into()))
    }
}

#[tauri::command]
fn show_main_window(app: AppHandle) -> AppResult<()> {
    app.set_activation_policy(tauri::ActivationPolicy::Regular)
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?;
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| AppError::InvalidRequest("main window is unavailable".into()))?;
    window
        .show()
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?;
    window
        .set_focus()
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?;
    app.emit_to("main", "navigate", "data")
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?;
    Ok(())
}

#[tauri::command]
fn show_settings_window(app: AppHandle) -> AppResult<()> {
    app.set_activation_policy(tauri::ActivationPolicy::Regular)
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?;
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| AppError::InvalidRequest("main window is unavailable".into()))?;
    window
        .show()
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?;
    window
        .set_focus()
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?;
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
    let builder = tauri::Builder::default()
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
            let database = Database::open(database_path)?;
            let index_status = Arc::new(RwLock::new(IndexStatus::default()));
            let providers = ProviderStore::new(data_dir.join("ProviderProbe"))?;
            let live = live::LiveMonitor::start(database.clone(), app.handle().clone())?;
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
            #[cfg(target_os = "macos")]
            if std::env::var_os("VIBEMETER_PREVIEW_NOTCH").is_none() {
                app.set_activation_policy(tauri::ActivationPolicy::Regular);
            }

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
                    provider_copy.refresh(credentials_allowed, cursor_dashboard_usage_enabled);
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
            Ok(())
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
            repair_live_hooks,
            uninstall_live_hooks,
            jump_to_live_session,
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
                && let Some(window) = app_handle.get_webview_window("main")
            {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }),
        Err(error) => eprintln!("VibeMeter failed to start: {error}"),
    }
}
