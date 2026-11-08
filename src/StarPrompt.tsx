// Asks once for a GitHub star after a few searches. "Star on GitHub" stars in place through
// the person's own `gh` login (no redirect); without it, the button opens the repository page.
// Nothing touches the network before the click. Dismissing, or starring, ends the asking.
import { useEffect, useState } from "react";
import { Star, X } from "lucide-react";
import { api, inTauri, local, openLink } from "./api";

const SEARCHES_KEY = "star-prompt:searches";
const DISMISSED_KEY = "star-prompt:dismissed";
const REPOSITORY_URL = "https://github.com/rbaltodano/inVectus";
const SEARCHES_BEFORE_ASKING = 5;
const IDLE_MS = 30_000;
const THANKS_MS = 5_000;
// Input that means the person is busy; the prompt waits until they pause.
const ACTIVITY_EVENTS = ["pointerdown", "keydown", "wheel", "focus"] as const;

type State = "ask" | "starring" | "thanks" | "open";

let shownThisSession = false;
let show: (() => void) | undefined;

const dismissed = () => local.get<boolean>(DISMISSED_KEY, false);
const dismiss = () => local.set(DISMISSED_KEY, true);

/** Count a finished search or idea check; past the fifth, ask once this session when idle. */
export function recordSearch() {
  if (!inTauri || shownThisSession || dismissed()) return;
  const n = local.get<number>(SEARCHES_KEY, 0) + 1;
  local.set(SEARCHES_KEY, n);
  // Storage that cannot keep the count could not keep a dismissal either: never ask then.
  if (local.get<number>(SEARCHES_KEY, 0) !== n || n <= SEARCHES_BEFORE_ASKING) return;
  shownThisSession = true;
  void waitForIdle().then(() => { if (!dismissed()) show?.(); });
}

/** Resolves after IDLE_MS without input while this window has focus. */
function waitForIdle() {
  return new Promise<void>((resolve) => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    const restart = () => {
      clearTimeout(timer);
      timer = setTimeout(() => {
        if (!document.hasFocus()) return; // the next focus restarts the wait
        for (const t of ACTIVITY_EVENTS) window.removeEventListener(t, restart, true);
        resolve();
      }, IDLE_MS);
    };
    for (const t of ACTIVITY_EVENTS) window.addEventListener(t, restart, { capture: true, passive: true });
    restart();
  });
}

export default function StarPrompt() {
  const [state, setState] = useState<State>();
  useEffect(() => { show = () => setState("ask"); return () => { show = undefined; }; }, []);
  useEffect(() => {
    if (state !== "thanks") return;
    const t = setTimeout(() => setState(undefined), THANKS_MS);
    return () => clearTimeout(t);
  }, [state]);
  if (!state) return null;

  const close = () => { dismiss(); setState(undefined); };
  const openGitHub = () => { close(); void openLink(REPOSITORY_URL).catch(() => undefined); };
  const star = async () => {
    setState("starring");
    const outcome = await api.starRepository().catch(() => "unavailable" as const);
    if (outcome === "starred") { dismiss(); setState("thanks"); } else setState("open");
  };

  return <div className="star-prompt" role="status" aria-live="polite">
    <Star size={16} strokeWidth={1.5} className="star-prompt-icon" aria-hidden="true" />
    <div className="star-prompt-body">
      {state === "thanks"
        ? <><b>Thanks for the star!</b><div className="muted small">It really helps inVectus grow.</div></>
        : <><b>Enjoying inVectus?</b>
          <div className="muted small">{state === "open"
            ? "Couldn't star from here. Open GitHub to star it there."
            : "Give it a star on GitHub to let us know!"}</div>
          <div className="row star-prompt-actions">
            <button type="button" className="text-btn" onClick={close}>Dismiss</button>
            {state === "open"
              ? <button type="button" className="btn primary small" onClick={openGitHub}>Open GitHub</button>
              : <button type="button" className="btn primary small" disabled={state === "starring"} onClick={() => void star()}>
                {state === "starring" ? "Starring…" : "Star on GitHub"}</button>}
          </div></>}
    </div>
    {state !== "starring" && <button type="button" className="star-prompt-close" aria-label="Close" onClick={close}><X size={14} aria-hidden="true" /></button>}
  </div>;
}
