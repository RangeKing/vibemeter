mod adapters;
pub mod database;
mod deep_review;
pub mod errors;
pub mod export;
mod export_localization;
mod git_evidence;
mod ingestion;
pub mod models;
mod pricing;
mod privacy;
mod providers;
mod review_engine;
mod review_localization;
mod tray;
mod vcti;

use crate::database::Database;
use crate::errors::{AppError, AppResult};
use crate::models::{
    ComparisonItem, DeepReviewPreview, DeepReviewRequest, ExportRequest, ExportResult,
    GenerateReviewRequest, IndexStatus, InsightsResponse, MenuBarSnapshot, OverviewResponse,
    PlaybookItem, ProjectControl, ProviderUsage, ReviewDocument, ReviewsResponse,
    SavePlaybookRequest, SessionDetail, SessionsResponse, SharePreview, ShareRenderRequest,
    SourceStatus, TaskSummary, TodayResponse, UpdateReviewRequest, VctiProfile,
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
async fn get_today(state: State<'_, AppState>) -> AppResult<TodayResponse> {
    let database = state.database.clone();
    let index_status = current_index_status(&state);
    tauri::async_runtime::spawn_blocking(move || database.today(index_status))
        .await
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?
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
async fn get_reviews(
    state: State<'_, AppState>,
    review_type: Option<String>,
    target_id: Option<String>,
) -> AppResult<ReviewsResponse> {
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || {
        database.reviews(review_type.as_deref(), target_id.as_deref())
    })
    .await
    .map_err(|error| AppError::InvalidRequest(error.to_string()))?
}

#[tauri::command]
async fn generate_review(
    state: State<'_, AppState>,
    request: GenerateReviewRequest,
) -> AppResult<ReviewDocument> {
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || database.generate_review(&request))
        .await
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?
}

#[tauri::command]
async fn preview_deep_review(
    state: State<'_, AppState>,
    task_id: String,
    locale: String,
    mode: String,
    provider: String,
    model: Option<String>,
) -> AppResult<DeepReviewPreview> {
    deep_review::validate_route(&mode, &provider, model.as_deref())?;
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (title, payload) = database.deep_review_payload(&task_id, &locale)?;
        let payload_hash = deep_review::payload_hash(
            &task_id,
            &locale,
            &mode,
            &provider,
            model.as_deref(),
            &payload,
        );
        Ok(DeepReviewPreview {
            task_id,
            title,
            mode,
            provider,
            model,
            character_count: payload.chars().count() as u64,
            payload,
            payload_hash,
            network_required: true,
            privacy_notes: vec![
                "deepReview.privacy.bounded".into(),
                "deepReview.privacy.noTranscript".into(),
                "deepReview.privacy.noSecrets".into(),
            ],
        })
    })
    .await
    .map_err(|error| AppError::InvalidRequest(error.to_string()))?
}

#[tauri::command]
async fn generate_deep_review(
    app: AppHandle,
    state: State<'_, AppState>,
    request: DeepReviewRequest,
) -> AppResult<ReviewDocument> {
    deep_review::validate_route(&request.mode, &request.provider, request.model.as_deref())?;
    let database = state.database.clone();
    let work_dir = app
        .path()
        .app_cache_dir()
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?
        .join("DeepReview");
    tauri::async_runtime::spawn_blocking(move || {
        let (fallback_title, payload) =
            database.deep_review_payload(&request.task_id, &request.locale)?;
        let current_hash = deep_review::payload_hash(
            &request.task_id,
            &request.locale,
            &request.mode,
            &request.provider,
            request.model.as_deref(),
            &payload,
        );
        if current_hash != request.payload_hash {
            return Err(AppError::InvalidRequest(
                "deep review evidence changed; preview it again".into(),
            ));
        }
        let (generated_title, content) = deep_review::run(
            &request.mode,
            &request.provider,
            request.model.as_deref(),
            &request.locale,
            &payload,
            &work_dir,
        )?;
        database.save_deep_review(
            &request.task_id,
            &request.locale,
            if generated_title.trim().is_empty() {
                fallback_title
            } else {
                generated_title
            },
            content,
        )
    })
    .await
    .map_err(|error| AppError::InvalidRequest(error.to_string()))?
}

