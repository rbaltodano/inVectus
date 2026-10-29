import { useEffect, useRef, useState } from "react";

/** Shared inline project-title editor: Enter/blur saves; Escape cancels. */
export default function ProjectName({ name, onSave, className = "", disabled = false }: {
  name: string; onSave: (name: string) => Promise<void>; className?: string; disabled?: boolean;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(name);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string>();
  const input = useRef<HTMLInputElement>(null);
  const pending = useRef(false);
  const cancelled = useRef(false);
  useEffect(() => { if (editing) { input.current?.focus(); input.current?.select(); } }, [editing]);
  const save = async () => {
    if (pending.current || cancelled.current) return;
    const title = draft.trim();
    if (!title) { setError("Enter a project name."); return; }
    if (title === name) { cancelled.current = true; setEditing(false); return; }
    pending.current = true; setSaving(true); setError(undefined);
    try { await onSave(title); cancelled.current = true; setEditing(false); }
    catch (e) { setError(String(e)); }
    finally { pending.current = false; setSaving(false); }
  };
  return <span className={`editable-project-name ${className}`}>
    {editing ? <input ref={input} aria-label="Project name" aria-invalid={!!error} value={draft} disabled={saving}
      onChange={(event) => { setDraft(event.target.value); setError(undefined); }} onBlur={() => void save()}
      onKeyDown={(event) => {
        if (event.nativeEvent.isComposing) return;
        if (event.key === "Enter") { event.preventDefault(); void save(); }
        if (event.key === "Escape") { event.preventDefault(); cancelled.current = true; setError(undefined); setEditing(false); }
      }} />
      : <button type="button" className="project-name-button" disabled={disabled} title="Click to rename project"
        aria-label={`Rename project ${name}`} onClick={() => { cancelled.current = false; setDraft(name); setError(undefined); setEditing(true); }}>{name}</button>}
    {error && <span className="project-name-error" role="alert">{error}</span>}
  </span>;
}
