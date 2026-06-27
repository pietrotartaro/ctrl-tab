# ctl-tab — macOS Alt-Tab clone (Tauri v2, single Rust binary)

A background utility that reimplements Alt-Tab on macOS:

- **Ctrl-Tab** → app switcher
- **Ctrl-§** → window switcher (windows of the front app / all apps)

Alt-Tab-style overlay: medium icons, the focused window title shown at the top,
navigation with Tab / Shift+Tab, mouse selection (hover + click).

## Architecture

- **Single binary.** Everything (hotkey capture, app/window enumeration, overlay,
  activation) lives in one Rust process. **No sidecar.**
- **Frontend:** React + TypeScript + Tailwind, built by Vite. The same bundle is
  loaded by every window; each view branches on the window label
  (`getCurrentWindow().label`).
- **Overlay:** an `NSPanel` created with `tauri-nspanel` (git, branch `v2.1`) via
  `PanelBuilder`. Non-activating, floating, transparent, on all Spaces, receives
  mouse clicks. Born hidden + centered.
- **Activation policy:** `Accessory` — no Dock icon, no menu bar.

## Stack & constraints (do not change without asking)

- Tauri v2, single Rust binary, frontend React + TS + Tailwind + Vite.
- Overlay: `tauri-nspanel` branch `v2.1`. Panel config: transparent,
  `decorations(false)`, `always_on_top(true)`, `skip_taskbar(true)`,
  `resizable(false)`, `no_activate(true)`, `level Floating`, collection behavior
  `can_join_all_spaces` + `ignores_cycle`, `ignoresMouseEvents = false`.
- Hotkeys: native **CGEventTap** (NOT the global-shortcut plugin). [Phase 1 ✓]
- Native macOS via `objc2` / `objc2-app-kit`. Window enumeration, titles and
  raising via the **Accessibility API**. [Phase ≥2]
- Permissions: **Accessibility only. No Screen Recording.**

> **Accessibility is mandatory for the hotkey.** An active CGEventTap on
> `kCGSessionEventTap` only receives events if the responsible process is trusted
> for Accessibility. Without it the tap installs but sees **no** events (no gesture
> works). In `tauri dev` the responsible process is the host terminal — grant
> Accessibility to the terminal; for a bundled build, grant it to `ctl-tab`. The
> app calls `AXIsProcessTrustedWithOptions` with the prompt at startup and logs
> instructions if untrusted.

### Gesture (Phase 1)

- Modifier = **Ctrl** (`kCGEventFlagMaskControl`). Virtual key codes: Tab = 48,
  ISO § = 10, Esc = 53, left Ctrl = 59.
- Ctrl+Tab → apps mode; Ctrl+§ → windows mode. First press starts the switcher
  (selected = 0) **and** advances +1; subsequent presses advance. Shift reverses
  direction (advance −1). Releasing Ctrl (flagsChanged without Control) commits;
  Esc cancels. While active, Tab/§ keyDowns are **consumed** (callback returns
  `NULL`) so they never reach the app underneath.
- The tap is active (`kCGEventTapOptionDefault`), head-insert, session-level, on
  the main run loop (`CFRunLoopGetMain`). It re-enables itself on
  `kCGEventTapDisabledByTimeout` / `ByUserInput`.
- Phase 1 only logs diagnostics (to **stderr**) over a fake 5-item list — no real
  app/window enumeration yet.

## Switcher state contract (used across phases)

### Rust → frontend events

- `switcher:show` — payload:
  ```ts
  {
    mode: "apps" | "windows",
    items: { id: string; title: string; appName: string; iconDataUrl: string }[],
    selected: number
  }
  ```
- `switcher:select` — payload: `{ selected: number }`
- `switcher:hide` — no payload

### Frontend → Rust commands (mouse)

- `switcher_hover(index)` — set the selected index in Rust state and re-emit
  `switcher:select` (keeps title + highlight in sync with keyboard navigation).
- `switcher_commit(index)` — set the index, commit (activate app/window), hide the
  overlay, reset state.

> Phase 0 only ships the scaffolding commands `show_overlay` / `hide_overlay`
> (manual test buttons). The contract above is implemented in later phases.

## TDD policy (from the session Preludio)

Real red/green/refactor TDD applies **only to pure, unit-testable logic**:

- selection index with wrap-around (advance ±1 over a list of length N)
- MRU ordering of the app list
- eligible-app filter (`.regular`, excluding our own app)
- serialization of event payloads (`switcher:show` / `switcher:select`)