#[tauri::command]
async fn accept_review(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || database.accept_review(&id))
        .await
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?
}

#[tauri::command]
async fn update_review(state: State<'_, AppState>, request: UpdateReviewRequest) -> AppResult<()> {
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || database.update_review(&request))
        .await
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?
}

#[tauri::command]
async fn delete_review(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || database.delete_review(&id))
        .await
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?
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
        let today_tasks = database.tasks("today")?;
        let worth_reviewing = today_tasks
            .iter()
            .filter(|task| task.worth_reviewing)
            .count() as u64;
        Ok(MenuBarSnapshot {
            generated_at: Utc::now().to_rfc3339(),
            today_usage,
            today_cost_usd,
            heatmap,
            providers,
            index_status,
            today_tasks: today_tasks.into_iter().take(3).collect(),
            worth_reviewing,
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
            ("deepReviewMode", "cli"),
            ("deepReviewProvider", "codex"),
            ("deepReviewModel", ""),
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
async fn set_app_setting(state: State<'_, AppState>, key: String, value: String) -> AppResult<()> {
    validate_setting(&key, &value)?;
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
        | "launchAtLogin" => {
            matches!(value, "true" | "false")
        }
        "retentionDays" => matches!(value, "30" | "90" | "180" | "365" | "730"),
        "deepReviewMode" => matches!(value, "cli" | "api"),
        "deepReviewProvider" => matches!(value, "codex" | "claude" | "openai" | "anthropic"),
        "deepReviewModel" => {
            value.len() <= 96
                && value.chars().all(|character| {
                    character.is_ascii_alphanumeric()
                        || matches!(character, '-' | '_' | '.' | ':' | '/')
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

#[tauri::command]
fn show_main_window(app: AppHandle) -> AppResult<()> {
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
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            #[cfg(desktop)]
            app.handle().plugin(tauri_plugin_autostart::init(
                tauri_plugin_autostart::MacosLauncher::LaunchAgent,
                Some(vec!["--autostart"]),
            ))?;
            let data_dir = app.path().app_data_dir()?;
            #[cfg(debug_assertions)]
            let database_path = std::env::var("AFTERVIBE_TEST_DB")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|_| data_dir.join("aftervibe.sqlite"));
            #[cfg(not(debug_assertions))]
            let database_path = data_dir.join("aftervibe.sqlite");
            let database = Database::open(database_path)?;
            let index_status = Arc::new(RwLock::new(IndexStatus::default()));
            let providers = ProviderStore::new(data_dir.join("ProviderProbe"))?;
            let state = AppState {
                database: database.clone(),
                index_status: index_status.clone(),
                providers: providers.clone(),
            };
            app.manage(state);
            tray::setup(app)?;

            #[cfg(debug_assertions)]
            if std::env::var_os("AFTERVIBE_PREVIEW_MENUBAR").is_some() {
                if let Some(main) = app.get_webview_window("main") {
                    let _ = main.hide();
                }
                if let Some(menubar) = app.get_webview_window("menubar") {
                    let _ = menubar.set_position(tauri::PhysicalPosition::new(500, 80));
                    let _ = menubar.show();
                    let _ = menubar.set_focus();
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
            }
            if window.label() == "menubar"
                && let WindowEvent::Focused(false) = event
            {
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_overview,
            get_today,
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
            get_reviews,
            generate_review,
            preview_deep_review,
            generate_deep_review,
            accept_review,
            update_review,
            delete_review,
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
        Err(error) => eprintln!("aftervibe failed to start: {error}"),
    }
}
