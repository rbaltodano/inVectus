import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

export default function App() {
  const [version, setVersion] = useState<string>();
  useEffect(() => { invoke<string>("engine_version").then(setVersion).catch(() => setVersion(undefined)); }, []);
  return <main><p className="eyebrow">inVectus · P0</p><h1>Find what’s missing.</h1>
    <p>Evidence comes before an absence claim.</p>
    <section><h2>Engine foundation</h2><p>{version ? `Rust core ${version} connected.` : "Open the desktop app to connect to the Rust core."}</p>
    <p>Import, unSearch, and verification arrive in later milestones.</p></section>
    <footer>Local first · macOS · Windows · Linux</footer></main>;
}
