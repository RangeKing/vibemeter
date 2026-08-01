// `tauri-nspanel` requires an explicit `-> ()` in delegate declarations.
#![allow(clippy::unused_unit)]

use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::image::Image;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{LogicalSize, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};

const POPOVER_WIDTH: f64 = 436.0;
const POPOVER_HEIGHT: f64 = 646.0;
const NOTCH_LEFT_WING_WIDTH: f64 = 88.0;
const NOTCH_RIGHT_WING_WIDTH: f64 = 98.0;
const NOTCH_EXPANDED_WIDTH: f64 = 440.0;
const NOTCH_EXPANDED_HEIGHT: f64 = 168.0;
const NOTCH_MIN_LEFT_WING_WIDTH: f64 = 68.0;
const NOTCH_MAX_LEFT_WING_WIDTH: f64 = 160.0;
const NOTCH_MIN_EXPANDED_HEIGHT: f64 = 150.0;
const NOTCH_SCREEN_BOTTOM_MARGIN: f64 = 16.0;
const NOTCH_HOVER_EXPAND_DELAY: Duration = Duration::from_millis(300);
const NOTCH_HOVER_COLLAPSE_DELAY: Duration = Duration::from_millis(500);
const NOTCH_COLLAPSE_ANIMATION_DURATION: Duration = Duration::from_millis(260);
static NOTCH_AVAILABLE: AtomicBool = AtomicBool::new(false);
static NOTCH_ENABLED: AtomicBool = AtomicBool::new(false);
static NOTCH_EXPANDED: AtomicBool = AtomicBool::new(false);
static NOTCH_PINNED: AtomicBool = AtomicBool::new(false);
static NOTCH_HAS_ACTIVITY: AtomicBool = AtomicBool::new(false);
static NOTCH_EXPANDED_FROM_HOVER: AtomicBool = AtomicBool::new(false);
static NOTCH_CURSOR_IN_HOVER_REGION: AtomicBool = AtomicBool::new(false);
static NOTCH_HOVER_GENERATION: AtomicU64 = AtomicU64::new(0);
static NOTCH_TRANSITION_GENERATION: AtomicU64 = AtomicU64::new(0);
static NOTCH_EVENT_MONITORS_INSTALLED: AtomicBool = AtomicBool::new(false);
static NOTCH_COLLAPSE_PENDING: AtomicBool = AtomicBool::new(false);
static NOTCH_LEFT_WING_WIDTH_BITS: AtomicU64 = AtomicU64::new(NOTCH_LEFT_WING_WIDTH.to_bits());
static NOTCH_EXPANDED_HEIGHT_BITS: AtomicU64 = AtomicU64::new(NOTCH_EXPANDED_HEIGHT.to_bits());
static NOTCH_GEOMETRY: OnceLock<Mutex<NotchGeometry>> = OnceLock::new();

#[derive(Clone, Copy, Debug)]
struct NotchGeometry {
    hardware_width: f64,
    hardware_height: f64,
    screen_x: f64,
    screen_y: f64,
    screen_width: f64,
    screen_height: f64,
}

