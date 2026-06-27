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
- App list/icons/activation via `objc2-app-kit` (NSWorkspace / NSRunningApplication
  / NSImage). [Phase 2 ✓] Window enumeration, titles and raising via the
  **Accessibility API** (raw AX FFI). [Phase 4 ✓]
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

> The contract above is fully implemented. The Phase-0 dev window and
> `show_overlay` / `hide_overlay` commands were removed in Phase 5.

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
  check. On apps gesture_start it builds the real list via `apps::build_ordered_apps`
  and on commit calls `apps::activate`.
- `src-tauri/src/apps.rs` — real app enumeration (NSWorkspace), 128px PNG icon data
  URLs (cached per pid), `activate(pid)`, and the NSWorkspace activation observer
  that keeps the MRU current.
- `src-tauri/src/controller.rs` — single source of truth (`Mutex<CtlState>`:
  switcher + config + recording + active gesture modifiers). Owns the config-driven
  key matching (`handle_key_down` / `handle_flags_changed`), recording capture, the
  contract events, panel show/size/position, activation, and the config commands.
- `src-tauri/src/config.rs` — shortcut config: `Combo`/`Config`, `validate_*`,
  `format_combo_label`, key-code names, JSON load/save. Pure parts unit-tested.
- `src-tauri/src/events.rs` — serde payload types for the switcher contract
  (camelCase field names, unit-tested).
- `src-tauri/src/windows.rs` — frontmost app's window enumeration + raising via
  raw Accessibility FFI (AXUIElementCreateApplication / CopyAttributeValue /
  PerformAction). Stores the AX window refs in the same order as the switcher list.
- `src/App.tsx` — branches on window label: the overlay React UI (Alt-Tab style,
  icon size = `ICON_SIZE`, keep `ITEM_W` in `controller.rs` in sync) and the
  Settings window UI (record/save/reset shortcuts).
- `src-tauri/src/lib.rs` also owns the tray (TrayIconBuilder: Impostazioni / Avvia
  al login / Esci), the single-instance + autostart plugins, the Settings window
  lifecycle (CloseRequested → hide, ExitRequested → prevent unless quitting), and
  the Accessory↔Regular activation toggle.
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

- **2026-06-27 (Phase 2):**
  - Real app data: `filter_eligible` (.regular, excl. own pid), `promote_mru`,
    `order_by_mru` are pure + unit-tested (15 lib tests total). Native
    enumeration/icons/activation/observer in `apps.rs` are acceptance-gated.
  - MRU is kept by an `addObserverForName:NSWorkspaceDidActivateApplicationNotification`
    block observer; on each activation it promotes `frontmostApplication`'s pid.
    `build_ordered_apps` also promotes the current frontmost so index 0 = current,
    index 1 = previous. Initial `selected = min(1, len-1)` → a single Ctrl+Tap+release
    jumps to the previous app.
  - Icons: `NSImage::imageWithSize_flipped_drawingHandler` (NOT the deprecated
    `lockFocus`) → 128px, TIFF → `NSBitmapImageRep` → PNG → base64 data URL, cached
    per pid in a process-global `Mutex<AppState>`.
  - Activation: `activateWithOptions(ActivateAllWindows)` only —
    `ActivateIgnoringOtherApps` is a no-op on macOS 14+.
  - objc2 notes: `NSImage::alloc` needs `objc2::AnyThread` in scope; the block
    observer needs `block2` as a direct dep; most NSWorkspace/NSRunningApplication
    accessors are safe (no `unsafe`), only `representationUsingType:properties:` and
    `addObserverForName:…` need `unsafe`.
  - Deps added: `block2`, `base64`.
  - Windows mode (Ctrl+§) still uses a stub list; real windows arrive in a later
    phase. `post_keys` gained an `apps-one` scenario (single Ctrl+Tab+release).

- **2026-06-27 (Phase 3):**
  - Overlay UI driven by the contract events. `controller.rs` owns the switcher
    state (moved out of the tap's `TapState`) so keyboard (tap) and mouse (Tauri
    commands `switcher_hover` / `switcher_commit`) drive the same state.
  - **Mouse fix:** the overlay must be shown with `makeKeyAndOrderFront` (key
    window), not `orderFrontRegardless` — otherwise the WKWebView gets no
    mouse-moved/hover and clicks don't land. A NonactivatingPanel becomes key
    WITHOUT activating our background app, so this is safe. Also set
    `acceptsMouseMovedEvents(true)`.
  - `switcher_commit` resets the state after activating, so the subsequent Ctrl
    release is a no-op (no double-commit). `switcher_hover` only mutates state +
    emits `switcher:select` (no native op → safe off-main-thread); `switcher_commit`
    runs on the main thread (panel + activation).
  - Panel is sized in Rust to the item count (`controller` constants must match the
    React item width) and centered on the screen containing the mouse.
  - Payload serialization is unit-tested (`appName` / `iconDataUrl` camelCase).
  - Verification note: AppleScript/System Events `click at` on the panel fails
    (-25208); use `CGEventPost` mouse events (the `post_keys` `click` / `mouseonly`
    / `moveonly` / `hold` scenarios) to drive the mouse for testing. `screencapture`
    requires Screen Recording for the host terminal (used only for dev screenshots,
    not by the app).

