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
    /// Where the notch sits inside the panel, in points down from its top. The
    /// panel is held inside the work area, so near the ends of the travel it
    /// can no longer be centred on the notch; the notch shifts within it
    /// instead, and reaches the edge of the display either way.
    pub notch_top: f64,
    #[serde(skip)]
    notch_height: f64,
    #[serde(skip)]
    card_height: f64,
    #[serde(skip)]
    generation: u64,
    #[serde(skip)]
    closing: bool,
}
/// A press being tracked from AppKit rather than from the page. See `begin_drag`.
struct Drag {
    /// Cursor Y when the press landed, in Cocoa screen coordinates, which grow
    /// upward from the primary display and owe nothing to any window.
    cursor_y: f64,
    /// The notch's offset at that moment, which the drag moves on from.
    offset: f64,
    moved: bool,
}

/// Pointer travel before a press counts as a drag rather than a click.
const DRAG_THRESHOLD: f64 = 4.0;
/// How often an in-flight drag re-reads the cursor.
const DRAG_TICK_MS: u64 = 16;

static DRAG: OnceLock<Mutex<Option<Drag>>> = OnceLock::new();
fn drag_lock() -> &'static Mutex<Option<Drag>> {
    DRAG.get_or_init(|| Mutex::new(None))
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

/// Records how long the notch and the card are, so the panel is never taller
/// than what it shows.
pub fn set_placement(
    app: &tauri::AppHandle,
    notch_height: Option<f64>,
    card_height: Option<f64>,
) -> tauri::Result<()> {
    if notch_height.is_none() && card_height.is_none() {
        return Ok(());
    }
    let handle = app.clone();
    app.run_on_main_thread(move || {
        {
            let mut s = state_lock().lock().expect("edge state");
            if let Some(height) = notch_height {
                s.notch_height = height.max(1.0);
            }
            if let Some(height) = card_height {
                s.card_height = height.max(1.0);
            }
        }
        let _ = layout(&handle);
        let _ = handle.emit_to("edge", "edge-state", state());
    })
}

/// Follows the pointer from AppKit for as long as the button is held.
///
/// The page cannot supply the position. Its coordinates are relative to the
/// window the drag is moving, so feeding them back in makes the notch chase
/// its own movement: each frame's reading already contains the last frame's
/// correction, and the strip oscillates between where it was and the pointer.
/// A borderless panel's `window.screenY` does not reliably say where it is
/// either, so there is nothing in the page to subtract that out with.
///
/// `NSEvent::mouseLocation` is a global reading that no window affects, so the
/// drag anchors to where the press landed and moves the notch exactly as far
/// as the cursor since. The page only says when the press starts and ends.
pub fn begin_drag(app: &tauri::AppHandle) -> tauri::Result<()> {
    let Some(cursor_y) = cursor_y() else {
        return Ok(());
    };
    {
        let mut drag = drag_lock().lock().expect("edge drag");
        if drag.is_some() {
            return Ok(());
        }
        *drag = Some(Drag {
            cursor_y,
            offset: state().offset,
            moved: false,
        });
    }
    let handle = app.clone();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(DRAG_TICK_MS));
            if drag_lock().lock().expect("edge drag").is_none() {
                break;
            }
            let app = handle.clone();
            if handle.run_on_main_thread(move || drag_tick(&app)).is_err() {
                break;
            }
        }
    });
    Ok(())
}

/// Ends the drag and reports the offset to save, and whether it moved at all —
/// a press that stayed put is a click on whatever it landed on.
pub fn end_drag() -> (bool, f64) {
    let moved = drag_lock()
        .lock()
        .expect("edge drag")
        .take()
        .is_some_and(|drag| drag.moved);
    (moved, state().offset)
}

fn drag_tick(app: &tauri::AppHandle) {
    let Some(cursor_y) = cursor_y() else {
        return;
    };
    // A pointerup the page never saw must not leave the notch following the
    // cursor around the screen.
    if !primary_button_down() {
        drag_lock().lock().expect("edge drag").take();
        return;
    }
    let Some((start_offset, delta, crossed)) = ({
        let mut guard = drag_lock().lock().expect("edge drag");
        match guard.as_mut() {
            Some(drag) => {
                // Cocoa's Y grows upward; the offset grows downward.
                let delta = drag.cursor_y - cursor_y;
                if drag.moved {
                    Some((drag.offset, delta, false))
                } else if delta.abs() < DRAG_THRESHOLD {
                    None
                } else {
                    drag.moved = true;
                    Some((drag.offset, delta, true))
                }
            }
            None => None,
        }
    }) else {
        return;
    };
    let Some((origin, available)) = work_area(app) else {
        return;
    };
    {
        let mut s = state_lock().lock().expect("edge state");
        let from = center_for_offset(start_offset, origin, available, s.notch_height);
        s.offset = offset_for_center(from + delta, origin, available, s.notch_height);
    }
    let shifted = layout(app).unwrap_or(false);
    if crossed {
        let _ = app.emit_to("edge", "edge-drag", true);
    }
    // The page has no use for the offset until the drag ends, so it only hears
    // from a frame that moved the notch inside the panel — which happens at
    // the ends of the travel, and nowhere else.
    if shifted || crossed {
        let _ = app.emit_to("edge", "edge-state", state());
    }
}

/// The work area's top and height, in points.
fn work_area(app: &tauri::AppHandle) -> Option<(f64, f64)> {
    let window = app.get_webview_window("edge")?;
    let monitor = window.primary_monitor().ok()??;
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    Some((
        area.position.y as f64 / scale,
        area.size.height as f64 / scale,
    ))
}

#[cfg(target_os = "macos")]
fn cursor_y() -> Option<f64> {
    use objc2_app_kit::NSEvent;

    Some(NSEvent::mouseLocation().y)
}

