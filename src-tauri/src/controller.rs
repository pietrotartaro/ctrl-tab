//! Orchestration between the gesture, switcher state, config, the overlay panel,
//! and the frontend events. Single source of truth (CtlState) so the event tap
//! (keyboard) and the Tauri commands (mouse + settings) all drive the same thing.
//! Native panel ops here must run on the main thread.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use objc2_app_kit::{NSEvent, NSScreen};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};
use tauri::{AppHandle, Emitter};
use tauri_nspanel::ManagerExt;

use crate::config::{self, Combo, Config, MOD_SHIFT, MODS_ALL};
use crate::events::{SelectPayload, ShowPayload, SwitchItem};
use crate::switcher::{AppItem, Mode, Switcher};
use crate::{apps, windows};

const OVERLAY_LABEL: &str = "overlay";

// Margin (logical points) kept free on each side of the active screen; the panel
// never grows wider/taller than (screen - 2*MARGIN).
const SCREEN_MARGIN: f64 = 80.0;

// Panel layout constants (logical px). Must match src/App.tsx exactly so the
// Rust-computed panel size matches the rendered flex-wrap layout (no centered
// title row anymore; only the wrapped icon grid).
const ITEM_BOX: f64 = 88.0; // per-item width (icon 64 + padding/label room)
const GRID_GAP: f64 = 4.0; // gap-1 between items/rows
const H_PAD: f64 = 12.0; // p-3 (card horizontal padding)
const V_PAD: f64 = 12.0; // p-3 (card vertical padding)
const ITEM_H: f64 = 98.0; // per-item height (icon 64 + name + p-2 padding)

/// Compute the panel content size for `count` uniform items, wrapping at `max_w`.
fn compute_panel_size(count: usize, max_w: f64) -> (f64, f64) {
    let count = count.max(1) as f64;
    let inner_max = (max_w - 2.0 * H_PAD).max(ITEM_BOX);
    let per_row = (((inner_max + GRID_GAP) / (ITEM_BOX + GRID_GAP)).floor())
        .clamp(1.0, count);
    let rows = (count / per_row).ceil();
    let content_w = per_row * ITEM_BOX + (per_row - 1.0) * GRID_GAP;
    // +2px slack so a row that is exactly content-wide doesn't wrap on sub-pixel
    // rounding (which would push items onto an extra, clipped row).
    let w = content_w + 2.0 * H_PAD + 2.0;
    let h = 2.0 * V_PAD + rows * ITEM_H + (rows - 1.0) * GRID_GAP;
    (w, h)
}

/// The two configurable actions.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Action {
    SwitchApp,
    SwitchWindows,
}

impl Action {
    fn mode(self) -> Mode {
        match self {
            Action::SwitchApp => Mode::Apps,
            Action::SwitchWindows => Mode::Windows,
        }
    }
    fn key(self) -> &'static str {
        match self {
            Action::SwitchApp => "app",
            Action::SwitchWindows => "windows",
        }
    }
}

struct CtlState {
    switcher: Switcher,
    config: Config,
    config_path: Option<PathBuf>,
    /// Which action is currently being recorded (if any).
    recording: Option<Action>,
    /// Modifier flags required by the active gesture (for release → commit).
    active_mods: u64,
    /// Previous Shift state, to detect rising edges (left navigation).
    prev_shift: bool,
}

fn state() -> &'static Mutex<CtlState> {
    static S: OnceLock<Mutex<CtlState>> = OnceLock::new();
    S.get_or_init(|| {
        Mutex::new(CtlState {
            switcher: Switcher::new(),
            config: Config::default(),
            config_path: None,
            recording: None,
            active_mods: 0,
            prev_shift: false,
        })
    })
}

/// Load config from disk into the shared state at startup.
pub fn init(config_path: PathBuf) {
    let cfg = config::load_from(&config_path);
    let mut s = state().lock().unwrap();
    s.config = cfg;
    s.config_path = Some(config_path);
}

