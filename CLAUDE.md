# ctrl-tab — Project Handoff

A complete, self-contained guide for a new contributor (or a future session with no
prior context) to understand and continue this project. Everything here is derived
from the actual repository state; module, function, command, event, and config names
are exact.

---

## 1. Overview

**ctrl-tab** is a macOS Alt-Tab-style switcher for Apple Silicon, built as a single
Rust binary with [Tauri v2](https://tauri.app). It is a silent background utility:

- **Ctrl-Tab** — switch between applications (most-recently-used order, like Cmd-Tab).
- **Ctrl-§** — switch between the windows of the frontmost application.

An overlay shows medium app icons; while holding **Ctrl** you tap **Tab** / **§** to
move the selection, then release **Ctrl** to switch. It needs **Accessibility
permission only** (no Screen Recording). Both shortcuts are configurable.

## 2. Current status

- **MVP complete and working.** App switcher, window switcher, keyboard + mouse
  navigation, configurable shortcuts, silent background lifecycle, Settings window,
  hover tooltip, VS Code window-title reordering, JetBrains Mono UI.
- **The `.dmg` builds** via `npm run tauri build` (ad-hoc signed, not notarized).
- **Branches:** all work is on **`main`**. The experimental hover-tooltip branch
  (`feature/hover-tooltip`) was **merged into `main` and deleted** — it is no longer
  separate. There are no open feature branches.
- **Renamed:** the app was rebranded to `ctrl-tab` with identifier
  `com.pietro.ctrltab`. The project folder rename is done.
- Licensing is in order: `LICENSE` (MIT), `THIRD-PARTY-LICENSES.md`,
  `licenses/JetBrainsMono-OFL.txt`, and a publishable `README.md`.

## 3. Architecture

**One Rust process owns everything — no sidecar.** Hotkey capture, app/window
enumeration, the overlay panel, and activation all live in the Rust binary. The
frontend is React + TypeScript + Tailwind, built by Vite; the same bundle is loaded
by every window and branches on the window label (`getCurrentWindow().label`).

### Runtime flow

```
 Keyboard (real key / CGEventPost)
     │
     ▼
 CGEventTap  (session-level, head-insert, ACTIVE)         hotkey.rs::tap_callback
     │   keyDown / flagsChanged
     │   (while a switcher is active, Tab/§ keyDowns are CONSUMED → returns NULL)
     ▼
 controller.rs   handle_key_down() / handle_flags_changed()
     │   match_trigger() → config::is_armed()  (exact modifier match)
     │   start() / advance() / commit() / cancel()  → mutates Switcher (switcher.rs)
     ▼
 Tauri events  ───────────────►   Overlay webview  (src/App.tsx → Overlay)
   switcher:show {mode,items,selected}    renders the icon grid
   switcher:select {selected}             moves the highlight
   switcher:hide                          hides
     ▲                                        │ useLayoutEffect → invoke("present_overlay")
     │  invoke (mouse)                         ▼
     │  switcher_hover / switcher_commit   controller::present_overlay → center_and_show
     └──────────────────────────────────  (tauri-nspanel NSPanel, make_key_and_order_front)

 commit  (Ctrl released, or mouse click) ─► controller::perform_commit:
     apps    → apps::activate(pid)          NSRunningApplication.activateWithOptions
     windows → windows::raise(index)        AXRaise + AXMain=true + activate owner app
```

## 4. Repo map

| Path | Purpose |
| --- | --- |
| `src-tauri/src/main.rs` | Binary entry; calls `ctrl_tab_lib::run()`. |
| `src-tauri/src/lib.rs` | Tauri app setup: plugins, `invoke_handler`, overlay creation (`create_overlay`, `OverlayPanel` via `panel!`), Settings show/hide + activation policy, `run()` event loop (Reopen/ExitRequested), the `QUIT` flag, `dlog!`/`debug_enabled`. |
| `src-tauri/src/hotkey.rs` | CGEventTap FFI: `tap_callback`, `ensure_accessibility`, `try_install_tap` / `install_event_tap` (background poll until trusted). |
| `src-tauri/src/controller.rs` | Single source of truth (`Mutex<CtlState>`): config-driven key matching (`handle_key_down`/`handle_flags_changed`, `match_trigger`), gesture (`start`/`advance`/`commit`/`cancel`), panel sizing/centering, mouse commands (`hover`/`commit_index`), config commands, recording. |
| `src-tauri/src/switcher.rs` | Pure logic (unit-tested): `wrapping_advance`, `filter_eligible`, `promote_mru`, `order_by_mru`; plus `Switcher` state, `Mode`, `AppItem`, `RawApp`. |
| `src-tauri/src/apps.rs` | App enumeration (NSWorkspace), 128px PNG icon data URLs (cached per pid), `frontmost`, `bundle_id`, `activate`, MRU observer (`install_workspace_observer`), `build_ordered_apps`. |
| `src-tauri/src/windows.rs` | Frontmost app's windows via raw Accessibility FFI: `enumerate`, `raise`; pure `is_vscode_bundle` + `reorder_vscode_title` (unit-tested). |
| `src-tauri/src/config.rs` | `Combo`/`Config`, `is_armed`, `validate_combo`/`validate_pair`, `format_combo_label`, `key_name`, JSON `load_from`/`save_to`. Pure parts unit-tested. |
| `src-tauri/src/events.rs` | Serde payloads for the contract: `SwitchItem` (camelCase `appName`/`iconDataUrl`), `ShowPayload`, `SelectPayload`. Unit-tested. |
| `src-tauri/examples/post_keys.rs` | Test harness posting real `CGEvent`s to drive the gesture (`apps-fwd`, `windows`, `hold`, `click`, …). |
| `src-tauri/tauri.conf.json` | `productName` `ctrl-tab`, `identifier` `com.pietro.ctrltab`, the hidden `settings` window, `macOSPrivateApi`, bundle (`dmg`, ad-hoc sign). |
| `src-tauri/capabilities/default.json` | Capabilities for the `overlay` + `settings` windows. |
| `src-tauri/Cargo.toml` | Crate `ctrl-tab`, lib `ctrl_tab_lib`, dependencies. |
| `src/App.tsx` | Frontend; branches on `windowLabel`: `Overlay()` (icon grid + hover tooltip) and `Settings()` (record shortcuts, autostart, Quit). Constants `ICON_SIZE=64`, `ITEM_BOX=88`. |
| `src/main.tsx` | React entry; imports JetBrains Mono weights 400/500/600/700 from `@fontsource`. |
| `src/index.css` | Tailwind import; `@theme --font-mono`; `:root --tile-radius` + `--switcher-font-family`; `body` font; `.ctl-panel` / `.ctl-tip` styles. |
| `app-icon.png`, `docs/wordmark.png` | App icon (1024²) and the "ctrl-tab" wordmark image used in the README header. |
| `src-tauri/icons/` | Generated bundle icons (`icon.icns`, PNGs). |

## 5. Native components (real names)

- **CGEventTap** (`hotkey.rs`) — an ACTIVE, session-level, head-insert tap on the
  main run loop, listening for `keyDown` (10) and `flagsChanged` (12). The
  `extern "C"` `tap_callback` reads `CGEventGetFlags` + the keycode field (9) and
  forwards to `controller::handle_key_down` / `handle_flags_changed`. Returns NULL to
  consume Tab/§ while active. Re-enables itself on
  `DisabledByTimeout`/`ByUserInput`. Modifier "arming" uses
  `config::is_armed(current_mods_masked, hold_mods)` — an **exact** match of the
  non-Shift standard modifiers (Shift ignored; Option/Command disqualify).
- **NSWorkspace / AppKit** (`apps.rs`, via `objc2`) — `enumerate_raw` lists running
  apps; `.regular`-only filtering + own-pid drop is pure (`filter_eligible`). MRU is
  kept by an `addObserverForName:NSWorkspaceDidActivateApplicationNotification` block
  (`install_workspace_observer` → `promote_mru`). Icons rendered via
  `NSImage::imageWithSize_flipped_drawingHandler` → TIFF → PNG → base64, cached per
  pid. `activate` uses `activateWithOptions(ActivateAllWindows)`.
- **Accessibility API** (`windows.rs`, raw AX FFI) — `enumerate(pid)` reads
  `AXWindows`, keeps `AXRole == AXWindow` with a non-empty `AXTitle`, retains the AX
  refs in list order, returns titles. `raise(index)` performs `AXRaise`, sets
  `AXMain = true`, then activates the owner app.
- **Overlay** (`tauri-nspanel`) — `OverlayPanel` defined via the `panel!` macro
  (`can_become_key_window: true`). Built in `create_overlay`: transparent,
  `decorations(false)`, `always_on_top(true)`, `skip_taskbar(true)`,
  `resizable(false)`, `no_activate(true)`, level Floating, collection behavior
  `can_join_all_spaces` + `ignores_cycle`, `ignoresMouseEvents = false`,
  `acceptsMouseMovedEvents = true`. Shown with `make_key_and_order_front` (key
  without activating the background app). Native dark vibrancy via
  `window-vibrancy` (`HudWindow`, radius `PANEL_RADIUS = 14.0`).

## 6. Rust ↔ frontend contract (real names)

### Tauri commands (frontend → Rust; registered in `lib.rs` `invoke_handler`)

| Command | Args | Effect |
| --- | --- | --- |
| `switcher_hover` | `index: usize` | Set selected index, re-emit `switcher:select`. |
| `switcher_commit` | `index: usize` | Set index, commit (activate), hide overlay (main thread). |
| `present_overlay` | — | Frontend signals it rendered; center + show the (pre-sized) panel. |
| `get_config` | — | Returns the current `Config`. |
| `start_recording` | `action: "app" \| "windows"` | Arm capture of the next modifier+key combo. |
| `save_config` | `appMods, appKey, winMods, winKey` | Validate + apply + persist; returns `Config`. |
| `reset_config` | — | Reset to defaults, persist, return `Config`. |
| `quit_app` | — | Set `QUIT` flag and `app.exit(0)`. |
| `get_autostart` | — | `bool` login-item state. |
| `set_autostart` | `enabled: bool` | Enable/disable the login item. |

### Events (Rust → frontend)

- `switcher:show` — `{ mode: "apps"|"windows", items: SwitchItem[], selected: number }`
  where `SwitchItem = { id, title, appName, iconDataUrl }`.
- `switcher:select` — `{ selected: number }`.
- `switcher:hide` — no payload.
- `recording:done` — `{ action, modifiers, keyCode, label }` (emitted from
  `handle_key_down` when recording is armed).

## 7. Behavioral spec (functional truth)

- **Shortcuts.** `Ctrl+Tab` → apps; `Ctrl+§` → windows. The trigger key always moves
  the selection **right**; **Shift+Tab/§** moves **left** but **only while a switcher
  is already active** (a Shift rising edge while idle does nothing). First press
  starts the switcher (selected = 1, i.e. the previous item) and the gesture stays
  open while Ctrl is held; releasing the hold modifier **commits**, `Esc` cancels.
  The switcher arms only on an **exact** modifier match (`is_armed`) — a hyperkey or
  Ctrl+Option/Command does NOT open it.
- **Mouse.** Hover selection engages **only on real pointer movement** (a stationary
  cursor at open keeps the keyboard selection). Click (`pointerdown`, any button)
  commits; the context menu is suppressed (`contextmenu` preventDefault) so a
  Ctrl+click does not open a menu and does not double-commit.
- **Overlay.** Dark "liquid glass" (native vibrancy + CSS tint), corner radius shared
  with the icon tiles (`--tile-radius`, `PANEL_RADIUS`). No centered title — each
  item shows its (truncated) label under the icon; the full name appears in a custom
  hover tooltip (`.ctl-tip`, fixed-position, below the cursor, clamped to the window).
  The window is Rust-sized to the content and centered on the active screen; the grid
  wraps to rows and never scrolls. Icon size `ICON_SIZE = 64` (`ITEM_BOX = 88` must
  match `controller.rs`). All switcher text uses `var(--switcher-font-family)`
  (JetBrains Mono).
- **Settings.** Silent start: no Dock icon, no menu-bar item. The window is created
  hidden; relaunching the app (Spotlight/Finder) shows it via `RunEvent::Reopen` /
  the single-instance callback (→ `show_settings`, switches to Regular for focus).
  Closing it hides + returns to Accessory; **Quit** is the only full exit (red
  button → `quit_app`). Recording a shortcut **applies + persists instantly** (no
  Save button); on validation failure the previous combo is kept. **Launch at login**
  is a switch (instant). UI is entirely in English.
- **VS Code window titles.** In windows mode, for VS Code bundles
  (`com.microsoft.VSCode` / `…VSCodeInsiders`) the title `file — project` is reordered
  to `project — file` (`reorder_vscode_title`, swaps the first two `" — "` segments
  when ≥2 exist; otherwise unchanged). Other apps are untouched.

## 8. Build & run

```bash
npm install                          # frontend deps

npm run tauri dev                    # build + run (debug)
CTRL_TAB_DEBUG=1 npm run tauri dev   # with verbose stderr diagnostics

npm run tauri build                  # release .app + .dmg
```

Bundle output:

- `.dmg` — `src-tauri/target/release/bundle/dmg/ctrl-tab_0.1.0_aarch64.dmg`
- `.app` — `src-tauri/target/release/bundle/macos/ctrl-tab.app`

Packaging: `bundle.targets = ["dmg"]`, `macOS.signingIdentity = "-"` (ad-hoc, not
notarized), `minimumSystemVersion = "13.0"`, `macOSPrivateApi = true`. First launch
of the installed app: right-click → Open, or
`xattr -dr com.apple.quarantine /Applications/ctrl-tab.app`.

Unit tests: `cd src-tauri && cargo test --lib` (39 tests).

## 9. Permissions & TCC

- **Accessibility only** — required for both the event tap (macOS only delivers tap
  events when the responsible process is trusted) and window enumeration/raising. The
  app calls `AXIsProcessTrustedWithOptions` at startup and polls until granted (no
  restart needed). **No Screen Recording** is used.
- In `tauri dev`, the responsible process is the **terminal** — grant it there. For a
  bundled build, grant **ctrl-tab**.
- **TCC caveat:** trust is keyed to the code signature (cdhash). Ad-hoc signing means
  every rebuild changes the cdhash and **invalidates the prior grant** — re-grant
  after reinstalling. Changing the bundle **identifier** likewise resets the TCC
  identity, the **autostart** login item (re-toggle Launch at login), and the config
  dir (`~/Library/Application Support/com.pietro.ctrltab/config.json` → shortcuts
  reset to defaults).

## 10. Architectural decisions & WHY (do not undo)

- **Native CGEventTap, not the global-shortcut plugin.** Only an active session tap
  can consume Tab/§ so they don't reach the app underneath and can drive the
  hold/release gesture. The plugin can't express this.
- **Single Rust binary, no sidecar.** All native work (tap, enumeration, panel,
  activation) is in-process; simpler lifecycle and no IPC with a helper.
- **Exact modifier match (`is_armed`).** Subset matching let a hyperkey
  (Ctrl+Shift+Opt+Cmd) or Ctrl+Option open the switcher accidentally. Shift is
  ignored on purpose (it drives left-navigation); Option/Command disqualify.
- **Rust-computed panel size + `present_overlay` round-trip.** Measuring a hidden
  webview was racy; Rust computes the size deterministically (uniform items), pre-
  sizes the hidden panel, and only shows it after the frontend signals it rendered —
  no flicker, no per-select resize.
- **Non-activating NSPanel shown with `make_key_and_order_front`.** It must become key
  to receive mouse-moved/hover/click, but a NonactivatingPanel does so without
  activating the background app.
- **Drop to Accessory before activating the target** (`perform_commit`). Otherwise,
  with Settings open (Regular), `activateWithOptions` returns false and the target
  won't come forward.
- **Native vibrancy for the glass**, CSS tint/sheen on top — real macOS blur instead
  of a flat color.
- **Single CSS variable `--switcher-font-family`** so the switcher font is revertible
  in one line without touching the Settings font.
- **Plain JSON config (no store plugin)** at `app_config_dir()/config.json`, loaded in
  `controller::init`. Kept minimal and dependency-free.
- **Silent background + toggle Regular only while Settings is open.** Staying
  Accessory + `NSApp.activate` left a lingering Dock icon; toggling to Regular only
  while Settings is visible gives reliable focus and removes the Dock icon when it
  closes. There is intentionally **no tray**.
- **Custom hover tooltip, not the native `title`.** The native tooltip does not render
  on a non-activating background panel (verified); a fixed-position `.ctl-tip` shows
  the full name with no layout reflow.

## 11. Testing / TDD policy

Real red/green/refactor TDD applies **only to pure, unit-testable logic**:

- `switcher::wrapping_advance` (index advance with wrap-around)
- `switcher::filter_eligible`, `promote_mru`, `order_by_mru` (eligibility + MRU)
- `config::is_armed`, `validate_combo`/`validate_pair`, `format_combo_label`, Config
  JSON round-trip
- `windows::reorder_vscode_title`, `is_vscode_bundle`
- `events` payload serialization (camelCase contract)

Native / UI code is **manual-acceptance-gated**, not unit-tested: CGEventTap/gesture,
NSPanel overlay, Accessibility enumeration/raising, icon rendering, app activation,
Settings lifecycle. Drive the gesture from code with
`cargo run --example post_keys -- <scenario>` while the app runs. Never delete native
code merely because "a test is missing".

## 12. Key dependencies

**Rust** (`src-tauri/Cargo.toml`): `tauri` 2 (features `macos-private-api`,
`tray-icon`, `image-png`), `tauri-plugin-single-instance` 2, `tauri-plugin-autostart`
2, `tauri-plugin-opener` 2, `tauri-nspanel` (git, branch **v2.1**), `objc2` 0.6,
`objc2-app-kit` 0.3, `objc2-foundation` 0.3, `block2` 0.6, `core-foundation` 0.10,
`core-graphics` 0.24, `window-vibrancy` 0.6, `base64` 0.22, `serde` 1, `serde_json` 1.

**Frontend** (`package.json`): `react`/`react-dom` 19, `@tauri-apps/api` 2,
`@tauri-apps/plugin-opener` 2, `@fontsource/jetbrains-mono` 5, Tailwind CSS 4 (via
`@tailwindcss/vite`), Vite 7, TypeScript 5.8.

> Notes: there is **no store plugin** (config is plain JSON). The Cargo `tray-icon` /
> `image-png` features are enabled but the current app creates **no tray** (leftover
> from an earlier phase; harmless).

## 13. License

ctrl-tab is **MIT** (`LICENSE`, © 2026 Pietro Tartaro). Third-party components and
their verified SPDX licenses are listed in `THIRD-PARTY-LICENSES.md` (Tauri & plugins
Apache-2.0/MIT, tauri-nspanel Apache-2.0/MIT, objc2 family Zlib/Apache-2.0/MIT,
core-graphics/core-foundation/base64/serde MIT/Apache-2.0, window-vibrancy
Apache-2.0/MIT, React/Tailwind/Vite/@fontsource MIT). **JetBrains Mono** ships under
the **SIL OFL 1.1** (`licenses/JetBrainsMono-OFL.txt`; "JetBrains Mono" is a Reserved
Font Name). Inspired by [AltTab](https://github.com/lwouis/alt-tab-macos) — an
independent project, not affiliated with AltTab or Apple, sharing no code.

## 14. Known pitfalls

- **Event tap can be disabled** by the system (`DisabledByTimeout`/`ByUserInput`);
  `tap_callback` re-enables it. Don't remove that path.
- **Mouse on the panel needs key status.** Use `make_key_and_order_front` (not
  `orderFrontRegardless`) and `acceptsMouseMovedEvents(true)`, or hover/click die.
- **Hyperkey / extra modifiers** must NOT open the switcher — keep the exact
  `is_armed` match.
- **§ key code is 10** (ISO `KEY_SECTION`); Tab is 48, Esc 53, left Ctrl 59.
- **Synthetic AppleScript/System Events keys are NOT seen by the tap** — verify with
  real input or `CGEventPost` (the `post_keys` example).
- **Folder/identifier changes** invalidate TCC and autostart (see §9).
- **`ITEM_BOX` must stay in sync** between `src/App.tsx` and `controller.rs` or the
  Rust-computed panel size won't match the rendered grid.
- **Diagnostics are gated** behind `CTRL_TAB_DEBUG=1` (the `dlog!` macro); the app is
  silent otherwise.

## 15. How to resume

1. Read this file, then skim `controller.rs` (orchestration), `hotkey.rs` (tap), and
   `src/App.tsx` (UI) — they cover ~all behavior.
2. Sanity check in ~2 minutes:
   ```bash
   npm install
   CTRL_TAB_DEBUG=1 npm run tauri dev
   ```
   Grant **Accessibility** to the terminal (System Settings → Privacy & Security →
   Accessibility). Then hold **Ctrl** and tap **Tab** (apps) / **§** (windows): the
   overlay appears, the selection moves, releasing Ctrl switches. `Esc` cancels.
   Relaunch the app to open **Settings**.
3. To drive the gesture deterministically:
   `cargo run --example post_keys -- apps-fwd` (with the app running).
4. Run the unit tests: `cd src-tauri && cargo test --lib`.
