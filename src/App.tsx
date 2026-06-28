import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

// Branch on the window label: "overlay" → the Alt-Tab overlay, "settings" → the
// settings window.
const windowLabel = getCurrentWindow().label;

// Medium icon size (px) and per-item box width (compact). Keep in sync with the
// matching constants in src-tauri/src/controller.rs.
const ICON_SIZE = 64;
const ITEM_BOX = 88;

type SwitchItem = {
  id: string;
  title: string;
  appName: string;
  iconDataUrl: string;
};

type ShowPayload = {
  mode: "apps" | "windows";
  items: SwitchItem[];
  selected: number;
};

/** Alt-Tab-style overlay, driven by Rust events. */
function Overlay() {
  const [items, setItems] = useState<SwitchItem[]>([]);
  const [selected, setSelected] = useState(0);
  // Hover-selection is enabled only after a REAL mouse movement (not when the panel
  // appears under a stationary cursor).
  const mouseActive = useRef(false);
  const baseline = useRef<{ x: number; y: number } | null>(null);
  const lastHover = useRef(-1);

  useEffect(() => {
    const unlisten = [
      listen<ShowPayload>("switcher:show", (e) => {
        setItems(e.payload.items);
        setSelected(e.payload.selected);
        // Reset hover gating for the new gesture.
        mouseActive.current = false;
        baseline.current = null;
        lastHover.current = e.payload.selected;
      }),
      listen<{ selected: number }>("switcher:select", (e) => {
        setSelected(e.payload.selected);
      }),
      listen("switcher:hide", () => {
        mouseActive.current = false;
        baseline.current = null;
      }),
    ];
    return () => {
      unlisten.forEach((p) => p.then((f) => f()));
    };
  }, []);

  // Once the new items are rendered, tell Rust to center + show the (already
  // Rust-sized) overlay. NOT keyed on `selected`, so navigating never re-shows or
  // moves the panel — the window already contains all apps.
  useLayoutEffect(() => {
    if (items.length === 0) return;
    invoke("present_overlay");
  }, [items]);

  // Hover selection driven by real movement only; plus suppress the context menu
  // (Ctrl is held while the switcher is open → Ctrl+click would be a secondary click).
  useEffect(() => {
    function onMove(e: PointerEvent) {
      if (!mouseActive.current) {
        const moved = e.movementX !== 0 || e.movementY !== 0;
        if (baseline.current == null) {
          // First event after open: treat as the (possibly spurious) appearance
          // position. Activate only if it carries real movement.
          baseline.current = { x: e.screenX, y: e.screenY };
          if (!moved) return;
        } else if (
          e.screenX === baseline.current.x &&
          e.screenY === baseline.current.y &&
          !moved
        ) {
          return;
        }
        mouseActive.current = true;
      }
      const el = (document.elementFromPoint(e.clientX, e.clientY) as HTMLElement | null)?.closest(
        "[data-idx]",
      ) as HTMLElement | null;
      if (!el) return;
      const i = Number(el.dataset.idx);
      if (i === lastHover.current) return; // only on selection change
      lastHover.current = i;
      setSelected(i);
      invoke("switcher_hover", { index: i });
    }
    const onCtx = (e: Event) => e.preventDefault();
    window.addEventListener("pointermove", onMove);
    window.addEventListener("contextmenu", onCtx);
    return () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("contextmenu", onCtx);
    };
  }, []);

  return (
    // The window is Rust-sized to the content; the dark-glass panel fills it and
    // wraps the icon grid onto multiple rows at the window width — never scrolls.
    // No centered title; each item keeps its own label below the icon.
    <div className="ctl-panel flex h-screen w-screen flex-wrap content-center justify-center gap-1 p-3 select-none">
      {items.map((item, i) => {
        const isSel = i === selected;
        return (
          <button
            key={item.id}
            data-idx={i}
            // Commit on pointer DOWN for ANY button so a Ctrl+click (secondary
            // click, since Ctrl is held) still opens the app; preventDefault stops
            // the context menu. Commit resets state, so the Ctrl release won't
            // double-commit.
            onPointerDown={(e) => {
              e.preventDefault();
              invoke("switcher_commit", { index: i });
            }}
            onContextMenu={(e) => e.preventDefault()}
            style={{
              width: ITEM_BOX,
              borderRadius: "var(--tile-radius)",
              background: isSel ? "rgba(255,255,255,0.16)" : "transparent",
            }}
            className="flex shrink-0 flex-col items-center gap-1 px-2 py-2"
          >
            <div
              className="flex items-center justify-center"
              style={{ width: ICON_SIZE, height: ICON_SIZE }}
            >
              {item.iconDataUrl ? (
                <img
                  src={item.iconDataUrl}
                  alt={item.appName}
                  style={{ width: ICON_SIZE, height: ICON_SIZE }}
                  className="object-contain"
                  draggable={false}
                />
              ) : (
                <div
                  className="bg-white/10"
                  style={{
                    width: ICON_SIZE - 8,
                    height: ICON_SIZE - 8,
                    borderRadius: "var(--tile-radius)",
                  }}
                />
              )}
            </div>
            <span className="w-full truncate text-center text-[11px] leading-tight text-white/85">
              {item.appName}
            </span>
          </button>
        );
      })}
    </div>
  );
}

