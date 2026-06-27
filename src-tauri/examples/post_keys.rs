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

use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

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
        // Ctrl held, Tab x3 forward, release → start, +1, +1, +1, commit(3).
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