pub fn current_config() -> Config {
    state().lock().unwrap().config.clone()
}

// ---- panel helpers (main thread) ----

fn build_show_payload(s: &Switcher) -> ShowPayload {
    let items = s
        .items
        .iter()
        .enumerate()
        .map(|(i, a)| SwitchItem {
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

/// Frame of the screen that currently holds the mouse (fallback: main screen).
fn active_screen_frame() -> NSRect {
    let default = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1440.0, 900.0));
    let Some(mtm) = MainThreadMarker::new() else {
        return default;
    };
    let mouse = NSEvent::mouseLocation();
    let mut frame = NSScreen::mainScreen(mtm).map(|s| s.frame()).unwrap_or(default);
    for s in NSScreen::screens(mtm).iter() {
        let f = s.frame();
        if mouse.x >= f.origin.x
            && mouse.x <= f.origin.x + f.size.width
            && mouse.y >= f.origin.y
            && mouse.y <= f.origin.y + f.size.height
        {
            frame = f;
            break;
        }
    }
    frame
}

/// Resize the (still hidden) panel to `w`×`h`. The webview reflows to this size so
/// the flex-wrap layout matches before we show it. Does NOT show the panel.
fn presize_hidden(app: &AppHandle, w: f64, h: f64) {
    if let Some(p) = panel(app) {
        p.as_panel().setContentSize(NSSize::new(w, h));
    }
}

/// Center the (already-sized) panel on the active screen and show it (key, without
/// activating our background app).
fn center_and_show(app: &AppHandle) {
    let Some(p) = panel(app) else { return };
    let screen = active_screen_frame();
    let ns = p.as_panel();
    let frame = ns.frame();
    let origin = NSPoint::new(
        screen.origin.x + (screen.size.width - frame.size.width) / 2.0,
        screen.origin.y + (screen.size.height - frame.size.height) / 2.0,
    );
    ns.setFrameOrigin(origin);
    p.make_key_and_order_front();
    crate::dlog!(
        "[ctl-tab] present overlay {}x{} @ {},{}",
        frame.size.width as i64,
        frame.size.height as i64,
        origin.x as i64,
        origin.y as i64
    );
}

fn hide_panel(app: &AppHandle) {
    if let Some(p) = panel(app) {
        p.hide();
    }
}

fn window_items() -> Vec<AppItem> {
    let Some((pid, _name)) = apps::frontmost() else {
        return Vec::new();
    };
    apps::ensure_icon(pid);
    // VS Code only: reorder window titles from "file — project" to "project — file".
    let is_vscode = apps::bundle_id(pid)
        .as_deref()
        .map(windows::is_vscode_bundle)
        .unwrap_or(false);
    windows::enumerate(pid)
        .into_iter()
        .map(|title| {
            let name = if is_vscode {
                windows::reorder_vscode_title(&title)
            } else {
                title
            };
            AppItem { pid, name }
        })
        .collect()
}

// ---- gesture internals (main thread) ----

fn start(app: &AppHandle, mode: Mode, required_mods: u64, from_left: bool) {
    let items = match mode {
        Mode::Apps => apps::build_ordered_apps(),
        Mode::Windows => window_items(),
    };
    if items.is_empty() {
        crate::dlog!("[ctl-tab] start {}: no items, not showing", mode.label());
        return;
    }
    // Right: select the previous item (index 1). Left: wrap to the last item.
    let selected = if from_left {
        items.len() - 1
    } else if items.len() > 1 {
        1
    } else {
        0
    };

    let screen = active_screen_frame();
    let max_w = (screen.size.width - 2.0 * SCREEN_MARGIN).max(ITEM_BOX + 2.0 * H_PAD);
    let count = items.len();
    let (w, h) = compute_panel_size(count, max_w);

    let payload = {
        let mut s = state().lock().unwrap();
        s.switcher.start(mode, items, selected);
        s.active_mods = required_mods;
        build_show_payload(&s.switcher)
    };
    // Size the (still hidden) panel to the Rust-computed content size so the webview
    // reflows the flex-wrap layout to match, then emit. The frontend renders and
    // signals readiness via present_overlay, which centers + shows it (no flicker,
    // no per-select resize). Item widths are uniform, so the layout is deterministic.
    presize_hidden(app, w, h);
    let _ = app.emit("switcher:show", payload);
}

/// Called by the frontend once it has rendered the items. Centers + shows the
/// overlay (already sized in `start`). No-op if the gesture is no longer active.
pub fn present_overlay(app: &AppHandle) {
    if !state().lock().unwrap().switcher.active {
        return;
    }
    center_and_show(app);
}

fn advance(app: &AppHandle, delta: isize) {
    let selected = {
        let mut s = state().lock().unwrap();
        s.switcher.advance(delta);
        s.switcher.selected
    };
    let _ = app.emit("switcher:select", SelectPayload { selected });
}

fn perform_commit(app: &AppHandle, mode: Mode, index: usize, item: Option<AppItem>) {
    let _ = app.emit("switcher:hide", ());
    hide_panel(app);
    // Drop to Accessory so we're not the active app — otherwise (e.g. the Settings
    // window is open under Regular) NSRunningApplication activation returns false and
    // the target app/window won't come forward.
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    match mode {
        Mode::Apps => {
            if let Some(it) = item {
                apps::activate(it.pid);
            }
        }
        Mode::Windows => windows::raise(index),
    }
}

fn commit(app: &AppHandle) {
    let (mode, index, item) = {
        let mut s = state().lock().unwrap();
        s.active_mods = 0;
        (s.switcher.mode, s.switcher.selected, s.switcher.commit())
    };
    perform_commit(app, mode, index, item);
}

fn cancel(app: &AppHandle) {
    {
        let mut s = state().lock().unwrap();
        s.active_mods = 0;
        s.switcher.cancel();
    }
    let _ = app.emit("switcher:hide", ());
    hide_panel(app);
}

// ---- tap event handlers (main thread; called from hotkey.rs) ----

/// Returns true if the keyDown should be consumed (not passed to the app below).
pub fn handle_key_down(app: &AppHandle, keycode: i64, flags: u64) -> bool {
    let mods = flags & MODS_ALL;

    // Recording mode: capture the first modifier+key combo, emit it, stop.
    let recording = state().lock().unwrap().recording;
    if let Some(action) = recording {
        if mods != 0 && !config::is_modifier_keycode(keycode) {
            let label = config::format_combo_label(mods, keycode);
            state().lock().unwrap().recording = None;
            let _ = app.emit(
                "recording:done",
                serde_json::json!({
                    "action": action.key(),
                    "modifiers": mods,
                    "keyCode": keycode,
                    "label": label,
                }),
            );
        }
        return true; // consume everything while recording
    }

    let active = state().lock().unwrap().switcher.active;

    // Esc cancels an active gesture.
    if active && keycode == 53 {
        cancel(app);
        return true;
    }

    let cfg = state().lock().unwrap().config.clone();
    // The trigger key (Tab / §) always moves the selection RIGHT. Left movement is
    // handled by a Shift rising edge in handle_flags_changed.
    let Some(action) = match_trigger(&cfg, keycode, mods) else {
        return false;
    };

    if !active {
        let combo = combo_for(&cfg, action);
        start(app, action.mode(), combo.modifiers, false);
    } else {
        advance(app, 1);
    }

    state().lock().unwrap().switcher.active
}

pub fn handle_flags_changed(app: &AppHandle, flags: u64) {
    let shift_now = flags & MOD_SHIFT != 0;
    let mods = flags & MODS_ALL;

    let (active, active_mods, prev_shift, recording) = {
        let mut s = state().lock().unwrap();
        let prev = s.prev_shift;
        s.prev_shift = shift_now; // always track the latest Shift state
        (
            s.switcher.active,
            s.active_mods,
            prev,
            s.recording.is_some(),
        )
    };

    if recording || !active {
        // Shift rising edge while idle does NOTHING — the switcher only opens via the
        // real trigger (Ctrl+Tab / Ctrl+§), never via Ctrl+Shift.
        return;
    }

    // Commit once the hold modifiers are no longer all held (e.g. Ctrl released).
    if (mods & active_mods) != active_mods {
        commit(app);
        return;
    }

    // Shift rising edge → move LEFT, but only while exactly armed (so extra
    // Option/Command don't navigate) and when the hold modifier isn't Shift.
    let shift_rising = shift_now && !prev_shift;
    if shift_rising && active_mods & MOD_SHIFT == 0 && config::is_armed(mods, active_mods) {
        advance(app, -1);
    }
}

fn combo_for(cfg: &Config, action: Action) -> &Combo {
    match action {
        Action::SwitchApp => &cfg.switch_app,
        Action::SwitchWindows => &cfg.switch_windows,
    }
}

/// Match a trigger keyDown (Tab / §) against the config — only when the held
/// modifiers EXACTLY arm the action (Shift ignored; Option/Command disqualify).
/// The key always means "move right". A custom combo that itself includes Shift
/// keeps the previous exact-match behavior.
fn match_trigger(cfg: &Config, keycode: i64, mods: u64) -> Option<Action> {
    for action in [Action::SwitchApp, Action::SwitchWindows] {
        let combo = combo_for(cfg, action);
        if keycode != combo.key_code || combo.modifiers == 0 {
            continue;
        }
        let armed = if combo.modifiers & MOD_SHIFT != 0 {
            mods == combo.modifiers // shift is part of the hold → exact match
        } else {
            config::is_armed(mods, combo.modifiers)
        };
        if armed {
            return Some(action);
        }
    }
    None
}

// ---- mouse-driven (Tauri commands) ----

pub fn hover(app: &AppHandle, index: usize) {
    let selected = {
        let mut s = state().lock().unwrap();
        if !s.switcher.active || s.switcher.items.is_empty() {
            return;
        }
        let max = s.switcher.items.len() - 1;
        s.switcher.selected = index.min(max);
        s.switcher.selected
    };
    let _ = app.emit("switcher:select", SelectPayload { selected });
}

pub fn commit_index(app: &AppHandle, index: usize) {
    let (mode, idx, item) = {
        let mut s = state().lock().unwrap();
        if !s.switcher.active || s.switcher.items.is_empty() {
            return;
        }
        let max = s.switcher.items.len() - 1;
        s.switcher.selected = index.min(max);
        s.active_mods = 0;
        (s.switcher.mode, s.switcher.selected, s.switcher.commit())
    };
    perform_commit(app, mode, idx, item);
}

// ---- settings-driven (Tauri commands) ----

pub fn start_recording(action: Action) {
    state().lock().unwrap().recording = Some(action);
    crate::dlog!("[ctl-tab] recording started for {}", action.key());
}

/// Validate + apply + persist a new config. Returns the saved config on success.
pub fn save_config(app_combo: Combo, win_combo: Combo) -> Result<Config, String> {
    config::validate_pair(&app_combo, &win_combo)?;
    let cfg = Config {
        switch_app: app_combo,
        switch_windows: win_combo,
    };
    let path = {
        let mut s = state().lock().unwrap();
        s.config = cfg.clone();
        s.recording = None;
        s.config_path.clone()
    };
    if let Some(path) = path {
        if let Err(e) = config::save_to(&path, &cfg) {
            return Err(format!("Impossibile salvare la config: {e}"));
        }
    }
    crate::dlog!("[ctl-tab] config saved: {} / {}", cfg.switch_app.label, cfg.switch_windows.label);
    Ok(cfg)
}

pub fn reset_config() -> Config {
    let cfg = Config::default();
    let path = {
        let mut s = state().lock().unwrap();
        s.config = cfg.clone();
        s.recording = None;
        s.config_path.clone()
    };
    if let Some(path) = path {
        let _ = config::save_to(&path, &cfg);
    }
    cfg
}