impl Default for NotchGeometry {
    fn default() -> Self {
        Self {
            hardware_width: 180.0,
            hardware_height: 34.0,
            screen_x: 0.0,
            screen_y: 0.0,
            screen_width: 1470.0,
            screen_height: 956.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct NotchWindowLayout {
    width: f64,
    height: f64,
    left_of_hardware_center: f64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotchUiState {
    available: bool,
    enabled: bool,
    expanded: bool,
    pinned: bool,
    has_activity: bool,
    hardware_width: f64,
    hardware_height: f64,
    left_wing_width: f64,
    right_wing_width: f64,
    expanded_height: f64,
}

#[cfg(target_os = "macos")]
use tauri_nspanel::{CollectionBehavior, ManagerExt, PanelBuilder, PanelLevel, StyleMask};

#[cfg(target_os = "macos")]
tauri_nspanel::tauri_panel! {
    panel!(VibeMeterNotchPanel {
        config: {
            can_become_key_window: true,
            can_become_main_window: false,
            is_floating_panel: true
        }
    })

    panel_event!(VibeMeterNotchPanelEventHandler {
        window_did_resign_key(notification: &NSNotification) -> ()
    })
}

pub fn setup(
    app: &mut tauri::App,
    notch_enabled: bool,
    menu_bar_enabled: bool,
) -> tauri::Result<()> {
    let notch_geometry = detect_notch();
    NOTCH_AVAILABLE.store(notch_geometry.is_some(), Ordering::SeqCst);
    update_notch_enabled_state(notch_enabled && notch_geometry.is_some());
    let _ = NOTCH_GEOMETRY.set(Mutex::new(notch_geometry.unwrap_or_default()));
    WebviewWindowBuilder::new(
        app,
        "menubar",
        WebviewUrl::App("index.html?surface=menubar".into()),
    )
    .title("VibeMeter")
    .inner_size(POPOVER_WIDTH, POPOVER_HEIGHT)
    .min_inner_size(POPOVER_WIDTH, POPOVER_HEIGHT)
    .max_inner_size(POPOVER_WIDTH, POPOVER_HEIGHT)
    .resizable(false)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    // The rounded surface draws its own shadow. A native macOS window shadow
    // stays rectangular on transparent webviews and creates a visible outer box.
    .shadow(false)
    .skip_taskbar(true)
    .visible(false)
    .build()?;

    #[cfg(target_os = "macos")]
    {
        let geometry = current_geometry();
        let panel = PanelBuilder::<_, VibeMeterNotchPanel>::new(app.handle(), "notch")
            .url(WebviewUrl::App("index.html?surface=notch".into()))
            .title("VibeMeter Live")
            .size(tauri::Size::Logical(LogicalSize::new(
                geometry.hardware_width,
                geometry.hardware_height,
            )))
            .level(PanelLevel::Status)
            .has_shadow(false)
            .opaque(false)
            .transparent(true)
            .hides_on_deactivate(false)
            .works_when_modal(true)
            .released_when_closed(false)
            .collection_behavior(
                CollectionBehavior::new()
                    .can_join_all_spaces()
                    .stationary()
                    .ignores_cycle()
                    .full_screen_auxiliary(),
            )
            .style_mask(StyleMask::empty().borderless().nonactivating_panel())
            .no_activate(true)
            .with_window(|window| {
                window
                    .accept_first_mouse(true)
                    .resizable(false)
                    .decorations(false)
                    .transparent(true)
                    .background_color(tauri::window::Color(0, 0, 0, 0))
                    .always_on_top(true)
                    .shadow(false)
                    .skip_taskbar(true)
                    .visible(NOTCH_ENABLED.load(Ordering::SeqCst))
            })
            .build()?;
        let app_handle = app.handle().clone();
        let handler = VibeMeterNotchPanelEventHandler::new();
        handler.window_did_resign_key(move |_| {
            if NOTCH_EXPANDED.load(Ordering::SeqCst) && !NOTCH_PINNED.load(Ordering::SeqCst) {
                let _ = set_notch_expanded(&app_handle, false);
            }
        });
        panel.set_event_handler(Some(handler.as_ref()));
        install_notch_event_monitors(app.handle())?;
    }

    #[cfg(not(target_os = "macos"))]
    WebviewWindowBuilder::new(
        app,
        "notch",
        WebviewUrl::App("index.html?surface=notch".into()),
    )
    .title("VibeMeter Live")
    .inner_size(
        current_geometry().hardware_width + current_left_wing_width() + NOTCH_RIGHT_WING_WIDTH,
        current_geometry().hardware_height,
    )
    .resizable(false)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .shadow(false)
    .skip_taskbar(true)
    .visible(false)
    .build()?;

    let icon = Image::from_bytes(include_bytes!("../icons/tray-template.png"))?;
    TrayIconBuilder::with_id("vibemeter-tray")
        .icon(icon)
        .icon_as_template(true)
        .tooltip("VibeMeter")
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            let TrayIconEvent::Click {
                rect,
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            else {
                return;
            };
            let app = tray.app_handle();
            let Some(window) = app.get_webview_window("menubar") else {
                return;
            };
            if window.is_visible().unwrap_or(false) {
                let _ = window.hide();
                return;
            }
            let scale = window.scale_factor().unwrap_or(2.0);
            let position = rect.position.to_physical::<f64>(scale);
            let size = rect.size.to_physical::<f64>(scale);
            let physical_width = POPOVER_WIDTH * scale;
            let x = (position.x + size.width - physical_width).max(8.0);
            let y = position.y + size.height + 7.0 * scale;
            let _ = window.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
            let _ = window.show();
            let _ = window.set_focus();
        })
        .build(app)?;
    set_menu_bar_enabled(app.handle(), menu_bar_enabled)?;
    if notch_enabled && NOTCH_AVAILABLE.load(Ordering::SeqCst) {
        set_notch_expanded(app.handle(), false)?;
        show_notch(app.handle())?;
    } else {
        hide_notch(app.handle())?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn install_notch_event_monitors(app: &tauri::AppHandle) -> tauri::Result<()> {
    use block2::RcBlock;
    use objc2_app_kit::{NSEvent, NSEventMask};
    use std::ptr::NonNull;

    if NOTCH_EVENT_MONITORS_INSTALLED.swap(true, Ordering::SeqCst) {
        return Ok(());
    }

    let click_app_handle = app.clone();
    let click_handler: RcBlock<dyn Fn(NonNull<NSEvent>)> = RcBlock::new(move |_| {
        if !NOTCH_EXPANDED.load(Ordering::SeqCst) {
            return;
        }
        if point_in_expanded_panel(current_geometry(), mouse_location()) {
            // A click inside a hover-opened panel is deliberate interaction, not
            // an outside-click dismissal. Promote it to a key panel so the same
            // pointer sequence reaches the webview controls reliably.
            if NOTCH_EXPANDED_FROM_HOVER.load(Ordering::SeqCst) {
                let _ = set_notch_expanded_internal(&click_app_handle, true, false);
            }
            return;
        }
        if !NOTCH_PINNED.load(Ordering::SeqCst) {
            let _ = set_notch_expanded(&click_app_handle, false);
        }
    });
    let click_mask =
        NSEventMask::LeftMouseDown | NSEventMask::RightMouseDown | NSEventMask::OtherMouseDown;
    let Some(click_monitor) =
        NSEvent::addGlobalMonitorForEventsMatchingMask_handler(click_mask, &click_handler)
    else {
        NOTCH_EVENT_MONITORS_INSTALLED.store(false, Ordering::SeqCst);
        return Err(std::io::Error::other("failed to install Notch click monitor").into());
    };

    let global_hover_app_handle = app.clone();
    let global_hover_handler: RcBlock<dyn Fn(NonNull<NSEvent>)> = RcBlock::new(move |_| {
        handle_notch_mouse_location(&global_hover_app_handle, mouse_location());
    });
    let Some(global_hover_monitor) = NSEvent::addGlobalMonitorForEventsMatchingMask_handler(
        NSEventMask::MouseMoved,
        &global_hover_handler,
    ) else {
        NOTCH_EVENT_MONITORS_INSTALLED.store(false, Ordering::SeqCst);
        return Err(std::io::Error::other("failed to install Notch hover monitor").into());
    };

    let local_hover_app_handle = app.clone();
    let local_hover_handler: RcBlock<dyn Fn(NonNull<NSEvent>) -> *mut NSEvent> =
        RcBlock::new(move |event: NonNull<NSEvent>| {
            handle_notch_mouse_location(&local_hover_app_handle, mouse_location());
            event.as_ptr()
        });
    let Some(local_hover_monitor) = (unsafe {
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(
            NSEventMask::MouseMoved,
            &local_hover_handler,
        )
    }) else {
        NOTCH_EVENT_MONITORS_INSTALLED.store(false, Ordering::SeqCst);
        return Err(std::io::Error::other("failed to install local Notch hover monitor").into());
    };

    // AppKit owns these monitors for the process lifetime. Keeping them installed avoids
    // re-registering global callbacks every time the lightweight panel changes shape.
    std::mem::forget(click_monitor);
    std::mem::forget(global_hover_monitor);
    std::mem::forget(local_hover_monitor);
    Ok(())
}

#[cfg(target_os = "macos")]
fn mouse_location() -> (f64, f64) {
    use objc2_app_kit::NSEvent;

    let point = NSEvent::mouseLocation();
    (point.x, point.y)
}

#[cfg(target_os = "macos")]
fn handle_notch_mouse_location(app: &tauri::AppHandle, point: (f64, f64)) {
    if !NOTCH_ENABLED.load(Ordering::SeqCst) {
        return;
    }
    let geometry = current_geometry();
    let expanded = NOTCH_EXPANDED.load(Ordering::SeqCst);
    let inside = if expanded {
        point_in_expanded_panel(geometry, point)
    } else {
        point_in_collapsed_hover_region(geometry, point, NOTCH_HAS_ACTIVITY.load(Ordering::SeqCst))
    };
    let was_inside = NOTCH_CURSOR_IN_HOVER_REGION.swap(inside, Ordering::SeqCst);

    if inside {
        if !was_inside {
            let generation = NOTCH_HOVER_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
            if !expanded {
                schedule_hover_expand(app.clone(), generation);
            }
        }
        return;
    }

    if was_inside {
        let generation = NOTCH_HOVER_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
        if expanded && NOTCH_EXPANDED_FROM_HOVER.load(Ordering::SeqCst) {
            schedule_hover_collapse(app.clone(), generation);
        }
    }
}

#[cfg(target_os = "macos")]
fn schedule_hover_expand(app: tauri::AppHandle, generation: u64) {
    std::thread::spawn(move || {
        std::thread::sleep(NOTCH_HOVER_EXPAND_DELAY);
        if NOTCH_ENABLED.load(Ordering::SeqCst)
            && NOTCH_HOVER_GENERATION.load(Ordering::SeqCst) == generation
            && NOTCH_CURSOR_IN_HOVER_REGION.load(Ordering::SeqCst)
            && !NOTCH_EXPANDED.load(Ordering::SeqCst)
        {
            let _ = set_notch_expanded_internal(&app, true, true);
        }
    });
}

#[cfg(target_os = "macos")]
fn schedule_hover_collapse(app: tauri::AppHandle, generation: u64) {
    std::thread::spawn(move || {
        std::thread::sleep(NOTCH_HOVER_COLLAPSE_DELAY);
        if NOTCH_HOVER_GENERATION.load(Ordering::SeqCst) == generation
            && !NOTCH_CURSOR_IN_HOVER_REGION.load(Ordering::SeqCst)
            && NOTCH_EXPANDED_FROM_HOVER.load(Ordering::SeqCst)
            && !NOTCH_PINNED.load(Ordering::SeqCst)
        {
            let _ = set_notch_expanded_internal(&app, false, false);
        }
    });
}

pub fn set_notch_expanded(app: &tauri::AppHandle, expanded: bool) -> tauri::Result<()> {
    set_notch_expanded_internal(app, expanded, false)
}

fn set_notch_expanded_internal(
    app: &tauri::AppHandle,
    expanded: bool,
    from_hover: bool,
) -> tauri::Result<()> {
    let expanded =
        expanded && NOTCH_ENABLED.load(Ordering::SeqCst) && NOTCH_AVAILABLE.load(Ordering::SeqCst);
    let transition_generation = NOTCH_TRANSITION_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    NOTCH_EXPANDED.store(expanded, Ordering::SeqCst);
    NOTCH_EXPANDED_FROM_HOVER.store(expanded && from_hover, Ordering::SeqCst);
    if !expanded {
        NOTCH_PINNED.store(false, Ordering::SeqCst);
    }
    // Let the webview animate its clipped island back into the hardware cutout
    // before the transparent native window adopts the collapsed frame.
    emit_notch_state(app);
    if !NOTCH_ENABLED.load(Ordering::SeqCst) {
        NOTCH_COLLAPSE_PENDING.store(false, Ordering::SeqCst);
        return hide_notch(app);
    }
    if expanded {
        NOTCH_COLLAPSE_PENDING.store(false, Ordering::SeqCst);
        apply_notch_geometry(app)?;
    } else {
        NOTCH_COLLAPSE_PENDING.store(true, Ordering::SeqCst);
        schedule_notch_collapse_geometry(app.clone(), transition_generation);
    }
    #[cfg(not(target_os = "macos"))]
    if let Some(window) = app.get_webview_window("notch") {
        window.show()?;
    }
    #[cfg(target_os = "macos")]
    if let Ok(panel) = app.get_webview_panel("notch") {
        app.run_on_main_thread(move || {
            if expanded && !from_hover {
                panel.show_and_make_key();
            } else if expanded {
                // A deliberate hover should reveal information without taking
                // focus from the editor or immediately triggering resign-key.
                panel.show();
            } else {
                panel.resign_key_window();
                panel.show();
            }
        })?;
    }
    Ok(())
}

fn schedule_notch_collapse_geometry(app: tauri::AppHandle, generation: u64) {
    std::thread::spawn(move || {
        std::thread::sleep(NOTCH_COLLAPSE_ANIMATION_DURATION);
        if NOTCH_TRANSITION_GENERATION.load(Ordering::SeqCst) != generation
            || NOTCH_EXPANDED.load(Ordering::SeqCst)
        {
            return;
        }
        NOTCH_COLLAPSE_PENDING.store(false, Ordering::SeqCst);
        let _ = apply_notch_geometry(&app);
    });
}

pub fn set_notch_pinned(app: &tauri::AppHandle, pinned: bool) -> tauri::Result<()> {
    let pinned =
        pinned && NOTCH_ENABLED.load(Ordering::SeqCst) && NOTCH_EXPANDED.load(Ordering::SeqCst);
    NOTCH_PINNED.store(pinned, Ordering::SeqCst);
    if pinned {
        NOTCH_EXPANDED_FROM_HOVER.store(false, Ordering::SeqCst);
    }
    emit_notch_state(app);
    Ok(())
}

pub fn set_notch_activity(app: &tauri::AppHandle, has_activity: bool) -> tauri::Result<()> {
    NOTCH_HAS_ACTIVITY.store(has_activity, Ordering::SeqCst);
    emit_notch_state(app);
    if NOTCH_ENABLED.load(Ordering::SeqCst)
        && !NOTCH_EXPANDED.load(Ordering::SeqCst)
        && !NOTCH_COLLAPSE_PENDING.load(Ordering::SeqCst)
    {
        apply_notch_geometry(app)?;
        show_notch(app)?;
    }
    Ok(())
}

pub fn set_notch_layout(
    app: &tauri::AppHandle,
    left_wing_width: f64,
    expanded_height: f64,
) -> tauri::Result<()> {
    let left_wing_width = finite_or(left_wing_width, NOTCH_LEFT_WING_WIDTH)
        .clamp(NOTCH_MIN_LEFT_WING_WIDTH, NOTCH_MAX_LEFT_WING_WIDTH);
    let expanded_height = clamp_expanded_height(expanded_height, current_geometry());
    let left_changed = NOTCH_LEFT_WING_WIDTH_BITS.swap(left_wing_width.to_bits(), Ordering::SeqCst)
        != left_wing_width.to_bits();
    let height_changed = NOTCH_EXPANDED_HEIGHT_BITS
        .swap(expanded_height.to_bits(), Ordering::SeqCst)
        != expanded_height.to_bits();
    if left_changed || height_changed {
        emit_notch_state(app);
        if !NOTCH_COLLAPSE_PENDING.load(Ordering::SeqCst) {
            apply_notch_geometry(app)?;
        }
    }
    Ok(())
}

pub fn notch_state() -> NotchUiState {
    let geometry = current_geometry();
    NotchUiState {
        available: NOTCH_AVAILABLE.load(Ordering::SeqCst),
        enabled: NOTCH_ENABLED.load(Ordering::SeqCst),
        expanded: NOTCH_EXPANDED.load(Ordering::SeqCst),
        pinned: NOTCH_PINNED.load(Ordering::SeqCst),
        has_activity: NOTCH_HAS_ACTIVITY.load(Ordering::SeqCst),
        hardware_width: geometry.hardware_width,
        hardware_height: geometry.hardware_height,
        left_wing_width: current_left_wing_width(),
        right_wing_width: NOTCH_RIGHT_WING_WIDTH,
        expanded_height: current_expanded_height(),
    }
}

pub fn set_notch_enabled(app: &tauri::AppHandle, enabled: bool) -> tauri::Result<()> {
    let enabled = enabled && NOTCH_AVAILABLE.load(Ordering::SeqCst);
    update_notch_enabled_state(enabled);
    emit_notch_state(app);
    if enabled {
        set_notch_expanded(app, false)?;
        show_notch(app)
    } else {
        hide_notch(app)
    }
}

fn update_notch_enabled_state(enabled: bool) {
    NOTCH_ENABLED.store(enabled, Ordering::SeqCst);
    NOTCH_CURSOR_IN_HOVER_REGION.store(false, Ordering::SeqCst);
    NOTCH_HOVER_GENERATION.fetch_add(1, Ordering::SeqCst);
    NOTCH_TRANSITION_GENERATION.fetch_add(1, Ordering::SeqCst);
    NOTCH_COLLAPSE_PENDING.store(false, Ordering::SeqCst);
    if !enabled {
        NOTCH_EXPANDED.store(false, Ordering::SeqCst);
        NOTCH_EXPANDED_FROM_HOVER.store(false, Ordering::SeqCst);
        NOTCH_PINNED.store(false, Ordering::SeqCst);
    }
}

pub fn set_menu_bar_enabled(app: &tauri::AppHandle, enabled: bool) -> tauri::Result<()> {
    if let Some(tray) = app.tray_by_id("vibemeter-tray") {
        tray.set_visible(enabled)?;
    }
    if !enabled && let Some(window) = app.get_webview_window("menubar") {
        window.hide()?;
    }
    Ok(())
}

fn current_geometry() -> NotchGeometry {
    NOTCH_GEOMETRY
        .get()
        .and_then(|geometry| geometry.lock().ok().map(|value| *value))
        .unwrap_or_default()
}

fn finite_or(value: f64, fallback: f64) -> f64 {
    if value.is_finite() { value } else { fallback }
}

fn clamp_expanded_height(value: f64, geometry: NotchGeometry) -> f64 {
    let screen_max =
        (geometry.screen_height - NOTCH_SCREEN_BOTTOM_MARGIN).max(NOTCH_MIN_EXPANDED_HEIGHT);
    finite_or(value, NOTCH_EXPANDED_HEIGHT).clamp(NOTCH_MIN_EXPANDED_HEIGHT, screen_max)
}

fn current_left_wing_width() -> f64 {
    f64::from_bits(NOTCH_LEFT_WING_WIDTH_BITS.load(Ordering::SeqCst))
}

fn current_expanded_height() -> f64 {
    f64::from_bits(NOTCH_EXPANDED_HEIGHT_BITS.load(Ordering::SeqCst))
}

fn apply_notch_geometry(app: &tauri::AppHandle) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window("notch") else {
        return Ok(());
    };
    let geometry = current_geometry();
    let layout = notch_window_layout(
        geometry,
        NOTCH_EXPANDED.load(Ordering::SeqCst),
        NOTCH_HAS_ACTIVITY.load(Ordering::SeqCst),
        current_left_wing_width(),
        NOTCH_RIGHT_WING_WIDTH,
        current_expanded_height(),
    );

    window.set_size(LogicalSize::new(layout.width, layout.height))?;
    #[cfg(target_os = "macos")]
    if let Ok(panel) = app.get_webview_panel("notch") {
        app.run_on_main_thread(move || panel.set_content_size(layout.width, layout.height))?;
    }
    if let Some(monitor) = window.current_monitor()?.or(window.primary_monitor()?) {
        let scale = monitor.scale_factor();
        let x = monitor.position().x as f64 + monitor.size().width as f64 / 2.0
            - layout.left_of_hardware_center * scale;
        window.set_position(PhysicalPosition::new(
            x.round() as i32,
            monitor.position().y,
        ))?;
    }
    Ok(())
}

fn notch_window_layout(
    geometry: NotchGeometry,
    expanded: bool,
    has_activity: bool,
    left_wing_width: f64,
    right_wing_width: f64,
    expanded_height: f64,
) -> NotchWindowLayout {
    if expanded {
        return NotchWindowLayout {
            width: NOTCH_EXPANDED_WIDTH,
            height: expanded_height,
            left_of_hardware_center: NOTCH_EXPANDED_WIDTH / 2.0,
        };
    }
    if has_activity {
        return NotchWindowLayout {
            width: geometry.hardware_width + left_wing_width + right_wing_width,
            height: geometry.hardware_height,
            left_of_hardware_center: geometry.hardware_width / 2.0 + left_wing_width,
        };
    }
    NotchWindowLayout {
        width: geometry.hardware_width,
        height: geometry.hardware_height,
        left_of_hardware_center: geometry.hardware_width / 2.0,
    }
}

#[cfg(target_os = "macos")]
fn point_in_collapsed_hover_region(
    geometry: NotchGeometry,
    point: (f64, f64),
    has_activity: bool,
) -> bool {
    let left_wing = if has_activity {
        current_left_wing_width()
    } else {
        0.0
    };
    let right_wing = if has_activity {
        NOTCH_RIGHT_WING_WIDTH
    } else {
        0.0
    };
    let center_x = geometry.screen_x + geometry.screen_width / 2.0;
    let min_x = center_x - geometry.hardware_width / 2.0 - left_wing;
    let max_x = center_x + geometry.hardware_width / 2.0 + right_wing;
    let max_y = geometry.screen_y + geometry.screen_height;
    let min_y = max_y - geometry.hardware_height;
    point.0 >= min_x && point.0 <= max_x && point.1 >= min_y && point.1 <= max_y
}

#[cfg(target_os = "macos")]
fn point_in_expanded_panel(geometry: NotchGeometry, point: (f64, f64)) -> bool {
    let center_x = geometry.screen_x + geometry.screen_width / 2.0;
    let min_x = center_x - NOTCH_EXPANDED_WIDTH / 2.0;
    let max_x = center_x + NOTCH_EXPANDED_WIDTH / 2.0;
    let max_y = geometry.screen_y + geometry.screen_height;
    let min_y = max_y - current_expanded_height();
    point.0 >= min_x && point.0 <= max_x && point.1 >= min_y && point.1 <= max_y
}

fn emit_notch_state(app: &tauri::AppHandle) {
    use tauri::Emitter;
    let _ = app.emit_to("notch", "notch-state", notch_state());
}

#[cfg(target_os = "macos")]
fn show_notch(app: &tauri::AppHandle) -> tauri::Result<()> {
    if !NOTCH_ENABLED.load(Ordering::SeqCst) {
        return hide_notch(app);
    }
    if let Ok(panel) = app.get_webview_panel("notch") {
        app.run_on_main_thread(move || panel.show())?;
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn show_notch(app: &tauri::AppHandle) -> tauri::Result<()> {
    if !NOTCH_ENABLED.load(Ordering::SeqCst) {
        return hide_notch(app);
    }
    if let Some(window) = app.get_webview_window("notch") {
        window.show()?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn hide_notch(app: &tauri::AppHandle) -> tauri::Result<()> {
    if let Ok(panel) = app.get_webview_panel("notch") {
        app.run_on_main_thread(move || panel.hide())?;
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn hide_notch(app: &tauri::AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window("notch") {
        window.hide()?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn detect_notch() -> Option<NotchGeometry> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSScreen;

    let mtm = MainThreadMarker::new()?;
    NSScreen::mainScreen(mtm).and_then(|screen| {
        let insets = screen.safeAreaInsets();
        let left = screen.auxiliaryTopLeftArea();
        let right = screen.auxiliaryTopRightArea();
        if insets.top <= 0.0 || left.size.width <= 0.0 || right.size.width <= 0.0 {
            return None;
        }
        let frame = screen.frame();
        let hardware_width =
            (frame.size.width - left.size.width - right.size.width).clamp(120.0, 260.0);
        Some(NotchGeometry {
            hardware_width,
            hardware_height: insets.top.clamp(28.0, 52.0),
            screen_x: frame.origin.x,
            screen_y: frame.origin.y,
            screen_width: frame.size.width,
            screen_height: frame.size.height,
        })
    })
}

#[cfg(not(target_os = "macos"))]
fn detect_notch() -> Option<NotchGeometry> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapsed_layout_keeps_the_physical_notch_centered_with_asymmetric_wings() {
        let geometry = NotchGeometry {
            hardware_width: 179.0,
            hardware_height: 32.0,
            ..NotchGeometry::default()
        };
        let layout = notch_window_layout(
            geometry,
            false,
            true,
            NOTCH_LEFT_WING_WIDTH,
            NOTCH_RIGHT_WING_WIDTH,
            NOTCH_EXPANDED_HEIGHT,
        );
        assert_eq!(layout.width, 365.0);
        assert_eq!(layout.height, geometry.hardware_height);
        assert_eq!(
            layout.left_of_hardware_center,
            NOTCH_LEFT_WING_WIDTH + geometry.hardware_width / 2.0
        );
        assert_eq!(
            layout.width - layout.left_of_hardware_center,
            geometry.hardware_width / 2.0 + NOTCH_RIGHT_WING_WIDTH
        );
    }

    #[test]
    fn expanded_layout_uses_the_content_driven_height_and_stays_top_centered() {
        let geometry = NotchGeometry::default();
        let layout =
            notch_window_layout(geometry, true, true, 132.0, NOTCH_RIGHT_WING_WIDTH, 186.0);
        assert_eq!(layout.width, NOTCH_EXPANDED_WIDTH);
        assert_eq!(layout.height, 186.0);
        assert_eq!(layout.left_of_hardware_center, NOTCH_EXPANDED_WIDTH / 2.0);
    }

    #[test]
    fn expanded_height_grows_with_content_until_the_screen_bottom_margin() {
        let geometry = NotchGeometry {
            screen_height: 500.0,
            ..NotchGeometry::default()
        };
        assert_eq!(clamp_expanded_height(429.0, geometry), 429.0);
        assert_eq!(clamp_expanded_height(900.0, geometry), 484.0);
    }

    #[test]
    fn disabling_notch_cancels_pending_hover_and_expansion_state() {
        NOTCH_EXPANDED.store(true, Ordering::SeqCst);
        NOTCH_EXPANDED_FROM_HOVER.store(true, Ordering::SeqCst);
        NOTCH_PINNED.store(true, Ordering::SeqCst);
        NOTCH_CURSOR_IN_HOVER_REGION.store(true, Ordering::SeqCst);
        NOTCH_COLLAPSE_PENDING.store(true, Ordering::SeqCst);
        let hover_generation = NOTCH_HOVER_GENERATION.load(Ordering::SeqCst);
        let transition_generation = NOTCH_TRANSITION_GENERATION.load(Ordering::SeqCst);

        update_notch_enabled_state(false);

        assert!(!NOTCH_ENABLED.load(Ordering::SeqCst));
        assert!(!NOTCH_EXPANDED.load(Ordering::SeqCst));
        assert!(!NOTCH_EXPANDED_FROM_HOVER.load(Ordering::SeqCst));
        assert!(!NOTCH_PINNED.load(Ordering::SeqCst));
        assert!(!NOTCH_CURSOR_IN_HOVER_REGION.load(Ordering::SeqCst));
        assert!(!NOTCH_COLLAPSE_PENDING.load(Ordering::SeqCst));
        assert!(NOTCH_HOVER_GENERATION.load(Ordering::SeqCst) > hover_generation);
        assert!(NOTCH_TRANSITION_GENERATION.load(Ordering::SeqCst) > transition_generation);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn hover_region_matches_the_visible_wings_and_physical_notch() {
        let geometry = NotchGeometry {
            hardware_width: 179.0,
            hardware_height: 32.0,
            ..NotchGeometry::default()
        };
        let hardware_left =
            geometry.screen_x + geometry.screen_width / 2.0 - geometry.hardware_width / 2.0;
        let visible_left = hardware_left - NOTCH_LEFT_WING_WIDTH;
        assert!(point_in_collapsed_hover_region(
            geometry,
            (735.0, 950.0),
            true
        ));
        assert!(point_in_collapsed_hover_region(
            geometry,
            (visible_left + 1.0, 950.0),
            true
        ));
        assert!(point_in_collapsed_hover_region(
            geometry,
            (922.0, 950.0),
            true
        ));
        assert!(!point_in_collapsed_hover_region(
            geometry,
            (visible_left - 1.0, 950.0),
            true
        ));
        assert!(!point_in_collapsed_hover_region(
            geometry,
            (924.0, 950.0),
            true
        ));
        assert!(!point_in_collapsed_hover_region(
            geometry,
            (visible_left + 1.0, 950.0),
            false
        ));
    }
}
