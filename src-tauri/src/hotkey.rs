//! Native CGEventTap that recognises the Alt-Tab-style gesture.
//!
//! This is non-unit-testable native code (per the TDD policy): it is gated by the
//! phase's manual acceptance criteria. The pure index math lives in `switcher`.
//!
//! Pipeline: an ACTIVE session-level event tap (head-insert) listens for keyDown
//! and flagsChanged. The extern "C" callback receives the `Switcher` state through
//! the `userInfo` refcon pointer and stays light. While a switcher is active it
//! CONSUMES Tab/§ keyDowns (returns NULL) so the app underneath never sees them.

use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::{CFString, CFStringRef as CFCfStringRef};
use tauri::AppHandle;

use crate::controller;

/// Set once the event tap is successfully created + wired.
static TAP_INSTALLED: AtomicBool = AtomicBool::new(false);

// ---- Opaque C pointer aliases ----
type CFMachPortRef = *mut c_void;
type CFRunLoopRef = *mut c_void;
type CFRunLoopSourceRef = *mut c_void;
type CFAllocatorRef = *const c_void;
type CFStringRef = *const c_void;
type CFDictionaryRef = *const c_void;
type CFIndex = isize;
type CGEventRef = *mut c_void;
type CGEventTapProxy = *const c_void;
type CGEventTapCallBack =
    unsafe extern "C" fn(CGEventTapProxy, u32, CGEventRef, *mut c_void) -> CGEventRef;

// ---- CoreGraphics constants ----
const KCG_SESSION_EVENT_TAP: u32 = 1; // kCGSessionEventTap
const KCG_HEAD_INSERT_EVENT_TAP: u32 = 0; // kCGHeadInsertEventTap
const KCG_EVENT_TAP_OPTION_DEFAULT: u32 = 0; // active (NOT listen-only)
const KCG_EVENT_KEY_DOWN: u32 = 10;
const KCG_EVENT_FLAGS_CHANGED: u32 = 12;
const KCG_EVENT_TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
const KCG_EVENT_TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFF_FFFF;
const KCG_KEYBOARD_EVENT_KEYCODE: u32 = 9; // kCGKeyboardEventKeycode

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: u64,
        callback: CGEventTapCallBack,
        user_info: *mut c_void,
    ) -> CFMachPortRef;
    fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    fn CGEventGetFlags(event: CGEventRef) -> u64;
    fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFRunLoopCommonModes: CFStringRef;
    fn CFRunLoopGetMain() -> CFRunLoopRef;
    fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
    fn CFMachPortCreateRunLoopSource(
        allocator: CFAllocatorRef,
        port: CFMachPortRef,
        order: CFIndex,
    ) -> CFRunLoopSourceRef;
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
    fn AXIsProcessTrusted() -> bool;
    static kAXTrustedCheckOptionPrompt: CFStringRef;
}

/// Whether the process is currently trusted for Accessibility (no prompt).
fn process_trusted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

/// State handed to the callback through the refcon pointer. Lives for the app's
/// lifetime (intentionally leaked). Switcher state lives in `controller`.
struct TapState {
    app: AppHandle,
    tap: CFMachPortRef,
}

/// The extern "C" event-tap callback. Kept light; mutates `TapState` via refcon.
unsafe extern "C" fn tap_callback(
    _proxy: CGEventTapProxy,
    etype: u32,
    event: CGEventRef,
    user_info: *mut c_void,
) -> CGEventRef {
    let state = &mut *(user_info as *mut TapState);

    match etype {
        KCG_EVENT_TAP_DISABLED_BY_TIMEOUT | KCG_EVENT_TAP_DISABLED_BY_USER_INPUT => {
            crate::dlog!("[ctrl-tab] event tap disabled ({etype:#x}); re-enabling");
            if !state.tap.is_null() {
                CGEventTapEnable(state.tap, true);
            }
            event
        }
        KCG_EVENT_KEY_DOWN => {
            let flags = CGEventGetFlags(event);
            let keycode = CGEventGetIntegerValueField(event, KCG_KEYBOARD_EVENT_KEYCODE);
            // All matching (config-driven combos + recording capture) lives in the
            // controller; it tells us whether to consume the key.
            if controller::handle_key_down(&state.app, keycode, flags) {
                return ptr::null_mut();
            }
            event
        }
        KCG_EVENT_FLAGS_CHANGED => {
            let flags = CGEventGetFlags(event);
            controller::handle_flags_changed(&state.app, flags);
            event
        }
        _ => event,
    }
}

