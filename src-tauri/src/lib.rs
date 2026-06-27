// ctl-tab — macOS Alt-Tab clone (Tauri v2, single Rust binary).
//
// A background utility: Ctrl-Tab switches apps, Ctrl-§ switches windows of the
// frontmost app (both shortcuts configurable). The overlay is a transparent,
// non-activating NSPanel; a Settings window + menu-bar tray manage the app.
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
use tauri::menu::{CheckMenuItem, Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, WebviewUrl};
use tauri_nspanel::{panel, CollectionBehavior, PanelBuilder, PanelLevel, StyleMask};

use controller::Action;

/// Set true only when the user really wants to quit (tray → Esci).
static QUIT: AtomicBool = AtomicBool::new(false);

/// Whether verbose diagnostic logging is enabled (env `CTL_TAB_DEBUG=1`).
pub(crate) fn debug_enabled() -> bool {
    static D: OnceLock<bool> = OnceLock::new();
    *D.get_or_init(|| {
        std::env::var("CTL_TAB_DEBUG")
            .map(|v| !v.is_empty() && v != "0")
            .unwrap_or(false)
    })
}

/// `eprintln!` only when `CTL_TAB_DEBUG` is set. Use for diagnostics.
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

// ---- Settings window + activation policy ----

/// Background apps run Accessory (no Dock icon). The Settings window needs focus
/// and keyboard, so switch to Regular while it's visible.
#[cfg(target_os = "macos")]
fn set_accessory(app: &AppHandle, accessory: bool) {
    let policy = if accessory {
        tauri::ActivationPolicy::Accessory
    } else {
        tauri::ActivationPolicy::Regular
    };
    let _ = app.set_activation_policy(policy);
}

fn show_settings(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(SETTINGS_LABEL) {
        // Regular so the window comes to the front and can take focus.
        #[cfg(target_os = "macos")]
        set_accessory(app, false);
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn hide_settings(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(SETTINGS_LABEL) {
        let _ = w.hide();
    }
    // Back to background (no Dock icon); also lets switch activation work.
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

// ---- Overlay panel ----

fn create_overlay(app: &AppHandle) -> tauri::Result<()> {
    let panel = PanelBuilder::<tauri::Wry, OverlayPanel>::new(app, OVERLAY_LABEL)
        .url(WebviewUrl::App("index.html".into()))
        .title("ctl-tab overlay")
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
        .has_shadow(false)
        .transparent(true)
        .build()?;

    panel.set_ignores_mouse_events(false);
    panel.set_accepts_mouse_moved_events(true);
    panel.hide();
    Ok(())
}

// ---- Tray ----

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let autostart_on = is_autostart_enabled(app);
    let settings_i = MenuItem::with_id(app, "settings", "Impostazioni…", true, None::<&str>)?;
    let autostart_i =
        CheckMenuItem::with_id(app, "autostart", "Avvia al login", true, autostart_on, None::<&str>)?;
    let quit_i = MenuItem::with_id(app, "quit", "Esci", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&settings_i, &autostart_i, &quit_i])?;

    let mut builder = TrayIconBuilder::new()
        .menu(&menu)
        .tooltip("ctl-tab")
        .on_menu_event(|app, event| match event.id().as_ref() {
            "settings" => show_settings(app),
            "autostart" => toggle_autostart(app),
            "quit" => {
                QUIT.store(true, Ordering::SeqCst);
                app.exit(0);
            }
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

// ---- Autostart (optional) ----

fn is_autostart_enabled(app: &AppHandle) -> bool {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().unwrap_or(false)
}

fn toggle_autostart(app: &AppHandle) {
    use tauri_plugin_autostart::ManagerExt;
    let mgr = app.autolaunch();
    let now = mgr.is_enabled().unwrap_or(false);
    let _ = if now { mgr.disable() } else { mgr.enable() };
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
            get_config,
            start_recording,
            save_config,
            reset_config
        ])
        .setup(|app| {
            let handle = app.handle();

            // Load persisted config into the shared state.
            let config_path: PathBuf = handle
                .path()
                .app_config_dir()
                .map(|d| d.join("config.json"))
                .unwrap_or_else(|_| PathBuf::from("ctl-tab-config.json"));
            controller::init(config_path);

            create_overlay(handle)?;
            build_tray(handle)?;

            // Settings window: closing it hides instead of quitting.
            if let Some(settings) = app.get_webview_window(SETTINGS_LABEL) {
                let h = handle.clone();
                settings.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        hide_settings(&h);
                    }
                });
            }
            // Opened from the launcher → show Settings.
            show_settings(handle);

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
        .run(|_app, event| {
            // Don't quit when the last window closes; only on explicit tray → Esci.
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                if !QUIT.load(Ordering::SeqCst) {
                    api.prevent_exit();
                }
            }
        });
}
