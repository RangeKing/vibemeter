use tauri::image::Image;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};

const POPOVER_WIDTH: f64 = 436.0;
const POPOVER_HEIGHT: f64 = 646.0;

pub fn setup(app: &mut tauri::App) -> tauri::Result<()> {
    WebviewWindowBuilder::new(
        app,
        "menubar",
        WebviewUrl::App("index.html?surface=menubar".into()),
    )
    .title("aftervibe")
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

    let icon = Image::from_bytes(include_bytes!("../icons/tray-template.png"))?;
    TrayIconBuilder::with_id("aftervibe-tray")
        .icon(icon)
        .icon_as_template(true)
        .tooltip("aftervibe")
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
    Ok(())
}