/// Prompt for / check Accessibility trust. Without it the tap receives no events.
pub fn ensure_accessibility() -> bool {
    unsafe {
        let key = CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt as CFCfStringRef);
        let value = CFBoolean::true_value();
        let dict = CFDictionary::from_CFType_pairs(&[(key.as_CFType(), value.as_CFType())]);
        let trusted = AXIsProcessTrustedWithOptions(dict.as_concrete_TypeRef() as CFDictionaryRef);
        if trusted {
            crate::dlog!("[ctrl-tab] Accessibility granted.");
        } else {
            eprintln!("[ctrl-tab] Accessibility NOT granted yet — shortcuts are inactive.");
            eprintln!("[ctrl-tab] Grant it in System Settings → Privacy & Security → Accessibility:");
            eprintln!("[ctrl-tab]   • in `tauri dev`, enable the host terminal app (it owns the process);");
            eprintln!("[ctrl-tab]   • for a bundled build, enable \"ctrl-tab\".");
            eprintln!("[ctrl-tab] The app will start working automatically once granted (no restart needed).");
        }
        trusted
    }
}

/// Try to create the event tap, wire it to the main run loop, and enable it.
/// Returns true on success. Must run on the main thread. Idempotent: no-op once
/// installed. Fails (returns false) if the process isn't Accessibility-trusted.
fn try_install_tap(app: &AppHandle) -> bool {
    if TAP_INSTALLED.load(Ordering::SeqCst) {
        return true;
    }
    unsafe {
        let state = Box::new(TapState {
            app: app.clone(),
            tap: ptr::null_mut(),
        });
        let state_ptr = Box::into_raw(state);

        let mask: u64 = (1u64 << KCG_EVENT_KEY_DOWN) | (1u64 << KCG_EVENT_FLAGS_CHANGED);
        let tap = CGEventTapCreate(
            KCG_SESSION_EVENT_TAP,
            KCG_HEAD_INSERT_EVENT_TAP,
            KCG_EVENT_TAP_OPTION_DEFAULT,
            mask,
            tap_callback,
            state_ptr as *mut c_void,
        );

        if tap.is_null() {
            drop(Box::from_raw(state_ptr)); // reclaim; will retry later
            return false;
        }

        (*state_ptr).tap = tap;
        let source = CFMachPortCreateRunLoopSource(ptr::null(), tap, 0);
        CFRunLoopAddSource(CFRunLoopGetMain(), source, kCFRunLoopCommonModes);
        CGEventTapEnable(tap, true);

        TAP_INSTALLED.store(true, Ordering::SeqCst);
        crate::dlog!(
            "[ctrl-tab] CGEventTap installed (session tap, head-insert, keyDown+flagsChanged, active)."
        );
        // state_ptr / tap / source intentionally leaked for the app's lifetime.
        true
    }
}

/// Install the event tap. If the app isn't Accessibility-trusted yet (the normal
/// first-launch case), poll in the background and install as soon as the user
/// grants the permission — no manual relaunch required.
pub fn install_event_tap(app: AppHandle) {
    if try_install_tap(&app) {
        return;
    }
    eprintln!("[ctrl-tab] event tap not installed yet — waiting for Accessibility to be granted…");

    std::thread::spawn(move || {
        // Poll for up to ~30 minutes; stop as soon as the tap is installed.
        for _ in 0..1200 {
            if TAP_INSTALLED.load(Ordering::SeqCst) {
                break;
            }
            std::thread::sleep(Duration::from_millis(1500));
            if !process_trusted() {
                continue;
            }
            let app = app.clone();
            // Tap creation + run-loop wiring must happen on the main thread.
            let _ = app.clone().run_on_main_thread(move || {
                if !TAP_INSTALLED.load(Ordering::SeqCst) && try_install_tap(&app) {
                    eprintln!("[ctrl-tab] Accessibility granted — shortcuts are now active.");
                }
            });
        }
    });
}
