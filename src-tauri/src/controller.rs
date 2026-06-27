//! Orchestration between the gesture, the switcher state, the overlay panel, and
//! the frontend events. Holds the single source of truth for switcher state so
//! both the event tap (keyboard) and the Tauri commands (mouse) drive the same
//! thing. Native panel ops here must run on the main thread.

use std::sync::{Mutex, OnceLock};

use objc2_app_kit::{NSEvent, NSScreen};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};
use tauri::{AppHandle, Emitter};
use tauri_nspanel::ManagerExt;

use crate::events::{SelectPayload, ShowPayload, SwitchItem};
use crate::switcher::{AppItem, Mode, Switcher};
use crate::{apps, windows};

const OVERLAY_LABEL: &str = "overlay";

// Panel layout (logical points). Must visually match the React overlay.
const ITEM_W: f64 = 104.0;
const H_PAD: f64 = 28.0;
const PANEL_H: f64 = 196.0;
const MIN_W: f64 = 260.0;

fn switcher() -> &'static Mutex<Switcher> {
    static S: OnceLock<Mutex<Switcher>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(Switcher::new()))
}

pub fn is_active() -> bool {
    switcher().lock().unwrap().active
}

fn build_show_payload(s: &Switcher) -> ShowPayload {
    let items = s
        .items
        .iter()
        .enumerate()
        .map(|(i, a)| SwitchItem {
            // Unique per item: in windows mode every item shares the owner pid.
            id: format!("{}-{}", a.pid, i),
            title: a.name.clone(),
            app_name: a.name.clone(),
            icon_data_url: apps::icon_for(a.pid).unwrap_or_default(),
        })
        .collect();
    ShowPayload {
        mode: s.mode.label().to_string(),
        items,
        selected: s.selected,
    }
}

fn panel(app: &AppHandle) -> Option<tauri_nspanel::PanelHandle<tauri::Wry>> {
    app.get_webview_panel(OVERLAY_LABEL).ok()
}

/// Center + size the overlay on the screen that currently holds the mouse.
fn position_and_size_panel(app: &AppHandle, item_count: usize) {
    let Some(p) = panel(app) else { return };
    let Some(mtm) = MainThreadMarker::new() else { return };

    let mouse = NSEvent::mouseLocation();
    let mut screen_frame = NSScreen::mainScreen(mtm)
        .map(|s| s.frame())
        .unwrap_or(NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(1440.0, 900.0),
        ));
    for s in NSScreen::screens(mtm).iter() {
        let f = s.frame();
        if mouse.x >= f.origin.x
            && mouse.x <= f.origin.x + f.size.width
            && mouse.y >= f.origin.y
            && mouse.y <= f.origin.y + f.size.height
        {
            screen_frame = f;
            break;
        }
    }

    let max_w = (screen_frame.size.width - 120.0).max(MIN_W);
    let n = item_count.max(1) as f64;
    let w = (H_PAD * 2.0 + n * ITEM_W).clamp(MIN_W, max_w);

    let ns = p.as_panel();
    ns.setContentSize(NSSize::new(w, PANEL_H));
    let frame = ns.frame();
    let origin = NSPoint::new(
        screen_frame.origin.x + (screen_frame.size.width - frame.size.width) / 2.0,
        screen_frame.origin.y + (screen_frame.size.height - frame.size.height) / 2.0,
    );
    ns.setFrameOrigin(origin);
}

fn show_panel(app: &AppHandle) {
    if let Some(p) = panel(app) {
        // Make it key so the webview receives mouse-moved/click events. A
        // NonactivatingPanel becomes key WITHOUT activating our (background) app.
        p.make_key_and_order_front();
    }
}

fn hide_panel(app: &AppHandle) {
    if let Some(p) = panel(app) {
        p.hide();
    }
}

/// Build the window-mode item list for the frontmost app. Each item carries the
/// owner pid (so all share the app icon); titles come from the windows. The list
/// index lines up with `windows::raise`.
fn window_items() -> Vec<AppItem> {
    let Some((pid, _name)) = apps::frontmost() else {
        return Vec::new();
    };
    apps::ensure_icon(pid);
    windows::enumerate(pid)
        .into_iter()
        .map(|title| AppItem { pid, name: title })
        .collect()
}

// ---- gesture-driven (called from the event tap, already on the main thread) ----

pub fn gesture_start(app: &AppHandle, mode: Mode) {
    let items = match mode {
        Mode::Apps => apps::build_ordered_apps(),
        Mode::Windows => window_items(),
    };
    // Nothing to switch between → don't show the overlay (gesture stays inactive).
    if items.is_empty() {
        crate::dlog!("[ctl-tab] gesture_start {}: no items, not showing", mode.label());
        return;
    }
    let selected = if items.len() > 1 { 1 } else { 0 };

    let payload = {
        let mut s = switcher().lock().unwrap();
        s.start(mode, items, selected);
        build_show_payload(&s)
    };
    let count = payload.items.len();

    let _ = app.emit("switcher:show", payload);
    position_and_size_panel(app, count);
    show_panel(app);
}

pub fn gesture_advance(app: &AppHandle, delta: isize) {
    let selected = {
        let mut s = switcher().lock().unwrap();
        s.advance(delta);
        s.selected
    };
    let _ = app.emit("switcher:select", SelectPayload { selected });
}

/// Hide the overlay and act on the committed selection (activate app / raise window).
fn perform_commit(app: &AppHandle, mode: Mode, index: usize, item: Option<AppItem>) {
    let _ = app.emit("switcher:hide", ());
    hide_panel(app);
    match mode {
        Mode::Apps => {
            if let Some(it) = item {
                apps::activate(it.pid);
            }
        }
        Mode::Windows => windows::raise(index),
    }
}

pub fn gesture_commit(app: &AppHandle) {
    let (mode, index, item) = {
        let mut s = switcher().lock().unwrap();
        (s.mode, s.selected, s.commit())
    };
    perform_commit(app, mode, index, item);
}

pub fn gesture_cancel(app: &AppHandle) {
    switcher().lock().unwrap().cancel();
    let _ = app.emit("switcher:hide", ());
    hide_panel(app);
}

// ---- mouse-driven (called from Tauri commands) ----

/// Update selection from a hover. State + event only; no native panel op, so it
/// is safe off the main thread.
pub fn hover(app: &AppHandle, index: usize) {
    let selected = {
        let mut s = switcher().lock().unwrap();
        if !s.active || s.items.is_empty() {
            return;
        }
        s.selected = index.min(s.items.len() - 1);
        s.selected
    };
    let _ = app.emit("switcher:select", SelectPayload { selected });
}

/// Commit from a click. Sets the index, activates, hides, resets — so the later
/// Ctrl release does not double-commit. MUST run on the main thread.
pub fn commit_index(app: &AppHandle, index: usize) {
    let (mode, idx, item) = {
        let mut s = switcher().lock().unwrap();
        if !s.active || s.items.is_empty() {
            return;
        }
        s.selected = index.min(s.items.len() - 1);
        (s.mode, s.selected, s.commit())
    };
    perform_commit(app, mode, idx, item);
}
