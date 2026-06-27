// ctl-tab — macOS Alt-Tab clone (Tauri v2, single Rust binary).
//
// A background utility: Ctrl-Tab switches apps, Ctrl-§ switches windows of the
// frontmost app. The overlay is a transparent, non-activating NSPanel
// (tauri-nspanel). The app runs as Accessory (no Dock icon). See CLAUDE.md.

mod apps;
mod controller;
mod events;
mod hotkey;
mod switcher;
mod windows;

use std::sync::OnceLock;

// These traits must be in scope (unqualified) for the panel! macro expansion.
use objc2::runtime::NSObjectProtocol;
use objc2::{ClassType, Message};
use tauri::{AppHandle, Manager, WebviewUrl};
use tauri_nspanel::{panel, CollectionBehavior, PanelBuilder, PanelLevel, StyleMask};

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

// Define the concrete NSPanel subclass used for the overlay. It can become the
// key window so the webview inside it receives mouse/keyboard events even though
// the app itself never activates (NonactivatingPanel style mask, set below).
panel!(OverlayPanel {
    config: {
        can_become_key_window: true
    }
});

const OVERLAY_LABEL: &str = "overlay";

/// Mouse hover over a switcher item: update selection + re-emit `switcher:select`.
#[tauri::command]
fn switcher_hover(app: AppHandle, index: usize) {
    controller::hover(&app, index);
}

/// Mouse click on a switcher item: set index, activate/raise, hide overlay, reset
/// state (so the later Ctrl release does not double-commit). Runs on the main thread.
#[tauri::command]
fn switcher_commit(app: AppHandle, index: usize) -> Result<(), String> {
    let app2 = app.clone();
    app.run_on_main_thread(move || controller::commit_index(&app2, index))
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
    // Deliver mouse-moved events so the webview's hover (onMouseEnter) fires.
    panel.set_accepts_mouse_moved_events(true);
    // Ensure it starts hidden regardless of builder ordering.
    panel.hide();

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_nspanel::init())
        .invoke_handler(tauri::generate_handler![switcher_hover, switcher_commit])
        .setup(|app| {
            // Background utility: no Dock icon, no menu bar.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            create_overlay(app.handle())?;

            // Native gesture pipeline: check Accessibility, observe app activations
            // for MRU, install the event tap.
            #[cfg(target_os = "macos")]
            {
                hotkey::ensure_accessibility();
                apps::install_workspace_observer();
                hotkey::install_event_tap(app.handle().clone());
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
