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

// Panel layout (logical points). ITEM_W must equal ITEM_BOX in src/App.tsx.
const ITEM_W: f64 = 104.0;
const H_PAD: f64 = 28.0;
const PANEL_H: f64 = 196.0;
const MIN_W: f64 = 260.0;

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

fn position_and_size_panel(app: &AppHandle, item_count: usize) {
    let Some(p) = panel(app) else { return };
    let Some(mtm) = MainThreadMarker::new() else { return };

    let mouse = NSEvent::mouseLocation();
    let mut screen_frame = NSScreen::mainScreen(mtm)
        .map(|s| s.frame())
        .unwrap_or(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1440.0, 900.0)));
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
        // Key (without activating our background app) so the webview gets mouse events.
        p.make_key_and_order_front();
    }
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
    windows::enumerate(pid)
        .into_iter()
        .map(|title| AppItem { pid, name: title })
        .collect()
}

// ---- gesture internals (main thread) ----

fn start(app: &AppHandle, mode: Mode, required_mods: u64) {
    let items = match mode {
        Mode::Apps => apps::build_ordered_apps(),
        Mode::Windows => window_items(),
    };
    if items.is_empty() {
        crate::dlog!("[ctl-tab] start {}: no items, not showing", mode.label());
        return;
    }
    let selected = if items.len() > 1 { 1 } else { 0 };

    let payload = {
        let mut s = state().lock().unwrap();
        s.switcher.start(mode, items, selected);
        s.active_mods = required_mods;
        build_show_payload(&s.switcher)
    };
    let count = payload.items.len();
    let _ = app.emit("switcher:show", payload);
    position_and_size_panel(app, count);
    show_panel(app);
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
    let Some((action, delta)) = match_combo(&cfg, keycode, mods) else {
        return false;
    };

    if !active {
        // Start only on the forward direction (no extra Shift).
        if delta > 0 {
            let combo = combo_for(&cfg, action);
            start(app, action.mode(), combo.modifiers);
        } else {
            return false; // backward while idle: ignore, don't consume
        }
    } else {
        advance(app, delta);
    }

    state().lock().unwrap().switcher.active
}

pub fn handle_flags_changed(app: &AppHandle, flags: u64) {
    let (active, active_mods) = {
        let s = state().lock().unwrap();
        (s.switcher.active, s.active_mods)
    };
    if active {
        let mods = flags & MODS_ALL;
        // Commit once the required modifiers are no longer all held.
        if (mods & active_mods) != active_mods {
            commit(app);
        }
    }
}

fn combo_for(cfg: &Config, action: Action) -> &Combo {
    match action {
        Action::SwitchApp => &cfg.switch_app,
        Action::SwitchWindows => &cfg.switch_windows,
    }
}

/// Match a keyDown against the config. Returns (action, direction) where +1 is
/// forward and -1 is backward (combo modifiers + Shift).
fn match_combo(cfg: &Config, keycode: i64, mods: u64) -> Option<(Action, isize)> {
    for action in [Action::SwitchApp, Action::SwitchWindows] {
        let combo = combo_for(cfg, action);
        if keycode == combo.key_code {
            if mods == combo.modifiers {
                return Some((action, 1));
            } else if mods == combo.modifiers | MOD_SHIFT {
                return Some((action, -1));
            }
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
