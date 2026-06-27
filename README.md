# ctl-tab

A macOS Alt-Tab clone built as a single-binary [Tauri v2](https://tauri.app) app
(Rust + React). It's a background utility:

- **Ctrl-Tab** — switch between applications (MRU order, like Cmd-Tab).
- **Ctrl-§** — switch between the windows of the frontmost application.

While holding **Ctrl**, tap **Tab** / **§** to move the selection right, **Shift+Tab**
to move left. Release **Ctrl** to confirm, **Esc** to cancel. You can also **hover**
and **click** items with the mouse. An Alt-Tab-style overlay shows medium app icons
with the selected item's title on top.

The app has no Dock icon and no menu bar (it runs as a background *Accessory*).

## Prerequisites

- **macOS** (Apple Silicon or Intel). Developed/tested on macOS 14+.
- **Xcode Command Line Tools**: `xcode-select --install`
- **Rust** (stable): `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
  then `source "$HOME/.cargo/env"`
- **Node.js** 18+ and **npm**.

## Build & run

```bash
npm install          # install frontend deps
npm run tauri dev    # build + run (debug)
```

For a release build/bundle:

```bash
npm run tauri build
```

Enable verbose diagnostic logging (otherwise the app is quiet):

```bash
CTL_TAB_DEBUG=1 npm run tauri dev
```

## Granting Accessibility

ctl-tab uses a CoreGraphics **event tap** to capture the Ctrl-Tab / Ctrl-§ gesture and
the **Accessibility API** to enumerate/raise windows. macOS only delivers events to the
tap if the **responsible process** is trusted for Accessibility:

1. Open **System Settings → Privacy & Security → Accessibility**.
2. Enable the relevant app:
   - In `npm run tauri dev`, the responsible process is your **terminal** (the one
     that launched the app) — enable that.
   - For a bundled build, enable **ctl-tab**.
3. Restart the app.

On launch the app calls `AXIsProcessTrustedWithOptions` with the system prompt; if it
isn't trusted it prints instructions and the gesture simply won't fire.

It requires **Accessibility only** — **no Screen Recording**.

### TCC note for development

Trust is keyed to the binary path / code signature, so a fresh `target/debug/ctl-tab`
inherits trust from the launching terminal (which is why you grant the terminal in dev).
If events stop arriving after a rebuild, re-check the Accessibility list.

> `screencapture` (used only for dev screenshots, not by the app) needs **Screen
> Recording** for the terminal — unrelated to ctl-tab's own permissions.

## Known MVP limitations

- **Windows mode is the frontmost app's windows only** (not all windows of all apps).
- **No previews/thumbnails** — windows show the app icon + the window title.
- **No persistence / preferences UI** — hotkeys and icon size are compile-time
  constants (`KEY_SECTION` in `src-tauri/src/hotkey.rs`, `ICON_SIZE` in `src/App.tsx`
  which must stay in sync with `ITEM_W` in `src-tauri/src/controller.rs`).
- **No quit UI** — as a background Accessory app there's no menu; quit from the dev
  terminal (Ctrl-C) or via Activity Monitor for a bundled build.
- Window order comes from the Accessibility `AXWindows` z-order; minimized windows may
  be excluded.
- If the frontmost app has **0 windows**, the windows overlay isn't shown; with **1
  window** a single item is shown.

## Architecture

See [CLAUDE.md](./CLAUDE.md) for the full architecture, the Rust→frontend event
contract, and per-phase decisions. In short: one Rust process owns the event tap, the
app/window enumeration (objc2 / raw AX FFI), the NSPanel overlay (tauri-nspanel), and
the activation; the React frontend only renders the overlay from emitted events.
