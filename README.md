# ctl-tab

A macOS Alt-Tab clone built as a single-binary [Tauri v2](https://tauri.app) app
(Rust + React). It's a background utility:

- **Ctrl-Tab** — switch between applications (MRU order, like Cmd-Tab).
- **Ctrl-§** — switch between the windows of the frontmost application.

While holding **Ctrl**, tap **Tab** / **§** to move the selection right, **Shift+Tab**
to move left. Release **Ctrl** to confirm, **Esc** to cancel. You can also **hover**
and **click** items with the mouse. An Alt-Tab-style overlay shows medium app icons
with the selected item's title on top.

Both shortcuts are **configurable** in the Settings window (record a new combo per
action). The app lives in the **menu bar** (tray): *Impostazioni…*, *Avvia al login*
(autostart), *Esci*. It runs as a background *Accessory* (no Dock icon while the
Settings window is closed); closing the Settings window keeps it running in the
background. Launching it again just re-shows Settings (single instance).

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
- **Shortcuts** are configurable in Settings and persist to
  `~/Library/Application Support/com.pietro.alttabclone/config.json`. The icon size
  is the compile-time `ICON_SIZE` in `src/App.tsx` (keep `ITEM_W` in
  `src-tauri/src/controller.rs` in sync).
- **Backward (Shift) direction** assumes the recorded combo does not itself include
  Shift; recording a Shift-based combo disables the reverse direction.
- Quit via the menu-bar tray → **Esci** (or Ctrl-C in the dev terminal).
- Window order comes from the Accessibility `AXWindows` z-order; minimized windows may
  be excluded.
- If the frontmost app has **0 windows**, the windows overlay isn't shown; with **1
  window** a single item is shown.

## Architecture

See [CLAUDE.md](./CLAUDE.md) for the full architecture, the Rust→frontend event
contract, and per-phase decisions. In short: one Rust process owns the event tap, the
app/window enumeration (objc2 / raw AX FFI), the NSPanel overlay (tauri-nspanel), and
the activation; the React frontend only renders the overlay from emitted events.

## Creare e installare il .dmg (Apple Silicon)

1. **Build:**

   ```bash
   npm run tauri build
   ```

2. **Output:**
   - `.dmg`: `src-tauri/target/release/bundle/dmg/ctl-tab_0.1.0_aarch64.dmg`
   - `.app`: `src-tauri/target/release/bundle/macos/ctl-tab.app`

3. **Installazione:** apri il `.dmg` e trascina `ctl-tab.app` in `/Applications`.

4. **Primo avvio** (app non notarizzata): tasto destro sull'app → **Apri** → **Apri**.
   Se Gatekeeper la blocca comunque, rimuovi la quarantena:

   ```bash
   xattr -dr com.apple.quarantine /Applications/ctl-tab.app
   ```

5. **Permessi:** concedi **Accessibility** all'app **installata** (Impostazioni di
   sistema → Privacy e sicurezza → Accessibilità) — è separata dal binario di
   sviluppo. Se i tasti non arrivano, abilita anche **Monitoraggio input** (Input
   Monitoring) per l'app.

6. **Nota:** l'app gira in **background** (icona nel menu bar); chiudendo la finestra
   Impostazioni resta attiva. Di default le shortcut sono **Ctrl+Tab** (app) e
   **Ctrl+§** (finestre), modificabili dalle Impostazioni.
