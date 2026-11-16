import { useState } from "react";

/** The app's checkbox: a square that fills in (flickering once the person has touched it).
 *  Put it inside a <label> with its text; the real input stays for keyboard and screen readers. */
export default function BoxCheck({ checked, disabled, title, onChange }: {
  checked: boolean; disabled?: boolean; title?: string; onChange: (checked: boolean) => void;
}) {
  const [touched, setTouched] = useState(false);
  return <>
    <input type="checkbox" className="box-check-input" checked={checked} disabled={disabled} title={title}
      onChange={(e) => { setTouched(true); onChange(e.target.checked); }} />
    <span className={`box-check${touched ? " touched" : ""}`} aria-hidden="true" title={title} />
  </>;
}
