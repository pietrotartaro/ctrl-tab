import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// Medium icon size (px). Keep `ITEM_W` in src-tauri/src/controller.rs in sync with
// ITEM_BOX so the Rust-computed panel width matches the rendered row.
const ICON_SIZE = 72;
const ITEM_BOX = 104; // per-item button width incl. padding + gap

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
      // On hide we keep the last list rendered; the panel is hidden by Rust, so no
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
          {current ? current.title : " "}
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
                style={{ width: ITEM_BOX }}
                className={[
                  "flex shrink-0 flex-col items-center gap-1 rounded-xl px-2 py-2 transition-colors",
                  isSel ? "bg-white/20" : "bg-transparent hover:bg-white/5",
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
                    // Placeholder for a missing icon.
                    <div
                      className="rounded-xl bg-white/10"
                      style={{ width: ICON_SIZE - 8, height: ICON_SIZE - 8 }}
                    />
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

// The only window is the overlay panel.
export default function App() {
  return <Overlay />;
}
