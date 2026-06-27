//! Test harness for the CGEventTap gesture pipeline.
//!
//! Posts real CGEvents via CGEventPost (which event taps DO observe, unlike
//! AppleScript/System Events synthetic keystrokes). Run while `tauri dev` is up:
//!
//!   cargo run --example post_keys -- <scenario>
//!
//! scenarios: probe | apps-fwd | apps-back | windows | esc
//!
//! Watch the app log for [ctl-tab] gesture_start / advance / commit / cancel.

use std::{thread, time::Duration};

use core_graphics::event::{
    CGEvent, CGEventFlags, CGEventTapLocation, CGEventType, CGMouseButton,
};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use core_graphics::geometry::CGPoint;

const KC_TAB: u16 = 48;
const KC_SECTION: u16 = 10;
const KC_ESC: u16 = 53;
const KC_LCTRL: u16 = 59;

fn src() -> CGEventSource {
    CGEventSource::new(CGEventSourceStateID::HIDSystemState).expect("event source")
}

fn pause() {
    thread::sleep(Duration::from_millis(120));
}

/// Post a key down/up for `keycode` with the given flags.
fn key(keycode: u16, down: bool, flags: CGEventFlags) {
    let ev = CGEvent::new_keyboard_event(src(), keycode, down).expect("keyboard event");
    ev.set_flags(flags);
    ev.post(CGEventTapLocation::HID);
}

fn ctrl_down() {
    key(KC_LCTRL, true, CGEventFlags::CGEventFlagControl);
    pause();
}

fn ctrl_up() {
    // Releasing control: flags now have no control bit → tap should commit.
    key(KC_LCTRL, false, CGEventFlags::CGEventFlagNull);
    pause();
}

fn tap_key(keycode: u16, flags: CGEventFlags) {
    key(keycode, true, flags);
    key(keycode, false, flags);
    pause();
}

fn mouse_click(x: f64, y: f64) {
    let pt = CGPoint::new(x, y);
    for ty in [
        CGEventType::MouseMoved,
        CGEventType::LeftMouseDown,
        CGEventType::LeftMouseUp,
    ] {
        let ev = CGEvent::new_mouse_event(src(), ty, pt, CGMouseButton::Left)
            .expect("mouse event");
        ev.post(CGEventTapLocation::HID);
        thread::sleep(Duration::from_millis(40));
    }
}

fn main() {
    let scenario = std::env::args().nth(1).unwrap_or_else(|| "apps-fwd".into());
    println!("[post_keys] running scenario: {scenario}");
    let ctrl = CGEventFlags::CGEventFlagControl;
    let ctrl_shift = CGEventFlags::CGEventFlagControl | CGEventFlags::CGEventFlagShift;

    match scenario.as_str() {
        // Just emit control down + up + one tab to learn how the tap sees them.
        "probe" => {
            ctrl_down();
            tap_key(KC_TAB, ctrl);
            ctrl_up();
        }
        // Hold the gesture open for a few seconds (for screenshots), then release.
        // Optional 2nd arg = number of extra Tab advances while held.
        "hold" => {
            let extra: usize = std::env::args()
                .nth(2)
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            ctrl_down();
            tap_key(KC_TAB, ctrl);
            for _ in 0..extra {
                tap_key(KC_TAB, ctrl);
            }
            thread::sleep(Duration::from_millis(3000));
            ctrl_up();
        }
        // Hold the WINDOWS gesture (Ctrl+§) open for screenshots, optional extra §.
        "hold-win" => {
            let extra: usize = std::env::args()
                .nth(2)
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            ctrl_down();
            tap_key(KC_SECTION, ctrl);
            for _ in 0..extra {
                tap_key(KC_SECTION, ctrl);
            }
            thread::sleep(Duration::from_millis(3000));
            ctrl_up();
        }
        // Ctrl + <keycode> tap+release. Used to record a combo and to test a
        // rebound Ctrl+<key> app-switch.
        "ctrlkey" => {
            let kc: u16 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(48);
            ctrl_down();
            tap_key(kc, ctrl);
            ctrl_up();
        }
        // Single Ctrl+Tab+release → start(selected=1), commit(1) = previous app.
        "apps-one" => {
            ctrl_down();
            tap_key(KC_TAB, ctrl);
            ctrl_up();
        }
        // Just a mouse click at (x,y) — use during an existing `hold`.
        "mouseonly" => {
            let x: f64 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(0.0);
            let y: f64 = std::env::args().nth(3).and_then(|s| s.parse().ok()).unwrap_or(0.0);
            mouse_click(x, y);
        }
        // Just a mouse move to (x,y) — use during an existing `hold` to test hover.
        "moveonly" => {
            let x: f64 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(0.0);
            let y: f64 = std::env::args().nth(3).and_then(|s| s.parse().ok()).unwrap_or(0.0);
            let ev = CGEvent::new_mouse_event(src(), CGEventType::MouseMoved, CGPoint::new(x, y), CGMouseButton::Left).unwrap();
            ev.post(CGEventTapLocation::HID);
        }
        // Open the gesture, click at (x,y) on an item, then release Ctrl.
        "click" => {
            let x: f64 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(0.0);
            let y: f64 = std::env::args().nth(3).and_then(|s| s.parse().ok()).unwrap_or(0.0);
            ctrl_down();
            tap_key(KC_TAB, ctrl);
            thread::sleep(Duration::from_millis(500));
            mouse_click(x, y);
            thread::sleep(Duration::from_millis(500));
            ctrl_up();
        }
        // Ctrl held, Tab x3 forward, release → start(1), +1, +1, commit(3).
        "apps-fwd" => {
            ctrl_down();
            tap_key(KC_TAB, ctrl);
            tap_key(KC_TAB, ctrl);
            tap_key(KC_TAB, ctrl);
            ctrl_up();
        }
        // Ctrl held, Tab (start→1), then Shift+Tab x2 backward (→0, →4), release → commit(4).
        "apps-back" => {
            ctrl_down();
            tap_key(KC_TAB, ctrl);
            tap_key(KC_TAB, ctrl_shift);
            tap_key(KC_TAB, ctrl_shift);
            ctrl_up();
        }
        // Windows mode via § (keycode 10): start→1, +1, release → commit(2).
        "windows" => {
            ctrl_down();
            tap_key(KC_SECTION, ctrl);
            tap_key(KC_SECTION, ctrl);
            ctrl_up();
        }
        // Ctrl held, Tab (start→1), Esc → cancel; release → no commit.
        "esc" => {
            ctrl_down();
            tap_key(KC_TAB, ctrl);
            tap_key(KC_ESC, ctrl);
            ctrl_up();
        }
        other => {
            eprintln!("unknown scenario: {other}");
            std::process::exit(2);
        }
    }

    println!("[post_keys] done");
}
