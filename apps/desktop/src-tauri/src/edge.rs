//! A window-local edge accessory. No global input hooks or activity recording.
use serde::Serialize;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::{Emitter, LogicalSize, Manager, PhysicalPosition, WebviewUrl};
#[cfg(target_os = "macos")]
use tauri_nspanel::{CollectionBehavior, ManagerExt, PanelBuilder, PanelLevel, StyleMask};

const WIDTH: f64 = 300.0;
const HEIGHT: f64 = 720.0;
const FOLDED_WIDTH: f64 = 58.0;
/// Tall enough for a ring per supported agent; the notch itself is only as
/// long as the rings it carries, and the rest of the panel stays click-through.
const FOLDED_HEIGHT: f64 = 720.0;
const SETTLE_MS: u64 = 700;

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EdgeState {
    pub enabled: bool,
    pub expanded: bool,
    pub pinned: bool,
    pub side: String,
    #[serde(skip)]
    generation: u64,
    #[serde(skip)]
    closing: bool,
}
static STATE: OnceLock<Mutex<EdgeState>> = OnceLock::new();
fn state_lock() -> &'static Mutex<EdgeState> {
    STATE.get_or_init(|| {
        Mutex::new(EdgeState {
            side: "right".into(),
            ..Default::default()
        })
    })
}
pub fn state() -> EdgeState {
    state_lock().lock().expect("edge state").clone()
}

#[cfg(target_os = "macos")]
tauri_nspanel::tauri_panel! {
    panel!(VibeMeterEdgePanel {
        config: {
            can_become_key_window: true,
            can_become_main_window: false,
            is_floating_panel: true
        }
    })
}

pub fn setup(app: &tauri::AppHandle, enabled: bool, side: &str) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let panel = PanelBuilder::<_, VibeMeterEdgePanel>::new(app, "edge")
            .url(WebviewUrl::App("index.html?surface=edge".into()))
            .title("VibeMeter")
            .size(tauri::Size::Logical(LogicalSize::new(
                FOLDED_WIDTH,
                FOLDED_HEIGHT,
            )))
            .level(PanelLevel::Floating)
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
                    .shadow(false)
                    .skip_taskbar(true)
                    .visible(false)
            })
            .build()?;
        panel.set_becomes_key_only_if_needed(true);
    }
    #[cfg(not(target_os = "macos"))]
    tauri::WebviewWindowBuilder::new(
        app,
        "edge",
        WebviewUrl::App("index.html?surface=edge".into()),
    )
    .inner_size(FOLDED_WIDTH, FOLDED_HEIGHT)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .resizable(false)
    .visible(false)
    .build()?;
    configure(app, Some(enabled), Some(side))?;
    // Recheck display geometry without polling input, so unplugging a monitor
    // cannot strand the accessory off-screen. All AppKit work stays on main.
    let handle = app.clone();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(2));
            let app = handle.clone();
            let _ = handle.run_on_main_thread(move || {
                if state().enabled {
                    let _ = layout(&app);
                }
            });
        }
    });
    Ok(())
}

pub fn configure(
    app: &tauri::AppHandle,
    enabled: Option<bool>,
    side: Option<&str>,
) -> tauri::Result<()> {
    {
        let mut s = state_lock().lock().expect("edge state");
        if let Some(enabled) = enabled {
            s.enabled = enabled;
        }
        if let Some(side) = side {
            s.side = side.into();
        }
        s.closing = false;
        s.expanded = false;
        s.pinned = false;
        s.generation += 1;
    }
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let _ = layout(&handle);
        if let Some(window) = handle.get_webview_window("edge") {
            #[cfg(target_os = "macos")]
            if let Ok(panel) = handle.get_webview_panel("edge") {
                if state().enabled {
                    panel.show();
                } else {
                    panel.hide();
                }
            }
            #[cfg(not(target_os = "macos"))]
            if state().enabled {
                let _ = window.show();
            } else {
                let _ = window.hide();
            }
            let _ = window.emit("edge-state", state());
        }
    })
}

pub fn set_expanded(
    app: &tauri::AppHandle,
    expanded: bool,
    pinned: Option<bool>,
) -> tauri::Result<()> {
    let generation = {
        let mut s = state_lock().lock().expect("edge state");
        if !s.enabled {
            return Ok(());
        }
        if let Some(pinned) = pinned {
            s.pinned = pinned;
        }
        s.expanded = expanded || s.pinned;
        s.closing = !s.expanded;
        s.generation += 1;
        s.generation
    };
    let handle = app.clone();
    app.run_on_main_thread(move || {
        if state().expanded {
            let _ = layout(&handle);
        }
        let _ = handle.emit_to("edge", "edge-state", state());
    })?;
    if !state().expanded {
        let handle = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(SETTLE_MS));
            let app = handle.clone();
            let _ = handle.run_on_main_thread(move || {
                if state().generation == generation {
                    state_lock().lock().expect("edge state").closing = false;
                    let _ = layout(&app);
                }
            });
        });
    }
    Ok(())
}

fn dimensions(expanded: bool, available_height: f64) -> (f64, f64) {
    // Both states clamp to the display: a panel taller than the work area
    // would put the first or last ring off-screen with no way to reach it.
    let height = HEIGHT.min((available_height - 32.0).max(200.0));
    if expanded {
        (WIDTH, height)
    } else {
        (FOLDED_WIDTH, FOLDED_HEIGHT.min(height))
    }
}
fn position(
    side: &str,
    x: f64,
    y: f64,
    screen_width: f64,
    screen_height: f64,
    width: f64,
    height: f64,
) -> (f64, f64) {
    (
        if side == "left" {
            x
        } else {
            x + screen_width - width
        },
        y + (screen_height - height) / 2.0,
    )
}
fn layout(app: &tauri::AppHandle) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window("edge") else {
        return Ok(());
    };
    let Some(monitor) = window.primary_monitor()? else {
        return Ok(());
    };
    let s = state();
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let (width, height) = dimensions(s.expanded || s.closing, area.size.height as f64 / scale);
    let (x, y) = position(
        &s.side,
        area.position.x as f64,
        area.position.y as f64,
        area.size.width as f64,
        area.size.height as f64,
        width * scale,
        height * scale,
    );
    let size = window.inner_size()?;
    if size.width != (width * scale).round() as u32
        || size.height != (height * scale).round() as u32
    {
        window.set_size(LogicalSize::new(width, height))?;
        #[cfg(target_os = "macos")]
        if let Ok(panel) = app.get_webview_panel("edge") {
            panel.set_content_size(width, height);
        }
    }
    let target = PhysicalPosition::new(x.round() as i32, y.round() as i32);
    if window.outer_position()? != target {
        window.set_position(target)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn folded_window_does_not_intercept_the_hidden_sidebar() {
        assert_eq!(dimensions(false, 900.0), (58.0, 720.0));
        assert_eq!(dimensions(false, 600.0), (58.0, 568.0));
        assert_eq!(dimensions(true, 500.0), (300.0, 468.0));
    }
    #[test]
    fn placement_respects_display_origin_and_both_edges() {
        assert_eq!(
            position("left", -1920.0, 100.0, 1920.0, 1080.0, 300.0, 580.0),
            (-1920.0, 350.0)
        );
        assert_eq!(
            position("right", -1920.0, 100.0, 1920.0, 1080.0, 300.0, 580.0),
            (-300.0, 350.0)
        );
    }
}
