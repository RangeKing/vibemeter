use std::sync::atomic::{AtomicBool, Ordering};
use tauri::image::Image;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{LogicalSize, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};

const POPOVER_WIDTH: f64 = 436.0;
const POPOVER_HEIGHT: f64 = 646.0;
const NOTCH_COLLAPSED_WIDTH: f64 = 354.0;
const NOTCH_COLLAPSED_HEIGHT: f64 = 42.0;
const NOTCH_EXPANDED_WIDTH: f64 = 430.0;
const NOTCH_EXPANDED_HEIGHT: f64 = 304.0;
static NOTCH_AVAILABLE: AtomicBool = AtomicBool::new(false);

pub fn setup(
    app: &mut tauri::App,
    notch_enabled: bool,
    menu_bar_enabled: bool,
) -> tauri::Result<()> {
    NOTCH_AVAILABLE.store(detect_notch(), Ordering::SeqCst);
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

    WebviewWindowBuilder::new(
        app,
        "notch",
        WebviewUrl::App("index.html?surface=notch".into()),
    )
    .title("VibeMeter Live")
    .inner_size(NOTCH_COLLAPSED_WIDTH, NOTCH_COLLAPSED_HEIGHT)
    .min_inner_size(NOTCH_COLLAPSED_WIDTH, NOTCH_COLLAPSED_HEIGHT)
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
        if let Some(window) = app.get_webview_window("notch") {
            window.show()?;
        }
    }
    Ok(())
}

pub fn set_notch_expanded(app: &tauri::AppHandle, expanded: bool) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window("notch") else {
        return Ok(());
    };
    let (width, height) = if expanded {
        (NOTCH_EXPANDED_WIDTH, NOTCH_EXPANDED_HEIGHT)
    } else {
        (NOTCH_COLLAPSED_WIDTH, NOTCH_COLLAPSED_HEIGHT)
    };
    window.set_size(LogicalSize::new(width, height))?;
    if let Some(monitor) = window.current_monitor()?.or(window.primary_monitor()?) {
        let scale = monitor.scale_factor();
        let physical_width = width * scale;
        let x = monitor.position().x as f64 + (monitor.size().width as f64 - physical_width) / 2.0;
        window.set_position(PhysicalPosition::new(
            x.round() as i32,
            monitor.position().y,
        ))?;
    }
    Ok(())
}

pub fn set_notch_enabled(app: &tauri::AppHandle, enabled: bool) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window("notch") else {
        return Ok(());
    };
    if enabled && NOTCH_AVAILABLE.load(Ordering::SeqCst) {
        set_notch_expanded(app, false)?;
        window.show()
    } else {
        window.hide()
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

#[cfg(target_os = "macos")]
fn detect_notch() -> bool {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSScreen;

    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    NSScreen::mainScreen(mtm).is_some_and(|screen| {
        let insets = screen.safeAreaInsets();
        let left = screen.auxiliaryTopLeftArea();
        let right = screen.auxiliaryTopRightArea();
        insets.top > 0.0 && left.size.width > 0.0 && right.size.width > 0.0
    })
}

#[cfg(not(target_os = "macos"))]
fn detect_notch() -> bool {
    false
}
