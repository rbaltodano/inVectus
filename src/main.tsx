import React from "react";
import ReactDOM from "react-dom/client";
import "@fontsource-variable/martian-mono";
import App from "./App";
import { installMotion } from "./motion";

installMotion();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
