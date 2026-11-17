import { useState } from "react";
import { api } from "./api";
import { ErrorLine, useAction } from "./App";
import { baseName } from "./paths";

/** An encrypted project failed to open because its key is not on this computer. */
export const isLocked = (e: unknown) => String(e).includes("project locked:");

/** Asks for a locked project's recovery code, keeps its key here, then reopens it. */
export default function UnlockProject({ path, onUnlocked, onCancel }: {
  path: string; onUnlocked: () => void; onCancel: () => void;
}) {
  const action = useAction();
  const [code, setCode] = useState("");
  const unlock = () => action.run(async () => { await api.unlockProject(path, code); setCode(""); onUnlocked(); });
  return <section className="banner stack gap-s" aria-label="Unlock project">
    <p className="small"><strong>{baseName(path).replace(/\.invectus$/, "")}</strong> is encrypted and its key is not on this computer.
      Enter the project’s recovery code to open it. The key is then kept in this computer’s keychain.</p>
    <form className="row wrap storage-cache-row" onSubmit={(e) => { e.preventDefault(); if (code.trim()) unlock(); }}>
      <input className="mono" aria-label="Recovery code" placeholder="XXXXX-XXXXX-…" value={code}
        onChange={(e) => setCode(e.target.value)} autoComplete="off" spellCheck={false} autoFocus />
      <button type="submit" className="btn small" disabled={action.busy || !code.trim()}>Unlock</button>
      <button type="button" className="text-btn" onClick={onCancel}>Cancel</button>
    </form>
    <ErrorLine error={action.error} />
  </section>;
}
