import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
// Self-hosted JetBrains Mono (bundled by Vite → no CDN, works offline, no font
// flash). Weights used across Settings + switcher: 400/500/600/700.
import "@fontsource/jetbrains-mono/400.css";
import "@fontsource/jetbrains-mono/500.css";
import "@fontsource/jetbrains-mono/600.css";
import "@fontsource/jetbrains-mono/700.css";
import "./index.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
