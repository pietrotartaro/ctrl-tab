//! Window enumeration + raising for the frontmost app, via the Accessibility API.
//!
//! Non-unit-testable native code (raw AX FFI): gated by the manual acceptance
//! criteria. All calls here run on the main thread.
//!
//! The switcher list index lines up 1:1 with `STATE.windows`, so the switcher's
//! selected index is what `raise(index)` operates on.

use std::ffi::c_void;
use std::ptr;
use std::sync::{Mutex, OnceLock};

use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::string::{CFString, CFStringRef as CfStringRef};

// ---- Opaque CF / AX pointer aliases ----
type AXUIElementRef = *mut c_void;
type CFTypeRef = *const c_void;
type CFStringRef = *const c_void;
type CFArrayRef = *const c_void;
type CFIndex = isize;
type AXError = i32;

const KAX_SUCCESS: AXError = 0;

// AX attribute / role / action names (their string values are stable).
const ATTR_WINDOWS: &str = "AXWindows";
const ATTR_TITLE: &str = "AXTitle";
const ATTR_ROLE: &str = "AXRole";
const ATTR_MAIN: &str = "AXMain";
const ROLE_WINDOW: &str = "AXWindow";
const ACTION_RAISE: &str = "AXRaise";

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> AXError;
    fn AXUIElementSetAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: CFTypeRef,
    ) -> AXError;
    fn AXUIElementPerformAction(element: AXUIElementRef, action: CFStringRef) -> AXError;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFArrayGetCount(array: CFArrayRef) -> CFIndex;
    fn CFArrayGetValueAtIndex(array: CFArrayRef, idx: CFIndex) -> *const c_void;
    fn CFRetain(cf: CFTypeRef) -> CFTypeRef;
    fn CFRelease(cf: CFTypeRef);
}

/// VS Code's default window-title segment separator (em dash with spaces).
const VSCODE_SEP: &str = " — ";

/// Bundle identifiers of VS Code (stable + Insiders).
pub fn is_vscode_bundle(bundle_id: &str) -> bool {
    matches!(bundle_id, "com.microsoft.VSCode" | "com.microsoft.VSCodeInsiders")
}

/// Reorder a VS Code window title from "file — project" to "project — file"
/// (swap the first two " — " segments).
///
/// Pure string logic. VS Code's AX window title is "file — project" (the app name
/// is NOT part of the AX title), so 2 segments is the normal case. If the title
/// has fewer than 2 segments (e.g. no folder open → just the file name, or a
/// custom title that doesn't match), it is returned unchanged.
pub fn reorder_vscode_title(title: &str) -> String {
    let mut parts: Vec<&str> = title.split(VSCODE_SEP).collect();
    if parts.len() >= 2 {
        parts.swap(0, 1);
        parts.join(VSCODE_SEP)
    } else {
        title.to_string()
    }
}

/// A retained AX window element. Only ever touched on the main thread.
struct AxWin(AXUIElementRef);
unsafe impl Send for AxWin {}
unsafe impl Sync for AxWin {}
impl Drop for AxWin {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0 as CFTypeRef) };
        }
    }
}

struct WinState {
    owner_pid: i32,
    windows: Vec<AxWin>,
}

fn state() -> &'static Mutex<WinState> {
    static S: OnceLock<Mutex<WinState>> = OnceLock::new();
    S.get_or_init(|| {
        Mutex::new(WinState {
            owner_pid: 0,
            windows: Vec::new(),
        })
    })
}

fn cfstr(s: &str) -> CFString {
    CFString::new(s)
}

/// Copy an attribute as a raw CFTypeRef (create rule — caller owns it).
fn copy_attr(el: AXUIElementRef, attr: &str) -> Option<CFTypeRef> {
    let key = cfstr(attr);
    let mut out: CFTypeRef = ptr::null();
    let err = unsafe {
        AXUIElementCopyAttributeValue(el, key.as_concrete_TypeRef() as CFStringRef, &mut out)
    };
    if err == KAX_SUCCESS && !out.is_null() {
        Some(out)
    } else {
        None
    }
}

/// Copy a CFString attribute as a Rust String.
fn copy_string_attr(el: AXUIElementRef, attr: &str) -> Option<String> {
    let v = copy_attr(el, attr)?;
    let s = unsafe { CFString::wrap_under_create_rule(v as CfStringRef) };
    Some(s.to_string())
}

