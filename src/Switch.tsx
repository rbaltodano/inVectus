import { useState } from "react";

/** The app's on/off switch: a square track whose colour flickers in when it turns on (once the
 *  person has touched it). Put it inside a <label> with its text; the real input stays for
 *  keyboard and screen readers. Pairs with BoxCheck. */
export default function Switch({ checked, disabled, onChange }: {
  checked: boolean; disabled?: boolean; onChange: (checked: boolean) => void;
}) {
  const [touched, setTouched] = useState(false);
  return <>
    <input type="checkbox" role="switch" className="sq-switch-input" checked={checked} disabled={disabled}
      onChange={(e) => { setTouched(true); onChange(e.target.checked); }} />
    <span className={`sq-switch${touched ? " touched" : ""}`} aria-hidden="true" />
  </>;
}
