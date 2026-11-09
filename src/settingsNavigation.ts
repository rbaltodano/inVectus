/** Settings search covers the controls inside each category, not just its title. */
export const SETTINGS_SECTIONS = [
  { id: "appearance", label: "Appearance", scope: "app", description: "Make inVectus feel at home on your computer.", keywords: "theme light dark system colors" },
  { id: "models", label: "Models", scope: "app", description: "Manage the local models shared by your projects.", keywords: "embedding bge nomic download install language model llm gemma size dimensions" },
  { id: "agents", label: "AI agents", scope: "app", description: "Connect your agents and review their recent activity.", keywords: "claude desktop code codex chatgpt cursor grok mcp connect disconnect account sign in activity" },
  { id: "network", label: "Network", scope: "project", description: "Choose when this project can use outside sources.", keywords: "offline online privacy pubmed openalex verification cache downloads" },
  { id: "scoring", label: "Gap scoring", scope: "project", description: "Adjust how gaps are ranked for this project.", keywords: "weights demand per coverage item since last momentum share newer than stale bridges closeness little in between recent saturation penalty reset defaults" },
  { id: "calibration", label: "Calibration", scope: "project", description: "Keep collection-level verdicts calibrated to your data.", keywords: "verdict recalibrate auc sample language model collection" },
  { id: "embedding", label: "Embedding details", scope: "project", description: "Inspect the embedding spaces used by this project.", keywords: "space id runtime origin vectors ann partitions dimensions collection model" },
] as const;

export type SettingsSection = typeof SETTINGS_SECTIONS[number]["id"];

export function searchSettings(query: string, extra: Partial<Record<SettingsSection, string>> = {}) {
  const terms = query.toLowerCase().trim().split(/\s+/).filter(Boolean);
  return SETTINGS_SECTIONS.filter((section) => {
    const text = `${section.label} ${section.description} ${section.keywords} ${extra[section.id] ?? ""}`.toLowerCase();
    return terms.every((term) => text.includes(term));
  });
}

/** Arrow keys activate adjacent settings tabs; Home/End reach either end. */
export function settingsTabIndex(key: string, current: number): number | undefined {
  const count = SETTINGS_SECTIONS.length;
  return key === "ArrowRight" ? (current + 1) % count : key === "ArrowLeft" ? (current + count - 1) % count : key === "Home" ? 0 : key === "End" ? count - 1 : undefined;
}
