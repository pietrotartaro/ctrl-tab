import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

// Branch on the window label: "overlay" → the Alt-Tab overlay, "settings" → the
// settings window.
const windowLabel = getCurrentWindow().label;

// Medium icon size (px). Keep ITEM_W in src-tauri/src/controller.rs in sync with
// ITEM_BOX so the Rust-computed panel width matches the rendered row.
const ICON_SIZE = 72;
const ITEM_BOX = 104;

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
  const itemRefs = useRef<(HTMLButtonElement | null)[]>([]);

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

  // Keep the selected item visible (scrollbar is hidden via CSS). No smooth
  // animation — instant.
  useEffect(() => {
    itemRefs.current[selected]?.scrollIntoView({ inline: "nearest", block: "nearest" });
  }, [selected, items]);

  const current = items[selected];

  return (
    <div className="flex h-screen w-screen items-center justify-center bg-transparent p-1.5 select-none">
      {/* Solid, fully opaque panel (no blur / no transparency). The Tauri window is
          transparent only so the rounded corners show. */}
      <div className="flex h-full w-full flex-col gap-2 rounded-2xl bg-[#1E1E20] px-5 py-4">
        <div className="truncate text-center text-[15px] font-medium leading-5 text-white">
          {current ? current.title : " "}
        </div>
        <div className="no-scrollbar flex flex-1 items-center justify-center gap-1 overflow-x-auto">
          {items.map((item, i) => {
            const isSel = i === selected;
            return (
              <button
                key={item.id}
                ref={(el) => {
                  itemRefs.current[i] = el;
                }}
                onMouseEnter={() => {
                  setSelected(i);
                  invoke("switcher_hover", { index: i });
                }}
                onClick={() => invoke("switcher_commit", { index: i })}
                style={{ width: ITEM_BOX }}
                className={[
                  "flex shrink-0 flex-col items-center gap-1 rounded-xl px-2 py-2",
                  // Solid highlight color (not an opacity change).
                  isSel ? "bg-[#3A3A3D]" : "bg-transparent",
                ].join(" ")}
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
                      className="rounded-xl bg-[#3A3A3D]"
                      style={{ width: ICON_SIZE - 8, height: ICON_SIZE - 8 }}
                    />
                  )}
                </div>
                <span className="w-full truncate text-center text-[11px] leading-tight text-neutral-300">
                  {item.appName}
                </span>
              </button>
            );
          })}
        </div>
      </div>
    </div>
  );
}

type Combo = { modifiers: number; key_code: number; label: string };
type Config = { switch_app: Combo; switch_windows: Combo };
type ActionKey = "app" | "windows";

/** Settings window: customize the two shortcuts. */
function Settings() {
  const [appLabel, setAppLabel] = useState("…");
  const [winLabel, setWinLabel] = useState("…");
  // Captured combos (modifiers + keyCode) per action; null until (re)recorded.
  const [appCombo, setAppCombo] = useState<{ modifiers: number; keyCode: number } | null>(null);
  const [winCombo, setWinCombo] = useState<{ modifiers: number; keyCode: number } | null>(null);
  const [recording, setRecording] = useState<ActionKey | null>(null);
  const [error, setError] = useState("");
  const [saved, setSaved] = useState(false);

  function applyConfig(cfg: Config) {
    setAppLabel(cfg.switch_app.label);
    setWinLabel(cfg.switch_windows.label);
    setAppCombo({ modifiers: cfg.switch_app.modifiers, keyCode: cfg.switch_app.key_code });
    setWinCombo({ modifiers: cfg.switch_windows.modifiers, keyCode: cfg.switch_windows.key_code });
  }

  useEffect(() => {
    invoke<Config>("get_config").then(applyConfig).catch(() => {});
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
          {recording === action ? "Premi una combinazione…" : label}
        </span>
      </div>
      <button
        onClick={() => record(action)}
        className="rounded-md bg-neutral-900 px-3 py-1.5 text-sm font-medium text-white hover:bg-neutral-700 active:bg-black disabled:opacity-50"
        disabled={recording !== null}
      >
        {recording === action ? "In ascolto…" : "Registra"}
      </button>
    </div>
  );

  return (
    <main className="flex h-screen flex-col gap-3 bg-neutral-50 p-5 text-neutral-900">
      <h1 className="text-base font-semibold">Scorciatoie</h1>
      <Row title="Switch app" label={appLabel} action="app" />
      <Row title="Switch finestre" label={winLabel} action="windows" />

      {error && <p className="text-sm text-red-600">{error}</p>}
      {saved && !error && <p className="text-sm text-green-600">Salvato.</p>}

      <div className="mt-auto flex items-center justify-end gap-2">
        <button
          onClick={resetDefaults}
          className="rounded-md border border-neutral-300 px-3 py-1.5 text-sm font-medium text-neutral-700 hover:bg-neutral-100"
        >
          Ripristina default
        </button>
        <button
          onClick={save}
          className="rounded-md bg-blue-600 px-4 py-1.5 text-sm font-medium text-white hover:bg-blue-500 active:bg-blue-700"
        >
          Salva
        </button>
      </div>
    </main>
  );
}

export default function App() {
  return windowLabel === "overlay" ? <Overlay /> : <Settings />;
}
