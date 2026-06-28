// ctrl-tab — macOS Alt-Tab clone (Tauri v2, single Rust binary).
//
// A silent background utility: Ctrl-Tab switches apps, Ctrl-§ switches windows of
// the frontmost app (both shortcuts configurable). The overlay is a transparent,
// non-activating NSPanel. No Dock icon, no menu-bar item. The Settings window is
// opened by relaunching the app (Reopen) and quit from a button inside it.
// See CLAUDE.md.

mod apps;
mod config;
mod controller;
mod events;
mod hotkey;
mod switcher;
mod windows;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

// These traits must be in scope (unqualified) for the panel! macro expansion.
use objc2::runtime::NSObjectProtocol;
use objc2::{ClassType, Message};
use tauri::{AppHandle, Manager, WebviewUrl};
use tauri_nspanel::{panel, CollectionBehavior, PanelBuilder, PanelLevel, StyleMask};

use controller::Action;

/// Set true only when the user really wants to quit (Settings → Quit).
static QUIT: AtomicBool = AtomicBool::new(false);

/// Whether verbose diagnostic logging is enabled (env `CTRL_TAB_DEBUG=1`).
pub(crate) fn debug_enabled() -> bool {
    static D: OnceLock<bool> = OnceLock::new();
    *D.get_or_init(|| {
        std::env::var("CTRL_TAB_DEBUG")
            .map(|v| !v.is_empty() && v != "0")
            .unwrap_or(false)
    })
}

/// `eprintln!` only when `CTRL_TAB_DEBUG` is set. Use for diagnostics.
#[macro_export]
macro_rules! dlog {
    ($($arg:tt)*) => {
        if $crate::debug_enabled() {
            eprintln!($($arg)*);
        }
    };
}

panel!(OverlayPanel {
    config: {
        can_become_key_window: true
    }
});

const OVERLAY_LABEL: &str = "overlay";
const SETTINGS_LABEL: &str = "settings";
/// Panel corner radius (px) — ~22% of the icon size; matches the CSS `--tile-radius`.
const PANEL_RADIUS: f64 = 14.0;

// ---- Settings window + activation policy ----

/// The app is Accessory (no Dock icon) while idle. To give the Settings window
/// reliable focus on macOS we switch to Regular while it is visible, then back to
/// Accessory when it hides — so the Dock icon appears only while Settings is open
/// and disappears when it closes (idle = no Dock icon).
#[cfg(target_os = "macos")]
fn set_accessory(app: &AppHandle, accessory: bool) {
    let policy = if accessory {
        tauri::ActivationPolicy::Accessory
    } else {
        tauri::ActivationPolicy::Regular
    };
    let _ = app.set_activation_policy(policy);
}

/// Show + focus the Settings window (Regular while visible). Always runs on the
/// main thread (the single-instance callback may fire off the main thread).
fn show_settings(app: &AppHandle) {
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        #[cfg(target_os = "macos")]
        set_accessory(&app, false); // Regular so the window comes front + takes focus
        if let Some(w) = app.get_webview_window(SETTINGS_LABEL) {
            let _ = w.show();
            let _ = w.unminimize();
            let _ = w.set_focus();
        }
    });
}

/// Hide the Settings window and return to Accessory (removes the Dock icon).
fn hide_settings(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(SETTINGS_LABEL) {
        let _ = w.hide();
    }
    #[cfg(target_os = "macos")]
    set_accessory(app, true);
}

// ---- Tauri commands ----

#[tauri::command]
fn switcher_hover(app: AppHandle, index: usize) {
    controller::hover(&app, index);
}

#[tauri::command]
fn switcher_commit(app: AppHandle, index: usize) -> Result<(), String> {
    let app2 = app.clone();
    app.run_on_main_thread(move || controller::commit_index(&app2, index))
        .map_err(|e| e.to_string())
}

