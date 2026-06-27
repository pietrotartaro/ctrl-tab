# Phase 0 — Foundations Implementation Plan

> **For agentic workers:** This is Phase 0 of an already-approved epic. Scaffolding/native steps are gated by manual Acceptance Criteria, not unit tests (per Preludio TDD policy). No hotkeys or native switching logic in this phase.

**Goal:** Stand up a single-binary Tauri v2 app (React+TS+Tailwind+Vite) with a transparent, non-activating NSPanel overlay, Accessory activation policy, and test commands to show/hide the overlay.

**Architecture:** Single Rust binary (no sidecar). Frontend is React/TS/Tailwind built by Vite. Overlay is an NSPanel created via `tauri-nspanel` (branch v2.1) using `PanelBuilder`, born hidden+centered, non-activating, floating, on all Spaces, receiving mouse clicks. App runs as a background Accessory (no Dock icon).

**Tech Stack:** Tauri v2, Rust, tauri-nspanel (v2.1), objc2/objc2-app-kit (later phases), React, TypeScript, Tailwind, Vite.

---

### Task 1: Scaffold Tauri v2 + React/TS + Vite in repo root

**Files:**
- Create: `package.json`, `src/`, `src-tauri/`, `index.html`, `vite.config.ts`, `tsconfig.json`
- Verify: project files land in repo root (not a subdirectory)

- [ ] Scaffold with `create-tauri-app` into a temp dir, then move contents into root (scaffolder cannot target a non-empty dir; `docs/` exists).
- [ ] `npm install`
- [ ] Confirm `src-tauri/Cargo.toml`, `package.json`, `src/` present in root.

### Task 2: Add Tailwind to the frontend

**Files:**
- Modify: `package.json` (devDeps), `src/index.css` / main CSS, `index.html`
- Create: `tailwind.config.js`, `postcss.config.js` (or Tailwind v4 Vite plugin)

- [ ] Install Tailwind + integrate with Vite.
- [ ] Add a visible Tailwind-styled element to confirm it works.

### Task 3: Add tauri-nspanel dependency + register plugin

**Files:**
- Modify: `src-tauri/Cargo.toml` — add `tauri-nspanel = { git = "...", branch = "v2.1" }`
- Modify: `src-tauri/src/lib.rs` — `.plugin(tauri_nspanel::init())`

### Task 4: Create overlay NSPanel in setup via PanelBuilder

**Files:**
- Modify: `src-tauri/src/lib.rs`

Panel config: transparent, decorations(false), always_on_top(true), skip_taskbar(true),
resizable(false), no_activate(true), level Floating, collection_behavior with
can_join_all_spaces + ignores_cycle. Born hidden + centered. ignoresMouseEvents = false.

### Task 5: Accessory activation policy

**Files:**
- Modify: `src-tauri/src/lib.rs` — `app.set_activation_policy(ActivationPolicy::Accessory)`
- Or `tauri.conf.json` `app.macOSPrivateApi` / config as needed.

### Task 6: Commands show_overlay / hide_overlay + frontend test buttons

**Files:**
- Modify: `src-tauri/src/lib.rs` — `#[tauri::command] show_overlay`, `hide_overlay`; register in `invoke_handler`
- Modify: `src/App.tsx` — two temporary buttons calling the commands

### Task 7: CLAUDE.md

**Files:**
- Create: `CLAUDE.md` — architecture, stack, switcher state contract (events + mouse commands), permissions (Accessibility only, no Screen Recording), TDD policy, decisions.

### Task 8: Build, run, verify acceptance criteria, commit

- [ ] `npm run tauri dev` compiles & launches with no errors.
- [ ] Panel transparent, borderless, above other windows; test buttons show/hide it.
- [ ] App not in Dock.
- [ ] Commit.
