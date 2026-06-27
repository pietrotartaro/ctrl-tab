// ctl-tab — macOS Alt-Tab clone. Phase 0: foundations.
//
// This phase wires up a transparent, non-activating NSPanel overlay via
// tauri-nspanel (branch v2.1), sets the app to Accessory (no Dock icon),
// and exposes show_overlay / hide_overlay test commands. No hotkeys or
// native switching logic yet.

// These two traits must be in scope (unqualified) for the panel! macro expansion.
mod hotkey;
mod switcher;

use objc2::runtime::NSObjectProtocol;
use objc2::{ClassType, Message};
use tauri::{AppHandle, Manager, WebviewUrl};
use tauri_nspanel::{
    panel, CollectionBehavior, ManagerExt, PanelBuilder, PanelLevel, StyleMask,
};

// Define the concrete NSPanel subclass used for the overlay. It can become the
// key window so the webview inside it receives mouse/keyboard events even though
// the app itself never activates (NonactivatingPanel style mask, set below).
panel!(OverlayPanel {
    config: {
        can_become_key_window: true
    }
});

const OVERLAY_LABEL: &str = "overlay";

/// Show the overlay panel. Runs on the main thread (NSPanel is main-thread only).
#[tauri::command]
fn show_overlay(app: AppHandle) -> Result<(), String> {
    let panel = app
        .get_webview_panel(OVERLAY_LABEL)
        .map_err(|e| format!("overlay panel not found: {e:?}"))?;
    app.run_on_main_thread(move || panel.show())
        .map_err(|e| e.to_string())
}

/// Hide the overlay panel. Runs on the main thread.
#[tauri::command]
fn hide_overlay(app: AppHandle) -> Result<(), String> {
    let panel = app
        .get_webview_panel(OVERLAY_LABEL)
        .map_err(|e| format!("overlay panel not found: {e:?}"))?;
    app.run_on_main_thread(move || panel.hide())
        .map_err(|e| e.to_string())
}

/// Build the overlay NSPanel: transparent, borderless, floating, on all Spaces,
/// non-activating, born hidden + centered, receiving mouse clicks.
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
                .visible(false) // born hidden
                .center()
                .inner_size(720.0, 220.0)
        })
        // borderless + non-activating: clicking the panel never activates the app
        .style_mask(StyleMask::empty().borderless().nonactivating_panel())
        .floating(true)
        .level(PanelLevel::Floating)
        // Stay visible when the app/panel deactivates; we control hiding explicitly.
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

    // Receive mouse events (hover/click) — explicit per the spec.
    panel.set_ignores_mouse_events(false);
    // Ensure it starts hidden regardless of builder ordering.
    panel.hide();

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_nspanel::init())
        .invoke_handler(tauri::generate_handler![show_overlay, hide_overlay])
        .setup(|app| {
            // Background utility: no Dock icon, no menu bar.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            create_overlay(app.handle())?;

            // Native gesture pipeline (Phase 1): check Accessibility, install the tap.
            #[cfg(target_os = "macos")]
            {
                hotkey::ensure_accessibility();
                hotkey::install_event_tap();
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