/// The overlay webview signals it has rendered; center + show it (already sized).
#[tauri::command]
fn present_overlay(app: AppHandle) -> Result<(), String> {
    let app2 = app.clone();
    app.run_on_main_thread(move || controller::present_overlay(&app2))
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn get_config() -> config::Config {
    controller::current_config()
}

#[tauri::command]
fn start_recording(action: String) {
    let a = match action.as_str() {
        "windows" => Action::SwitchWindows,
        _ => Action::SwitchApp,
    };
    controller::start_recording(a);
}

#[tauri::command]
fn save_config(
    app_mods: u64,
    app_key: i64,
    win_mods: u64,
    win_key: i64,
) -> Result<config::Config, String> {
    let app_combo = config::Combo::new(app_mods, app_key);
    let win_combo = config::Combo::new(win_mods, win_key);
    controller::save_config(app_combo, win_combo)
}

#[tauri::command]
fn reset_config() -> config::Config {
    controller::reset_config()
}

/// Quit the app for real (the only way to fully exit — there is no tray).
#[tauri::command]
fn quit_app(app: AppHandle) {
    QUIT.store(true, Ordering::SeqCst);
    app.exit(0);
}

#[tauri::command]
fn get_autostart(app: AppHandle) -> bool {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
fn set_autostart(app: AppHandle, enabled: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let mgr = app.autolaunch();
    if enabled { mgr.enable() } else { mgr.disable() }.map_err(|e| e.to_string())
}

// ---- Overlay panel ----

fn create_overlay(app: &AppHandle) -> tauri::Result<()> {
    let panel = PanelBuilder::<tauri::Wry, OverlayPanel>::new(app, OVERLAY_LABEL)
        .url(WebviewUrl::App("index.html".into()))
        .title("ctrl-tab overlay")
        .with_window(|w| {
            w.decorations(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .resizable(false)
                .transparent(true)
                .visible(false)
                .center()
                .inner_size(720.0, 220.0)
        })
        .style_mask(StyleMask::empty().borderless().nonactivating_panel())
        .floating(true)
        .level(PanelLevel::Floating)
        .hides_on_deactivate(false)
        .no_activate(true)
        .collection_behavior(
            CollectionBehavior::new()
                .can_join_all_spaces()
                .ignores_cycle(),
        )
        .has_shadow(true) // soft drop shadow for the glass panel
        .transparent(true)
        .build()?;

    panel.set_ignores_mouse_events(false);
    panel.set_accepts_mouse_moved_events(true);
    panel.hide();

    // Native dark vibrancy ("liquid glass") behind the transparent webview. The CSS
    // adds the dark tint + sheen on top; the corner radius matches the CSS panel.
    #[cfg(target_os = "macos")]
    if let Some(win) = app.get_webview_window(OVERLAY_LABEL) {
        use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial, NSVisualEffectState};
        if let Err(e) = apply_vibrancy(
            &win,
            NSVisualEffectMaterial::HudWindow,
            Some(NSVisualEffectState::Active),
            Some(PANEL_RADIUS),
        ) {
            eprintln!("[ctrl-tab] vibrancy not applied: {e:?}");
        }
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // single-instance MUST be the first plugin.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            show_settings(app);
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_nspanel::init())
        .invoke_handler(tauri::generate_handler![
            switcher_hover,
            switcher_commit,
            present_overlay,
            get_config,
            start_recording,
            save_config,
            reset_config,
            quit_app,
            get_autostart,
            set_autostart
        ])
        .setup(|app| {
            let handle = app.handle();

            // Silent background utility: Accessory (no Dock icon), and we never switch
            // to Regular (that would show a Dock icon).
            #[cfg(target_os = "macos")]
            let _ = handle.set_activation_policy(tauri::ActivationPolicy::Accessory);

            // Load persisted config into the shared state.
            let config_path: PathBuf = handle
                .path()
                .app_config_dir()
                .map(|d| d.join("config.json"))
                .unwrap_or_else(|_| PathBuf::from("ctrl-tab-config.json"));
            controller::init(config_path);

            create_overlay(handle)?;

            // Settings window is created hidden (see tauri.conf.json visible:false).
            // Closing it hides instead of quitting.
            if let Some(settings) = app.get_webview_window(SETTINGS_LABEL) {
                let h = handle.clone();
                settings.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        hide_settings(&h);
                    }
                });
            }
            // NOTE: we do NOT show Settings on first launch — start silent.

            // Native gesture pipeline.
            #[cfg(target_os = "macos")]
            {
                hotkey::ensure_accessibility();
                apps::install_workspace_observer();
                hotkey::install_event_tap(handle.clone());
            }

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| match event {
            // Relaunching the app (Spotlight/Raycast/Finder) while it's already
            // running sends Reopen — show the Settings window.
            tauri::RunEvent::Reopen { .. } => show_settings(app),
            // Don't quit when the last window closes; only on explicit Quit.
            tauri::RunEvent::ExitRequested { api, .. } => {
                if !QUIT.load(Ordering::SeqCst) {
                    api.prevent_exit();
                }
            }
            _ => {}
        });
}
