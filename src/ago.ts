/** When something happened, relative to `now`: "just now", "5 min ago", "3 h ago", "yesterday",
    "4 days ago", then the date ("Nov 3", or "Nov 3, 2025" in another year). */
export function ago(t: number, now: number = Date.now()): string {
  const s = Math.max(0, Math.round((now - t) / 1000));
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
  const days = Math.floor(s / 86400);
  if (days === 1) return "yesterday";
  if (days < 7) return `${days} days ago`;
  const d = new Date(t);
  const sameYear = d.getFullYear() === new Date(now).getFullYear();
  return d.toLocaleDateString(undefined, sameYear ? { month: "short", day: "numeric" } : { month: "short", day: "numeric", year: "numeric" });
}