**Do NOT impose unit tests on non-unit-testable native code:** CGEventTap/gesture,
NSPanel/overlay, Accessibility (enumeration + window raising), icon generation, app
activation. Those are gated by each phase's **manual Acceptance Criteria** plus smoke
tests where they make sense. Never delete native code merely because "a test is
missing" — for that code a unit test is not required.

## Layout

- `src/` — React/TS frontend (`App.tsx` branches on window label).
- `src-tauri/src/lib.rs` — Tauri app: plugin registration, overlay creation,
  commands, activation policy, gesture pipeline setup.
- `src-tauri/src/switcher.rs` — pure `wrapping_advance` (unit-tested) + `Switcher`
  state/logging glue.
- `src-tauri/src/hotkey.rs` — CGEventTap FFI, extern "C" callback, Accessibility
  check.
- `src-tauri/examples/post_keys.rs` — test harness that posts real CGEvents
  (`CGEventPost`) to drive the gesture for verification. Run with `tauri dev` up:
  `cargo run --example post_keys -- <apps-fwd|apps-back|windows|esc|probe>`.
- `src-tauri/tauri.conf.json` — windows (`main` dev-controls window), `macOSPrivateApi`.
- `src-tauri/capabilities/default.json` — capabilities for `main` + `overlay`.

### How to test the gesture by hand

With Accessibility granted, hold **Ctrl** and tap **Tab** repeatedly: the stderr log
shows `gesture_start` then `advance dir=+1` with the index climbing; **Shift+Tab**
goes `dir=-1`; release **Ctrl** → `commit`. **Ctrl+§** drives `windows` mode. **Esc**
while active → `cancel`. While active, Tab does not change tabs/fields in the app
underneath (it is consumed). Automated equivalent: the `post_keys` example above
(AppleScript/System Events synthetic keys do NOT reach the tap — only real input or
`CGEventPost`).

## Decisions log

- **2026-06-27 (Phase 0):**
  - Rust toolchain installed via rustup (stable).
  - Tailwind v4 via `@tailwindcss/vite` (no `tailwind.config.js` needed); global
    `src/index.css` imports Tailwind and makes `html/body/#root` transparent.
  - There is no default panel type in `tauri-nspanel` v2.1 — the concrete NSPanel
    subclass `OverlayPanel` is defined with the `panel!` macro
    (`config: { can_become_key_window: true }`). The macro needs these traits in
    scope (unqualified): `objc2::runtime::NSObjectProtocol`, `objc2::ClassType`,
    `objc2::Message`.
  - Window-level options (`decorations`, `always_on_top`, `skip_taskbar`,
    `resizable`, `transparent`, `visible(false)`, `center`, `inner_size`) are passed
    through `PanelBuilder::with_window(...)`; panel-level options via the builder.
  - NSPanel operations are main-thread only → `show_overlay` / `hide_overlay` use
    `app.run_on_main_thread(...)`.
  - A visible `main` window hosts temporary dev controls; the `overlay` panel is a
    separate hidden window. Both load the same bundle and branch on label.
  - `objc2` / `objc2-app-kit` added as direct deps (needed now by the macro, and by
    later native phases).

- **2026-06-27 (Phase 1):**
  - CGEventTap implemented with raw FFI (extern "C" callback, state via the
    `userInfo` refcon pointer, returns `NULL` to consume) rather than the
    `core-graphics` safe `CGEventTap` wrapper, to match the spec's exact control
    requirements. `core-foundation` is used for the `AXIsProcessTrustedWithOptions`
    options dictionary; `core-graphics` is used by the `post_keys` test harness.
  - Deps added: `objc2-foundation`, `core-foundation`, `core-graphics`.
  - The Ctrl modifier key release is detected as a `flagsChanged` event (not a
    keyUp) — that is what drives `commit`.
  - Diagnostic logs go to **stderr** (`eprintln!`): unbuffered, so they appear
    immediately when stdout is a pipe (Rust block-buffers piped stdout).
  - **System Events / AppleScript synthetic key events are NOT seen by the session
    event tap.** Verify with real keystrokes or `CGEventPost` (the `post_keys`
    example). This bit us during verification.
  - `wrapping_advance` covered by 6 unit tests (TDD); the tap/gesture is gated by
    manual acceptance criteria per the TDD policy.
