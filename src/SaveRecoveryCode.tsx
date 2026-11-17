import { useEffect, useState } from "react";
import { api, type ProjectInfo } from "./api";

/** A new encrypted project needs a recoverable key before its first import. The code stays
 * in component memory; it is copied to the clipboard only on the person's explicit click. */
export default function SaveRecoveryCode({ project, onContinue }: {
  project: ProjectInfo; onContinue: () => void;
}) {
  const [code, setCode] = useState<string>();
  const [error, setError] = useState<string>();
  const [copied, setCopied] = useState(false);
  const [saved, setSaved] = useState(false);
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    let active = true;
    setCode(undefined); setError(undefined); setCopied(false); setSaved(false);
    api.projectRecoveryCode(project.root)
      .then((value) => { if (active) setCode(value); })
      .catch((e) => { if (active) setError(String(e)); });
    return () => { active = false; };
  }, [project.root, attempt]);
  const copy = async () => {
    if (!code) return;
    try { await navigator.clipboard.writeText(code); setCopied(true); setError(undefined); }
    catch { setError("Could not copy. Select the code and save it in your password manager or on paper."); }
  };
  return <div className="page new-project">
    <div className="page-head"><div>
      <p className="eyebrow">Project created</p>
      <h1>Save your recovery code</h1>
      <p className="dim new-project-lede">Keep this code for <strong>{project.name}</strong> in a password manager or on paper.
        You need it to open the encrypted database on another computer or if its key is lost.</p>
    </div></div>
    <section className="new-project-section" aria-label="Project recovery code">
      <p className="small">If you lose both the key on this computer and this code, the encrypted data cannot be recovered.
        Anyone with the code can open it.</p>
      {code ? <div className="stack gap-s">
        <code className="mono recovery-code" aria-label="Recovery code">{code}</code>
        <div><button type="button" className="btn small" onClick={() => void copy()}>{copied ? "Copied" : "Copy recovery code"}</button></div>
        <label className="row gap-s"><input type="checkbox" checked={saved} onChange={(e) => setSaved(e.target.checked)} />
          <span className="small">I saved this recovery code somewhere safe.</span></label>
      </div> : !error && <p className="muted small" role="status">Loading recovery code…</p>}
      {error && <p className="err" role="alert">{error}</p>}
      {!code && error && <div><button type="button" className="btn small" onClick={() => setAttempt((n) => n + 1)}>Try again</button></div>}
      <p className="dim small">You can show this code again in Settings → Network → Recovery code.</p>
    </section>
    <div className="new-project-actions"><button type="button" className="btn primary" disabled={!code || !saved}
      onClick={() => { setCode(undefined); onContinue(); }}>Continue</button></div>
  </div>;
}
