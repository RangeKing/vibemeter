//! A window-local edge accessory. No global input hooks or activity recording.
use serde::Serialize;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::{Emitter, LogicalSize, Manager, PhysicalPosition, WebviewUrl};
#[cfg(target_os = "macos")]
use tauri_nspanel::{CollectionBehavior, ManagerExt, PanelBuilder, PanelLevel, StyleMask};

const WIDTH: f64 = 240.0;
const FOLDED_WIDTH: f64 = 47.0;
/// A floor, so a panel is never too small to receive a pointer.
const MIN_PANEL_HEIGHT: f64 = 80.0;
/// Clear of the work area's own edges, so the panel is never flush against the
/// menu bar or the Dock.
const PANEL_MARGIN: f64 = 24.0;
const SETTLE_MS: u64 = 700;
/// Centred until the user drags it.
const DEFAULT_OFFSET: f64 = 0.5;
/// Until the page reports its own, which it does as soon as it has rings.
const DEFAULT_NOTCH_HEIGHT: f64 = 220.0;
/// Likewise for the card, which the page measures once it is laid out.
const DEFAULT_CARD_HEIGHT: f64 = 260.0;

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EdgeState {
    pub enabled: bool,
    pub expanded: bool,
    pub pinned: bool,
    pub side: String,
    /// Where along the display's travel the notch's centre sits, 0 at the top
    /// and 1 at the bottom. The panel is much taller than the notch, so this
    /// places the notch and lets the empty, click-through remainder of the
    /// panel hang off the screen if it must.
    pub offset: f64,
    #[serde(skip)]
    notch_height: f64,
    #[serde(skip)]
    card_height: f64,
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
            offset: DEFAULT_OFFSET,
            notch_height: DEFAULT_NOTCH_HEIGHT,
            card_height: DEFAULT_CARD_HEIGHT,
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

pub fn setup(app: &tauri::AppHandle, enabled: bool, side: &str, offset: f64) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let panel = PanelBuilder::<_, VibeMeterEdgePanel>::new(app, "edge")
            .url(WebviewUrl::App("index.html?surface=edge".into()))
            .title("VibeMeter")
            .size(tauri::Size::Logical(LogicalSize::new(
                FOLDED_WIDTH,
                DEFAULT_NOTCH_HEIGHT,
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
    .inner_size(FOLDED_WIDTH, DEFAULT_NOTCH_HEIGHT)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .resizable(false)
    .visible(false)
    .build()?;
    configure(app, Some(enabled), Some(side), Some(offset))?;
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
    offset: Option<f64>,
) -> tauri::Result<()> {
    {
        let mut s = state_lock().lock().expect("edge state");
        if let Some(enabled) = enabled {
            s.enabled = enabled;
        }
        if let Some(side) = side {
            s.side = side.into();
        }
        if let Some(offset) = offset {
            s.offset = offset.clamp(0.0, 1.0);
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

/// Restores a saved position without touching whether the panel is open.
pub fn set_offset(app: &tauri::AppHandle, offset: f64) -> tauri::Result<()> {
    state_lock().lock().expect("edge state").offset = offset.clamp(0.0, 1.0);
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let _ = layout(&handle);
        let _ = handle.emit_to("edge", "edge-state", state());
    })
}

/// Moves the notch, and records how long it is.
///
/// `center_y` is a screen coordinate — where the page wants the middle of the
/// notch to land — because the page has no idea where the display's work area
/// starts or ends. It is turned into an offset here, against the travel the
/// display actually allows, and clamped so the notch cannot be dragged past
/// either end and stranded out of reach.
pub fn set_placement(
    app: &tauri::AppHandle,
    center_y: Option<f64>,
    notch_height: Option<f64>,
    card_height: Option<f64>,
) -> tauri::Result<()> {
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let Some(window) = handle.get_webview_window("edge") else {
            return;
        };
        let Ok(Some(monitor)) = window.primary_monitor() else {
            return;
        };
        let scale = monitor.scale_factor();
        let area = monitor.work_area();
        let origin = area.position.y as f64 / scale;
        let available = area.size.height as f64 / scale;
        let changed = {
            let mut s = state_lock().lock().expect("edge state");
            if let Some(height) = notch_height {
                s.notch_height = height.max(1.0);
            }
            if let Some(height) = card_height {
                s.card_height = height.max(1.0);
            }
            if let Some(center) = center_y {
                s.offset = offset_for_center(center, origin, available, s.notch_height);
            }
            center_y.is_some() || notch_height.is_some() || card_height.is_some()
        };
        if changed {
            let _ = layout(&handle);
            let _ = handle.emit_to("edge", "edge-state", state());
        }
    })
}

/// The screen coordinates the notch's centre can occupy, given how long it is.
fn travel(origin: f64, available: f64, notch_height: f64) -> (f64, f64) {
    let half = (notch_height.min(available) / 2.0).max(0.0);
    (origin + half, origin + available - half)
}

fn offset_for_center(center: f64, origin: f64, available: f64, notch_height: f64) -> f64 {
    let (top, bottom) = travel(origin, available, notch_height);
    if bottom <= top {
        return DEFAULT_OFFSET;
    }
    ((center - top) / (bottom - top)).clamp(0.0, 1.0)
}

/// Top of the panel, so the notch's centre lands at `offset` of its travel.
fn vertical_origin(
    origin: f64,
    available: f64,
    panel_height: f64,
    notch_height: f64,
    offset: f64,
) -> f64 {
    let (top, bottom) = travel(origin, available, notch_height);
    let center = top + (bottom - top).max(0.0) * offset.clamp(0.0, 1.0);
    // macOS moves a window back onto the display rather than placing it partly
    // off, and the notch would come back with it. Keeping the panel inside the
    // work area means the position asked for is the position given — which is
    // also why the panel is only as tall as its content: a panel taller than
    // the notch can only be dragged across `work area - panel`, and a fixed
    // tall one left the notch stuck in a band around the middle.
    (center - panel_height / 2.0).clamp(origin, origin + (available - panel_height).max(0.0))
}

/// The panel is exactly as tall as what it has to show: the notch when folded,
/// and whichever of the notch and the card is taller when open.
fn dimensions(
    expanded: bool,
    available_height: f64,
    notch_height: f64,
    card_height: f64,
) -> (f64, f64) {
    let ceiling = (available_height - PANEL_MARGIN).max(MIN_PANEL_HEIGHT);
    let content = if expanded {
        notch_height.max(card_height)
    } else {
        notch_height
    };
    let height = content.clamp(MIN_PANEL_HEIGHT, ceiling);
    (if expanded { WIDTH } else { FOLDED_WIDTH }, height)
}
fn horizontal_origin(side: &str, x: f64, screen_width: f64, width: f64) -> f64 {
    if side == "left" {
        x
    } else {
        x + screen_width - width
    }
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
    let available = area.size.height as f64 / scale;
    let (width, height) = dimensions(
        s.expanded || s.closing,
        available,
        s.notch_height,
        s.card_height,
    );
    let x = horizontal_origin(
        &s.side,
        area.position.x as f64,
        area.size.width as f64,
        width * scale,
    );
    let y = vertical_origin(
        area.position.y as f64 / scale,
        available,
        height,
        s.notch_height,
        s.offset,
    ) * scale;
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
    fn the_panel_is_only_as_tall_as_what_it_shows() {
        // Folded, it is the notch. Open, whichever of the notch and card is
        // taller. Anything bigger costs travel: the notch is centred in the
        // panel, so it can only move across `work area - panel`.
        assert_eq!(dimensions(false, 900.0, 276.0, 260.0), (47.0, 276.0));
        assert_eq!(dimensions(true, 900.0, 276.0, 260.0), (240.0, 276.0));
        assert_eq!(dimensions(true, 900.0, 117.0, 260.0), (240.0, 260.0));
        // A display too short for the content clamps, rather than hanging the
        // ends off where they cannot be reached.
        assert_eq!(dimensions(false, 400.0, 541.0, 260.0), (47.0, 376.0));
    }

    #[test]
    fn a_content_sized_panel_can_be_dragged_the_whole_way_down_the_edge() {
        // 900pt of work area from 25, a 276pt notch, a panel that matches it:
        // the notch's centre reaches 163 and 787, which is the full travel.
        let (origin, available, notch) = (25.0, 900.0, 276.0);
        let panel = dimensions(false, available, notch, 260.0).1;
        let top = vertical_origin(origin, available, panel, notch, 0.0) + panel / 2.0;
        let bottom = vertical_origin(origin, available, panel, notch, 1.0) + panel / 2.0;
        assert_eq!(top, 163.0);
        assert_eq!(bottom, 787.0);
        assert_eq!(bottom - top, available - notch);

        // The 720pt panel this replaced could only carry the notch across 204
        // of those 624 points, which is what made it feel stuck.
        let fixed = vertical_origin(origin, available, 720.0, notch, 1.0)
            - vertical_origin(origin, available, 720.0, notch, 0.0);
        assert!(fixed < 250.0);
    }
    #[test]
    fn placement_respects_display_origin_and_both_edges() {
        assert_eq!(horizontal_origin("left", -1920.0, 1920.0, 240.0), -1920.0);
        assert_eq!(horizontal_origin("right", -1920.0, 1920.0, 240.0), -240.0);
    }

    #[test]
    fn dragging_moves_the_notch_and_never_past_either_end() {
        // A 900pt work area starting at 25, a 220pt notch and a 720pt panel:
        // the notch's centre can travel between 135 and 815.
        let (origin, available, notch, panel) = (25.0, 900.0, 220.0, 720.0);
        assert_eq!(offset_for_center(475.0, origin, available, notch), 0.5);
        assert_eq!(offset_for_center(135.0, origin, available, notch), 0.0);
        // Dragged well past the bottom of the display, it stops at the end
        // rather than leaving the notch somewhere unreachable.
        assert_eq!(offset_for_center(4000.0, origin, available, notch), 1.0);

        // A panel matching the notch reaches both ends of that travel.
        let fitted = vertical_origin(origin, available, notch, notch, 0.0);
        assert_eq!(fitted + notch / 2.0, 135.0);
        assert_eq!(
            vertical_origin(origin, available, notch, notch, 1.0) + notch / 2.0,
            815.0
        );

        // A panel taller than the notch cannot: it is held inside the work
        // area, which pulls the notch back with it. Hence `dimensions` sizing
        // the panel to its content rather than to a fixed height.
        let centred = vertical_origin(origin, available, panel, notch, 0.0) + panel / 2.0;
        assert!(centred > 135.0);
        assert_eq!(
            vertical_origin(origin, available, panel, notch, 0.0),
            origin
        );
    }

    #[test]
    fn a_notch_taller_than_the_display_still_lands_on_it() {
        // No travel to speak of: every offset has to resolve to the same place
        // rather than dividing by a zero-width range.
        assert_eq!(offset_for_center(500.0, 0.0, 400.0, 900.0), DEFAULT_OFFSET);
        assert_eq!(vertical_origin(0.0, 400.0, 400.0, 900.0, 1.0), 0.0);
    }
}
