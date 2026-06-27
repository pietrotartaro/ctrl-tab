//! Real running-app data for the switcher (Phase 2).
//!
//! Non-unit-testable native code (objc2-app-kit): enumeration, icon rendering,
//! activation, and the NSWorkspace MRU observer. Gated by the manual acceptance
//! criteria. The pure ordering/filtering logic lives in `switcher`.

use std::collections::HashMap;
use std::ptr::NonNull;
use std::sync::{Mutex, OnceLock};

use base64::Engine;
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2_app_kit::{
    NSApplicationActivationOptions, NSApplicationActivationPolicy, NSBitmapImageFileType,
    NSBitmapImageRep, NSCompositingOperation, NSImage, NSRunningApplication,
    NSWorkspaceDidActivateApplicationNotification, NSWorkspace,
};
use objc2_foundation::{NSDictionary, NSNotification, NSPoint, NSRect, NSSize, NSString};

use crate::switcher::{filter_eligible, order_by_mru, promote_mru, AppItem, RawApp};

/// Process-wide app state shared between the gesture (main thread) and the
/// NSWorkspace activation observer (also main thread). Behind a Mutex for safety.
struct AppState {
    /// Most-recently-used pid order (front = most recent).
    mru: Vec<i32>,
    /// pid → PNG data URL, generated lazily and cached.
    icon_cache: HashMap<i32, String>,
}

fn app_state() -> &'static Mutex<AppState> {
    static STATE: OnceLock<Mutex<AppState>> = OnceLock::new();
    STATE.get_or_init(|| {
        Mutex::new(AppState {
            mru: Vec::new(),
            icon_cache: HashMap::new(),
        })
    })
}

fn own_pid() -> i32 {
    std::process::id() as i32
}

fn nsstring_to_string(s: Option<Retained<NSString>>) -> String {
    s.map(|s| s.to_string()).unwrap_or_default()
}

/// Enumerate running apps into the pure `RawApp` shape.
fn enumerate_raw() -> Vec<RawApp> {
    let workspace = NSWorkspace::sharedWorkspace();
    let apps = workspace.runningApplications();
    apps.iter()
        .map(|app| RawApp {
            pid: app.processIdentifier(),
            name: nsstring_to_string(app.localizedName()),
            regular: app.activationPolicy() == NSApplicationActivationPolicy::Regular,
        })
        .collect()
}

/// Render an app's icon to a 128px PNG data URL (non-deprecated block-based draw).
fn icon_data_url(app: &NSRunningApplication) -> Option<String> {
    let icon = app.icon()?;
    let size = NSSize::new(128.0, 128.0);

    let icon_for_block = icon.clone();
    let handler = RcBlock::new(move |dst: NSRect| -> Bool {
        icon_for_block.drawInRect_fromRect_operation_fraction(
            dst,
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0)),
            NSCompositingOperation::SourceOver,
            1.0,
        );
        Bool::YES
    });
    let scaled = NSImage::imageWithSize_flipped_drawingHandler(size, false, &handler);

    let tiff = scaled.TIFFRepresentation()?;
    let rep = NSBitmapImageRep::imageRepWithData(&tiff)?;
    let empty = NSDictionary::new();
    let png =
        unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &empty) }?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(png.to_vec());
    Some(format!("data:image/png;base64,{b64}"))
}

/// Find a running app by pid.
fn running_app(pid: i32) -> Option<Retained<NSRunningApplication>> {
    NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
}

/// Build the MRU-ordered, filtered list of eligible apps, refreshing the icon
/// cache. Also seeds the MRU so the current frontmost app is first.
pub fn build_ordered_apps() -> Vec<AppItem> {
    // Ensure the currently-frontmost app is at the front of the MRU, so index 0
    // is "current" and index 1 is "previous".
    let frontmost_pid = NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .map(|a| a.processIdentifier());

    let eligible = filter_eligible(enumerate_raw(), own_pid());
    let items: Vec<AppItem> = eligible
        .into_iter()
        .map(|r| AppItem {
            pid: r.pid,
            name: r.name,
        })
        .collect();

    let ordered = {
        let mut state = app_state().lock().unwrap();
        if let Some(front) = frontmost_pid {
            if items.iter().any(|a| a.pid == front) {
                promote_mru(&mut state.mru, front);
            }
        }
        order_by_mru(items, &state.mru)
    };

    // Refresh icon cache for any pid we don't have yet.
    for item in &ordered {
        let have = app_state().lock().unwrap().icon_cache.contains_key(&item.pid);
        if !have {
            if let Some(app) = running_app(item.pid) {
                if let Some(url) = icon_data_url(&app) {
                    app_state().lock().unwrap().icon_cache.insert(item.pid, url);
                }
            }
        }
    }

    let resolved = {
        let state = app_state().lock().unwrap();
        ordered
            .iter()
            .filter(|a| state.icon_cache.contains_key(&a.pid))
            .count()
    };
    eprintln!(
        "[ctl-tab] built {} apps, {} icons resolved (mru-ordered)",
        ordered.len(),
        resolved
    );

    ordered
}

/// Cached icon data URL for a pid (if `build_ordered_apps` resolved it).
pub fn icon_for(pid: i32) -> Option<String> {
    app_state().lock().unwrap().icon_cache.get(&pid).cloned()
}

/// Like `icon_for`, but renders + caches the icon if it isn't cached yet.
pub fn ensure_icon(pid: i32) -> Option<String> {
    if let Some(url) = icon_for(pid) {
        return Some(url);
    }
    let app = running_app(pid)?;
    let url = icon_data_url(&app)?;
    app_state().lock().unwrap().icon_cache.insert(pid, url.clone());
    Some(url)
}

/// The frontmost app's (pid, localized name), if any.
pub fn frontmost() -> Option<(i32, String)> {
    let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
    Some((app.processIdentifier(), nsstring_to_string(app.localizedName())))
}

/// Activate the app with the given pid, bringing it to the front.
pub fn activate(pid: i32) {
    if let Some(app) = running_app(pid) {
        // On macOS 14+ `ActivateIgnoringOtherApps` has no effect; ActivateAllWindows
        // brings the app and all its windows forward.
        let ok = app.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows);
        eprintln!("[ctl-tab] activate pid={pid} -> {ok}");
    } else {
        eprintln!("[ctl-tab] activate pid={pid} -> app not found");
    }
}

/// Subscribe to NSWorkspace activation notifications to keep the MRU current.
/// Must run on the main thread.
pub fn install_workspace_observer() {
    let block = RcBlock::new(|_notif: NonNull<NSNotification>| {
        if let Some(front) = NSWorkspace::sharedWorkspace()
            .frontmostApplication()
            .map(|a| a.processIdentifier())
        {
            if front != own_pid() {
                let mut state = app_state().lock().unwrap();
                promote_mru(&mut state.mru, front);
            }
        }
    });

    let workspace = NSWorkspace::sharedWorkspace();
    let center = workspace.notificationCenter();
    let token = unsafe {
        center.addObserverForName_object_queue_usingBlock(
            Some(NSWorkspaceDidActivateApplicationNotification),
            None,
            None,
            &block,
        )
    };
    // Keep the observer alive for the app's lifetime.
    std::mem::forget(token);
    eprintln!("[ctl-tab] NSWorkspace activation observer installed.");
}
