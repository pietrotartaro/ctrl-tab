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
- Hotkeys: native **CGEventTap** (NOT the global-shortcut plugin). [Phase ≥1]
- Native macOS via `objc2` / `objc2-app-kit`. Window enumeration, titles and
  raising via the **Accessibility API**. [Phase ≥2]
- Permissions: **Accessibility only. No Screen Recording.**

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
  commands, activation policy.
- `src-tauri/tauri.conf.json` — windows (`main` dev-controls window), `macOSPrivateApi`.
- `src-tauri/capabilities/default.json` — capabilities for `main` + `overlay`.

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