#[cfg(not(target_os = "macos"))]
fn cursor_y() -> Option<f64> {
    None
}

#[cfg(target_os = "macos")]
fn primary_button_down() -> bool {
    use objc2_app_kit::NSEvent;

    NSEvent::pressedMouseButtons() & 1 != 0
}

#[cfg(not(target_os = "macos"))]
fn primary_button_down() -> bool {
    false
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

/// Where the notch's centre sits for a given offset.
fn center_for_offset(offset: f64, origin: f64, available: f64, notch_height: f64) -> f64 {
    let (top, bottom) = travel(origin, available, notch_height);
    top + (bottom - top).max(0.0) * offset.clamp(0.0, 1.0)
}

/// Top of the panel, so the notch's centre lands at `offset` of its travel.
fn vertical_origin(
    origin: f64,
    available: f64,
    panel_height: f64,
    notch_height: f64,
    offset: f64,
) -> f64 {
    let center = center_for_offset(offset, origin, available, notch_height);
    // macOS moves a window back onto the display rather than placing it partly
    // off, and the notch would come back with it. Keeping the panel inside the
    // work area means the position asked for is the position given.
    (center - panel_height / 2.0).clamp(origin, origin + (available - panel_height).max(0.0))
}

/// Where the notch sits inside the panel, in points down from its top.
///
/// Centring it there would cost travel: an open panel is as tall as the card,
/// and holding that inside the work area pulls the notch back from the ends of
/// the display with it. So the panel goes as close as it is allowed and the
/// notch takes up the remaining distance inside it. The notch always fits —
/// the panel is never shorter than the notch — so this never clips it.
fn notch_top_in_panel(
    origin: f64,
    available: f64,
    panel_height: f64,
    notch_height: f64,
    offset: f64,
) -> f64 {
    let center = center_for_offset(offset, origin, available, notch_height);
    let top = vertical_origin(origin, available, panel_height, notch_height, offset);
    (center - notch_height / 2.0 - top).clamp(0.0, (panel_height - notch_height).max(0.0))
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
/// Places the panel and says whether the notch moved inside it, which is the
/// only part of the geometry the page has to redraw.
fn layout(app: &tauri::AppHandle) -> tauri::Result<bool> {
    let Some(window) = app.get_webview_window("edge") else {
        return Ok(false);
    };
    let Some(monitor) = window.primary_monitor()? else {
        return Ok(false);
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
    let origin = area.position.y as f64 / scale;
    let y = vertical_origin(origin, available, height, s.notch_height, s.offset) * scale;
    let notch_top = notch_top_in_panel(origin, available, height, s.notch_height, s.offset);
    let shifted = {
        let mut current = state_lock().lock().expect("edge state");
        let shifted = current.notch_top.round() != notch_top.round();
        current.notch_top = notch_top;
        shifted
    };
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
    Ok(shifted)
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

        // A drag is where it started plus how far the pointer moved, so a
        // press that has not moved leaves the notch exactly where it was.
        for offset in [0.0, 0.25, 0.5, 1.0] {
            let from = center_for_offset(offset, origin, available, notch);
            assert_eq!(offset_for_center(from, origin, available, notch), offset);
        }
        // And moving 100pt moves the notch 100pt, not to wherever the pointer
        // happens to be.
        let from = center_for_offset(0.5, origin, available, notch);
        let moved = offset_for_center(from + 100.0, origin, available, notch);
        assert_eq!(
            center_for_offset(moved, origin, available, notch),
            from + 100.0
        );

        // A panel matching the notch reaches both ends of that travel.
        let fitted = vertical_origin(origin, available, notch, notch, 0.0);
        assert_eq!(fitted + notch / 2.0, 135.0);
        assert_eq!(
            vertical_origin(origin, available, notch, notch, 1.0) + notch / 2.0,
            815.0
        );

        // A taller panel is held inside the work area, so its centre cannot
        // reach the ends — which is why the notch is placed inside it rather
        // than centred in it.
        let centred = vertical_origin(origin, available, panel, notch, 0.0) + panel / 2.0;
        assert!(centred > 135.0);
        assert_eq!(
            vertical_origin(origin, available, panel, notch, 0.0),
            origin
        );
    }

    #[test]
    fn an_open_card_does_not_shorten_the_notch_travel() {
        // 900pt of work area from 25, a 223pt notch, and a 420pt card holding
        // the panel open. Centring the notch in that panel would strand its
        // centre between 235 and 715; placing it inside reaches 136.5 and
        // 813.5, which is the whole edge.
        let (origin, available, notch, panel) = (25.0, 900.0, 223.0, 420.0);
        let screen_top = |offset: f64| {
            vertical_origin(origin, available, panel, notch, offset)
                + notch_top_in_panel(origin, available, panel, notch, offset)
        };
        assert_eq!(screen_top(0.0), origin);
        assert_eq!(screen_top(1.0), origin + available - notch);
        // Centring it in the panel is what used to cost the last 98 points:
        // the panel stops at the bottom of the work area and its centre with
        // it, well short of where the notch's centre is allowed to go.
        let centred = vertical_origin(origin, available, panel, notch, 1.0) + panel / 2.0;
        assert_eq!(centred, 715.0);
        assert_eq!(center_for_offset(1.0, origin, available, notch), 813.5);

        // The notch stays inside the panel at every point along the way, so a
        // shift never clips it.
        for step in 0..=20 {
            let at = notch_top_in_panel(origin, available, panel, notch, f64::from(step) / 20.0);
            assert!((0.0..=panel - notch).contains(&at));
        }
        // Folded, the panel is the notch, so there is nothing to shift.
        assert_eq!(
            notch_top_in_panel(origin, available, notch, notch, 0.4),
            0.0
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
