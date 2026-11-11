import { useEffect, useRef, useState } from "react";

/** Shared inline project-title editor: Enter/blur saves; Escape cancels. */
export default function ProjectName({ name, onSave, onSelect, className = "", disabled = false }: {
  name: string; onSelect?: () => void; onSave: (name: string) => Promise<void>; className?: string; disabled?: boolean;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(name);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string>();
  const input = useRef<HTMLInputElement>(null);
  const beginRename = () => {
    cancelled.current = false; setDraft(name); setError(undefined); setEditing(true);
  };
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
      : <button type="button" className="project-name-button" disabled={disabled} title={onSelect ? "Click to open; double-click to rename" : "Double-click to rename project"}
        aria-label={`${onSelect ? "Open" : "Rename"} project ${name}`}
        onClick={(event) => {
          // One click opens at once (the row stays put, so a second click can still rename).
          if (onSelect) { event.stopPropagation(); if (event.detail <= 1) onSelect(); return; }
          if (event.detail === 0) beginRename();
        }}
        onDoubleClick={beginRename}
        onKeyDown={(event) => { if (event.key === "F2") { event.preventDefault(); beginRename(); } }}>{name}</button>}
    {error && <span className="project-name-error" role="alert">{error}</span>}
  </span>;
}
