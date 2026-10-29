import type { ReactNode } from "react";
import { inTauri, openLink, webLink } from "./api";

/** An item or source title, linked when it has a web address; opens in the system browser. */
export default function Link({ url, children, className }: { url: string | null | undefined; children: ReactNode; className?: string }) {
  const href = webLink(url);
  if (!href) return <span className={className}>{children}</span>;
  return <a href={href} className={["source-link", className].filter(Boolean).join(" ")} target="_blank" rel="noreferrer"
    onClick={(e) => { if (inTauri) { e.preventDefault(); openLink(href).catch(() => undefined); } }}>{children}</a>;
}