type Combo = { modifiers: number; key_code: number; label: string };
type Config = { switch_app: Combo; switch_windows: Combo };
type ActionKey = "app" | "windows";

/** Settings window: shortcuts (instant apply), autostart switch, quit. */
function Settings() {
  const [appLabel, setAppLabel] = useState("…");
  const [winLabel, setWinLabel] = useState("…");
  const [recording, setRecording] = useState<ActionKey | null>(null);
  const [error, setError] = useState("");
  const [autostart, setAutostart] = useState(false);
  // Current applied combos, kept in a ref so the recording:done listener (set up
  // once) always sees the latest values.
  const combos = useRef<{ app: Combo; win: Combo } | null>(null);

  function applyConfig(cfg: Config) {
    setAppLabel(cfg.switch_app.label);
    setWinLabel(cfg.switch_windows.label);
    combos.current = { app: cfg.switch_app, win: cfg.switch_windows };
  }

  useEffect(() => {
    invoke<Config>("get_config").then(applyConfig).catch(() => {});
    invoke<boolean>("get_autostart").then(setAutostart).catch(() => {});
    const un = listen<{ action: ActionKey; modifiers: number; keyCode: number; label: string }>(
      "recording:done",
      async (e) => {
        const { action, modifiers, keyCode } = e.payload;
        setRecording(null);
        const cur = combos.current;
        if (!cur) return;
        // Build the new pair and apply+persist instantly (the backend validates).
        const next = { modifiers, key_code: keyCode, label: "" };
        const app = action === "app" ? next : cur.app;
        const win = action === "windows" ? next : cur.win;
        setError("");
        try {
          const cfg = await invoke<Config>("save_config", {
            appMods: app.modifiers,
            appKey: app.key_code,
            winMods: win.modifiers,
            winKey: win.key_code,
          });
          applyConfig(cfg); // success → labels reflect the new combo
        } catch (err) {
          // Validation failed → keep the previous combo, show the error.
          setError(String(err));
        }
      },
    );
    return () => {
      un.then((f) => f());
    };
  }, []);

  function record(action: ActionKey) {
    setError("");
    setRecording(action);
    invoke("start_recording", { action });
  }

  async function resetDefaults() {
    setError("");
    const cfg = await invoke<Config>("reset_config");
    applyConfig(cfg);
  }

  async function toggleAutostart(next: boolean) {
    setAutostart(next); // optimistic
    try {
      await invoke("set_autostart", { enabled: next }); // applied instantly
    } catch (e) {
      setError(String(e));
      invoke<boolean>("get_autostart").then(setAutostart).catch(() => {}); // revert
    }
  }

  const Row = ({ title, label, action }: { title: string; label: string; action: ActionKey }) => (
    <div className="flex items-center justify-between gap-3 rounded-lg border border-neutral-200 bg-white px-3 py-2.5">
      <div className="flex flex-col">
        <span className="text-sm font-medium text-neutral-800">{title}</span>
        <span className="font-mono text-lg text-neutral-900">
          {recording === action ? "Press a shortcut…" : label}
        </span>
      </div>
      <button
        onClick={() => record(action)}
        className="rounded-md bg-neutral-900 px-3 py-1.5 text-sm font-medium text-white hover:bg-neutral-700 active:bg-black disabled:opacity-50"
        disabled={recording !== null}
      >
        {recording === action ? "Listening…" : "Record"}
      </button>
    </div>
  );

  return (
    <main className="flex h-screen flex-col gap-3 bg-neutral-50 p-5 text-neutral-900">
      <h1 className="text-base font-semibold">Settings</h1>
      <Row title="Switch apps" label={appLabel} action="app" />
      <Row title="Switch windows" label={winLabel} action="windows" />

      <div className="flex items-center justify-between gap-3 rounded-lg border border-neutral-200 bg-white px-3 py-2.5">
        <span className="text-sm font-medium text-neutral-800">Launch at login</span>
        <button
          role="switch"
          aria-checked={autostart}
          onClick={() => toggleAutostart(!autostart)}
          className={`flex h-6 w-11 items-center rounded-full p-0.5 transition-colors ${
            autostart ? "justify-end bg-blue-600" : "justify-start bg-neutral-300"
          }`}
        >
          <span className="h-5 w-5 rounded-full bg-white shadow" />
        </button>
      </div>

      {error && <p className="text-sm text-red-600">{error}</p>}

      <div className="mt-auto flex items-center justify-between gap-2">
        <button
          onClick={() => invoke("quit_app")}
          className="rounded-md border border-red-300 px-3 py-1.5 text-sm font-medium text-red-600 hover:bg-red-50"
        >
          Quit
        </button>
        <button
          onClick={resetDefaults}
          className="rounded-md border border-neutral-300 px-3 py-1.5 text-sm font-medium text-neutral-700 hover:bg-neutral-100"
        >
          Restore defaults
        </button>
      </div>
    </main>
  );
}

export default function App() {
  return windowLabel === "overlay" ? <Overlay /> : <Settings />;
}