/// Enumerate standard, titled windows of `pid`. Stores the AX refs (same order as
/// the returned titles) and returns the titles. Returns empty if none.
pub fn enumerate(pid: i32) -> Vec<String> {
    let app = unsafe { AXUIElementCreateApplication(pid) };
    if app.is_null() {
        return Vec::new();
    }

    let mut titles = Vec::new();
    let mut wins = Vec::new();

    if let Some(arr_ref) = copy_attr(app, ATTR_WINDOWS) {
        let arr = arr_ref as CFArrayRef;
        let count = unsafe { CFArrayGetCount(arr) };
        for i in 0..count {
            let w = unsafe { CFArrayGetValueAtIndex(arr, i) } as AXUIElementRef;
            if w.is_null() {
                continue;
            }
            // Standard windows only.
            if copy_string_attr(w, ATTR_ROLE).as_deref() != Some(ROLE_WINDOW) {
                continue;
            }
            let title = copy_string_attr(w, ATTR_TITLE).unwrap_or_default();
            if title.trim().is_empty() {
                continue;
            }
            // The array owns `w`; retain it so it outlives the array.
            unsafe { CFRetain(w as CFTypeRef) };
            wins.push(AxWin(w));
            titles.push(title);
        }
        unsafe { CFRelease(arr_ref) };
    }

    unsafe { CFRelease(app as CFTypeRef) };

    let mut st = state().lock().unwrap();
    st.owner_pid = pid;
    st.windows = wins;

    crate::dlog!("[ctrl-tab] windows: enumerated {} for pid={}", titles.len(), pid);
    titles
}

/// Raise the window at `index`: AXRaise + set it main, then activate the app.
pub fn raise(index: usize) {
    let owner = {
        let st = state().lock().unwrap();
        if let Some(w) = st.windows.get(index) {
            let el = w.0;
            unsafe {
                AXUIElementPerformAction(
                    el,
                    cfstr(ACTION_RAISE).as_concrete_TypeRef() as CFStringRef,
                );
                let truth = CFBoolean::true_value();
                AXUIElementSetAttributeValue(
                    el,
                    cfstr(ATTR_MAIN).as_concrete_TypeRef() as CFStringRef,
                    truth.as_concrete_TypeRef() as CFTypeRef,
                );
            }
            crate::dlog!("[ctrl-tab] windows: raised index={index}");
        } else {
            crate::dlog!("[ctrl-tab] windows: raise index={index} out of range");
        }
        st.owner_pid
    };
    // Bring the owning app forward so the raised window is actually focused.
    crate::apps::activate(owner);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reorders_two_segments_file_project() {
        // The real VS Code AXTitle is "file — project" (no trailing app name).
        assert_eq!(
            reorder_vscode_title("boost.json — backend"),
            "backend — boost.json"
        );
        assert_eq!(
            reorder_vscode_title("App.tsx (Working Tree) (App.tsx) — ctrl-tab"),
            "ctrl-tab — App.tsx (Working Tree) (App.tsx)"
        );
    }

    #[test]
    fn single_segment_unchanged() {
        // No folder open: title is just the file → fallback, unchanged.
        assert_eq!(reorder_vscode_title("Untitled-1"), "Untitled-1");
    }

    #[test]
    fn reorders_three_segments_swaps_first_two() {
        // Defensive: if a trailing segment ever appears, still file<->project only.
        assert_eq!(
            reorder_vscode_title("a — b — c"),
            "b — a — c"
        );
    }

    #[test]
    fn no_separator_unchanged() {
        assert_eq!(reorder_vscode_title("PlainTitle"), "PlainTitle");
    }

    #[test]
    fn unsaved_marker_stays_on_file_segment() {
        // The ● dirty marker rides along with the file segment.
        assert_eq!(
            reorder_vscode_title("● app.tsx — my-project"),
            "my-project — ● app.tsx"
        );
    }

    #[test]
    fn recognizes_vscode_bundle_ids() {
        assert!(is_vscode_bundle("com.microsoft.VSCode"));
        assert!(is_vscode_bundle("com.microsoft.VSCodeInsiders"));
        assert!(!is_vscode_bundle("com.jetbrains.PhpStorm"));
        assert!(!is_vscode_bundle(""));
    }
}
