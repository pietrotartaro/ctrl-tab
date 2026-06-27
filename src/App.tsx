import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

// Both the dev-controls window ("main") and the overlay NSPanel ("overlay")
// load the same bundle. Branch on the window label to decide what to render.
const windowLabel = getCurrentWindow().label;

/** Temporary controls (Phase 0) to show/hide the overlay panel by hand. */
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
      <p className="text-sm text-neutral-500">Phase 0 foundations</p>
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

/** The overlay content. Transparent background; only the card is visible.
    The "Close" button proves the panel receives mouse clicks. */
function Overlay() {
  return (
    <div className="flex h-full w-full items-center justify-center bg-transparent">
      <div className="flex flex-col items-center gap-3 rounded-2xl bg-black/60 px-10 py-8 text-white shadow-2xl backdrop-blur-md">
        <span className="text-base font-medium">ctl-tab overlay</span>
        <span className="text-xs text-white/60">
          transparent · floating · non-activating
        </span>
        <button
          onClick={() => invoke("hide_overlay")}
          className="mt-1 rounded-md bg-white/15 px-3 py-1 text-xs hover:bg-white/25 active:bg-white/35"
        >
          Close (test click)
        </button>
      </div>
    </div>
  );
}

export default function App() {
  return windowLabel === "overlay" ? <Overlay /> : <DevControls />;
}
