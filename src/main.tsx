import React from "react";
import ReactDOM from "react-dom/client";
import "@fontsource-variable/martian-mono";
import "@fontsource-variable/inter";
import App from "./App";
import { installMotion } from "./motion";

// Scope native material styling to the macOS desktop shell only.
if ("__TAURI_INTERNALS__" in window && /Mac/.test(navigator.platform)) {
  document.documentElement.classList.add("native-macos");
}

installMotion();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
