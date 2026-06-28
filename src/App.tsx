import { useEffect, useLayoutEffect, useState } from "react";
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

  useEffect(() => {
    const unlisten = [
      listen<ShowPayload>("switcher:show", (e) => {
        setItems(e.payload.items);
        setSelected(e.payload.selected);
      }),
      listen<{ selected: number }>("switcher:select", (e) => {
        setSelected(e.payload.selected);
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
            onMouseEnter={() => {
              setSelected(i);
              invoke("switcher_hover", { index: i });
            }}
            onClick={() => invoke("switcher_commit", { index: i })}
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

/** Settings window: customize the two shortcuts, autostart, and quit. */
function Settings() {
  const [appLabel, setAppLabel] = useState("…");
  const [winLabel, setWinLabel] = useState("…");
  // Captured combos (modifiers + keyCode) per action; null until (re)recorded.
  const [appCombo, setAppCombo] = useState<{ modifiers: number; keyCode: number } | null>(null);
  const [winCombo, setWinCombo] = useState<{ modifiers: number; keyCode: number } | null>(null);
  const [recording, setRecording] = useState<ActionKey | null>(null);
  const [error, setError] = useState("");
  const [saved, setSaved] = useState(false);
  const [autostart, setAutostart] = useState(false);

  function applyConfig(cfg: Config) {
    setAppLabel(cfg.switch_app.label);
    setWinLabel(cfg.switch_windows.label);
    setAppCombo({ modifiers: cfg.switch_app.modifiers, keyCode: cfg.switch_app.key_code });
    setWinCombo({ modifiers: cfg.switch_windows.modifiers, keyCode: cfg.switch_windows.key_code });
  }

  useEffect(() => {
    invoke<Config>("get_config").then(applyConfig).catch(() => {});
    invoke<boolean>("get_autostart").then(setAutostart).catch(() => {});
    const un = listen<{ action: ActionKey; modifiers: number; keyCode: number; label: string }>(
      "recording:done",
      (e) => {
        const { action, modifiers, keyCode, label } = e.payload;
        setRecording(null);
        setSaved(false);
        if (action === "app") {
          setAppCombo({ modifiers, keyCode });
          setAppLabel(label);
        } else {
          setWinCombo({ modifiers, keyCode });
          setWinLabel(label);
        }
      },
    );
    return () => {
      un.then((f) => f());
    };
  }, []);

  function record(action: ActionKey) {
    setError("");
    setSaved(false);
    setRecording(action);
    invoke("start_recording", { action });
  }

  async function save() {
    if (!appCombo || !winCombo) return;
    setError("");
    try {
      const cfg = await invoke<Config>("save_config", {
        appMods: appCombo.modifiers,
        appKey: appCombo.keyCode,
        winMods: winCombo.modifiers,
        winKey: winCombo.keyCode,
      });
      applyConfig(cfg);
      setSaved(true);
    } catch (e) {
      setError(String(e));
    }
  }

  async function resetDefaults() {
    setError("");
    const cfg = await invoke<Config>("reset_config");
    applyConfig(cfg);
    setSaved(true);
  }

  async function toggleAutostart(next: boolean) {
    setAutostart(next); // optimistic
    try {
      await invoke("set_autostart", { enabled: next });
    } catch (e) {
      setError(String(e));
      // revert to the real state on failure
      invoke<boolean>("get_autostart").then(setAutostart).catch(() => {});
    }
  }

  const Row = ({
    title,
    label,
    action,
  }: {
    title: string;
    label: string;
    action: ActionKey;
  }) => (
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

      <label className="flex items-center justify-between gap-3 rounded-lg border border-neutral-200 bg-white px-3 py-2.5">
        <span className="text-sm font-medium text-neutral-800">Launch at login</span>
        <input
          type="checkbox"
          checked={autostart}
          onChange={(e) => toggleAutostart(e.currentTarget.checked)}
          className="h-4 w-4 accent-blue-600"
        />
      </label>

      {error && <p className="text-sm text-red-600">{error}</p>}
      {saved && !error && <p className="text-sm text-green-600">Saved.</p>}

      <div className="mt-auto flex items-center justify-between gap-2">
        <button
          onClick={() => invoke("quit_app")}
          className="rounded-md border border-red-300 px-3 py-1.5 text-sm font-medium text-red-600 hover:bg-red-50"
        >
          Quit
        </button>
        <div className="flex items-center gap-2">
          <button
            onClick={resetDefaults}
            className="rounded-md border border-neutral-300 px-3 py-1.5 text-sm font-medium text-neutral-700 hover:bg-neutral-100"
          >
            Restore defaults
          </button>
          <button
            onClick={save}
            className="rounded-md bg-blue-600 px-4 py-1.5 text-sm font-medium text-white hover:bg-blue-500 active:bg-blue-700"
          >
            Save
          </button>
        </div>
      </div>
    </main>
  );
}

export default function App() {
  return windowLabel === "overlay" ? <Overlay /> : <Settings />;
}