- **2026-06-27 (Phase 4):**
  - Windows mode (Ctrl+§) via raw Accessibility FFI in `windows.rs`. `enumerate(pid)`
    reads `AXWindows`, keeps role==`AXWindow` with a non-empty `AXTitle`, retains the
    AX refs (in list order), returns titles. `raise(index)` does `AXRaise` +
    set `AXMain` true, then activates the owner app.
  - The AX bindings are raw FFI (no objc2 AX crate). It was NOT fragile — no Swift
    sidecar needed. Attribute/role/action names are created as CFStrings from their
    stable string values ("AXWindows", "AXTitle", "AXRole", "AXMain", "AXRaise").
  - `controller` builds window items with `pid` = owner app pid (so all share the
    app icon) and `name` = window title; the list index lines up with
    `windows::raise`. `build_show_payload` makes item `id` unique (`pid-index`) since
    windows share a pid (avoids React key collisions).
  - 0 windows → overlay is NOT shown (gesture stays inactive). 1 window → a single
    item (selected 0). The 0-window path is code-handled but was not runtime-tested.
  - § key code is `KEY_SECTION = 10` (ISO) in `hotkey.rs` — remap there if needed.
  - Apps mode and windows mode are independent (verified: Ctrl+Tab still app-switches
    after using Ctrl+§).

- **2026-06-27 (Phase 5 — polish, MVP complete):**
  - Removed the Phase-0 dev window (`tauri.conf.json` `windows: []`) and the
    `show_overlay`/`hide_overlay` commands + DevControls UI. The app is now purely
    background: 0 visible windows when idle, not in the Dock.
  - Diagnostic logging is gated behind `CTL_TAB_DEBUG=1` via the `dlog!` macro +
    `debug_enabled()` (lib.rs). Actionable messages (Accessibility NOT granted, tap
    creation failure) stay unconditional. Verified silent without the flag.
  - Icon size is the `ICON_SIZE` constant in `src/App.tsx` (`ITEM_BOX` must equal
    `controller.rs` `ITEM_W`). Many apps: the item row is `overflow-x-auto` and the
    panel width is clamped to the screen width.
  - Added a smoke test composing `filter_eligible` → `order_by_mru` (the native
    app-list pipeline). 18 lib tests total.
  - Runtime-verified end to end: Tab/Shift nav + mouse hover/click in both modes,
    no double-commit (click then Ctrl-release = one commit), Esc cancels without
    activating, mouse leaving the panel keeps the last hovered selection (release
    commits it), Ctrl+Tab vs Ctrl+§ independent, background-only.
  - Code-handled but NOT force-tested at runtime (documented honestly): event-tap
    auto-recovery on `DisabledByTimeout/ByUserInput`; app-dies-mid-gesture
    (`activate` logs "app not found", `raise` no-ops on stale refs — no panic);
    no-eligible-apps (overlay not shown); missing icon (frontend placeholder);
    multi-monitor centering (single display available here — the code centers on the
    NSScreen containing the mouse).

- **2026-06-27 (Phase 6 — Settings window + configurable shortcuts):**
  - Pure (TDD): `validate_combo`/`validate_pair` (≥1 modifier + non-modifier key;
    reject identical pair), `format_combo_label` (⌃⌥⇧⌘ order), Config JSON
    round-trip. 27 lib tests total.
  - Shortcuts are config-driven: the tap forwards every keyDown/flagsChanged to
    `controller`, which matches against `Config` (modifiers+keyCode). Forward =
    exact modifier match; backward = combo modifiers + Shift. Commit fires when the
    combo's modifiers are no longer all held. Hot-applied (the tap reads shared
    state), so Save takes effect without restart.
  - Recording uses the existing tap (NOT the webview): `start_recording(action)`
    sets a recording flag; the next keyDown with ≥1 modifier (and a non-modifier
    key) is captured and emitted as `recording:done`. This captures the real macOS
    keyCode (reliable for § etc.).
  - Persistence: JSON at `app_config_dir()/config.json` (no store plugin). Loaded at
    startup via `controller::init`; defaults if absent.
  - Background lifecycle: tray (TrayIconBuilder) with Impostazioni / Avvia al login
    (autostart plugin) / Esci. Settings window CloseRequested → `prevent_close` +
    hide; ExitRequested → `prevent_exit` unless the `QUIT` flag is set (tray → Esci).
    single-instance plugin re-shows Settings on a second launch. Activation policy:
    Regular while Settings is visible (for focus/keys), Accessory when hidden.
  - Deps: `tauri-plugin-single-instance`, `tauri-plugin-autostart`, tauri features
    `tray-icon` + `image-png`.
  - Runtime-verified: Settings opens on launch; closing it keeps the app in
    background (background-only) with shortcuts working; record ⌃Q → Save → works
    immediately AND persists across restart; old ⌃Tab unbinds; identical-combo Save
    is rejected (config unchanged); single-instance re-shows Settings (2nd process
    exits).
  - Caveat: `activateWithOptions` returns false while Settings is open (our app is
    Regular/foreground) — irrelevant in normal background use (verified true once
    Settings is closed). Tray menu item clicks (Esci/autostart) are wired but were
    not force-tested via synthetic menu-bar clicks; the ExitRequested/QUIT path and
    autostart toggle are simple and code-verified.
