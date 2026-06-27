import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

// Both the dev-controls window ("main") and the overlay NSPanel ("overlay")
// load the same bundle. Branch on the window label.
const windowLabel = getCurrentWindow().label;

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
      // Keep the last list rendered on hide; the panel is hidden by Rust, so no
      // empty flash, and the next show replaces it.
    ];
    return () => {
      unlisten.forEach((p) => p.then((f) => f()));
    };
  }, []);

  const current = items[selected];

  return (
    <div className="flex h-screen w-screen items-center justify-center bg-transparent p-1.5 select-none">
      <div className="flex h-full w-full flex-col gap-2 rounded-2xl border border-white/10 bg-neutral-900/60 px-5 py-4 backdrop-blur-2xl">
        {/* Selected item title */}
        <div className="truncate text-center text-[15px] font-medium leading-5 text-white/90">
          {current ? current.title : " "}
        </div>

        {/* Row of items */}
        <div className="flex flex-1 items-center justify-center gap-1 overflow-x-auto">
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
                className={[
                  "flex w-[88px] shrink-0 flex-col items-center gap-1 rounded-xl px-2 py-2 transition-colors",
                  isSel ? "bg-white/20" : "bg-transparent hover:bg-white/5",
                ].join(" ")}
              >
                <div className="flex h-[72px] w-[72px] items-center justify-center">
                  {item.iconDataUrl ? (
                    <img
                      src={item.iconDataUrl}
                      alt={item.appName}
                      className="h-[72px] w-[72px] object-contain"
                      draggable={false}
                    />
                  ) : (
                    <div className="h-[64px] w-[64px] rounded-xl bg-white/10" />
                  )}
                </div>
                <span className="w-full truncate text-center text-[11px] leading-tight text-white/70">
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

/** Temporary controls (Phase 0) to show/hide the empty overlay by hand. */
function DevControls() {
  const [status, setStatus] = useState("idle");

  async function run(cmd: "show_overlay" | "hide_overlay") {
    try {
      await invoke(cmd);
      setStatus(`${cmd} → ok`);
    } catch (e) {
      setStatus(`${cmd} → error: ${String(e)}`);
    }
  }

  return (
    <main className="flex h-full flex-col items-center justify-center gap-4 bg-neutral-100 p-6 text-neutral-900">
      <h1 className="text-lg font-semibold">ctl-tab — dev controls</h1>
      <p className="text-sm text-neutral-500">
        Hold Ctrl and tap Tab to use the real switcher overlay.
      </p>
      <div className="flex gap-3">
        <button
          onClick={() => run("show_overlay")}
          className="rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-500 active:bg-blue-700"
        >
          Show overlay
        </button>
        <button
          onClick={() => run("hide_overlay")}
          className="rounded-md bg-neutral-700 px-4 py-2 text-sm font-medium text-white hover:bg-neutral-600 active:bg-neutral-800"
        >
          Hide overlay
        </button>
      </div>
      <code className="text-xs text-neutral-400">{status}</code>
    </main>
  );
}

export default function App() {
  return windowLabel === "overlay" ? <Overlay /> : <DevControls />;
}
