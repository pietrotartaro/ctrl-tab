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

use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::{CFString, CFStringRef as CFCfStringRef};

use crate::switcher::{Mode, Switcher};

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

// CGEventFlags masks
const MASK_CONTROL: u64 = 0x0004_0000; // kCGEventFlagMaskControl
const MASK_SHIFT: u64 = 0x0002_0000; // kCGEventFlagMaskShift

// macOS virtual key codes
const KEY_TAB: i64 = 48;
const KEY_SECTION: i64 = 10; // ISO § / ± key
const KEY_ESC: i64 = 53;

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
    static kAXTrustedCheckOptionPrompt: CFStringRef;
}

/// State handed to the callback through the refcon pointer. Lives for the app's
/// lifetime (intentionally leaked).
struct TapState {
    switcher: Switcher,
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
            eprintln!("[ctl-tab] event tap disabled ({etype:#x}); re-enabling");
            if !state.tap.is_null() {
                CGEventTapEnable(state.tap, true);
            }
            event
        }
        KCG_EVENT_KEY_DOWN => {
            let flags = CGEventGetFlags(event);
            let keycode = CGEventGetIntegerValueField(event, KCG_KEYBOARD_EVENT_KEYCODE);
            let ctrl = flags & MASK_CONTROL != 0;
            let shift = flags & MASK_SHIFT != 0;

            if ctrl && (keycode == KEY_TAB || keycode == KEY_SECTION) {
                let mode = if keycode == KEY_TAB {
                    Mode::Apps
                } else {
                    Mode::Windows
                };
                if shift {
                    state.switcher.advance(-1);
                } else if !state.switcher.active {
                    let items = match mode {
                        Mode::Apps => crate::apps::build_ordered_apps(),
                        // Windows mode is populated in a later phase; keep a stub list.
                        Mode::Windows => (0..5)
                            .map(|i| crate::switcher::AppItem {
                                pid: -(i as i32),
                                name: format!("window-{i}"),
                            })
                            .collect(),
                    };
                    // Start on the previous item (index 1) so a single Ctrl+Tap+release
                    // jumps to the last-used app.
                    let selected = if items.len() > 1 { 1 } else { 0 };
                    state.switcher.start(mode, items, selected);
                } else {
                    state.switcher.advance(1);
                }
                // Consume Tab/§ so they never reach the app underneath.
                if state.switcher.active {
                    return ptr::null_mut();
                }
                return event;
            }

            if keycode == KEY_ESC && state.switcher.active {
                state.switcher.cancel();
                return ptr::null_mut();
            }

            event
        }
        KCG_EVENT_FLAGS_CHANGED => {
            let flags = CGEventGetFlags(event);
            let ctrl = flags & MASK_CONTROL != 0;
            // Ctrl released while active → confirm the selection and activate it.
            if !ctrl && state.switcher.active {
                let mode = state.switcher.mode;
                if let Some(item) = state.switcher.commit() {
                    if mode == Mode::Apps {
                        crate::apps::activate(item.pid);
                    }
                }
            }
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
            println!("[ctl-tab] Accessibility granted.");
        } else {
            eprintln!("[ctl-tab] Accessibility NOT granted — the CGEventTap will receive NO events.");
            eprintln!("[ctl-tab] Grant it in System Settings → Privacy & Security → Accessibility:");
            eprintln!("[ctl-tab]   • in `tauri dev`, enable the host terminal app (it owns the process);");
            eprintln!("[ctl-tab]   • for a bundled build, enable \"ctl-tab\".");
            eprintln!("[ctl-tab] Then restart the app.");
        }
        trusted
    }
}

/// Create the event tap, wire it to the main run loop, and enable it.
/// Must run on the main thread (Tauri's setup does).
pub fn install_event_tap() {
    unsafe {
        let state = Box::new(TapState {
            switcher: Switcher::new(),
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
            eprintln!("[ctl-tab] failed to create CGEventTap (is Accessibility granted?)");
            drop(Box::from_raw(state_ptr)); // reclaim on failure
            return;
        }

        (*state_ptr).tap = tap;

        let source = CFMachPortCreateRunLoopSource(ptr::null(), tap, 0);
        CFRunLoopAddSource(CFRunLoopGetMain(), source, kCFRunLoopCommonModes);
        CGEventTapEnable(tap, true);

        println!(
            "[ctl-tab] CGEventTap installed (session tap, head-insert, keyDown+flagsChanged, active)."
        );
        // state_ptr / tap / source intentionally leaked for the app's lifetime.
    }
}
