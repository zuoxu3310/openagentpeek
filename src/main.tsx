import React from "react";
import ReactDOM from "react-dom/client";
import { error as logError, warn as logWarn } from "@tauri-apps/plugin-log";
import App from "./App";
import "./index.css";

// Forward uncaught frontend errors into the same log file as the Rust side, so a
// packaged .app (no devtools, no console) is still debuggable.
window.addEventListener("error", (e) => {
  void logError(`${e.message} @ ${e.filename}:${e.lineno}:${e.colno}`);
});
window.addEventListener("unhandledrejection", (e) => {
  void logWarn(`unhandled rejection: ${String(e.reason)}`);
});

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
