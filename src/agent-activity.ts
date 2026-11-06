export interface AgentActivity {
  client: string | null; tool: string; project: string; query: string | null;
  gap_id: number | null; ok: boolean; error: string | null; at_ms: number;
  call_id?: string; phase?: "started" | "finished";
}

/** A completed call replaces its in-flight entry, rather than appearing twice. */
export function recordActivity(events: AgentActivity[], event: AgentActivity): AgentActivity[] {
  return [event, ...events.filter((e) => !event.call_id || e.call_id !== event.call_id)].slice(0, 50);
}

const WRITES = new Set(["suggest_decision", "scan_gaps", "submit_description", "submit_region_names", "submit_paraphrases", "verify_gap", "import_collection", "open_project"]);
export function changesProject(event: AgentActivity, root: string): boolean {
  return event.project === root && event.phase !== "started" && event.ok && WRITES.has(event.tool);
}

const clip = (s: string, n = 40) => (s.length > n ? `${s.slice(0, n - 1)}…` : s);
export function describe(a: AgentActivity): string {
  const q = a.query ? ` “${clip(a.query)}”` : "";
  const gap = a.gap_id != null ? ` on gap ${a.gap_id}` : "";
  const labels: Record<string, [string, string]> = {
    search: [`searching${q}`, `searched${q}`], check_idea: [`checking${q}`, `checked${q}`],
    list_gaps: ["reading the gaps", "read the gaps"], gap_detail: [`reading gap ${a.gap_id ?? ""}`.trim(), `read gap ${a.gap_id ?? ""}`.trim()],
    gap_context: [`reading the evidence${gap}`, `read the evidence${gap}`],
    suggest_decision: [`saving a suggestion${gap}`, `suggested a decision${gap}`],
    submit_description: [`checking a description${gap}`, `submitted a description${gap}`],
    paraphrase_sample: ["reading the calibration sample", "read the calibration sample"],
    submit_paraphrases: ["checking paraphrases", "submitted calibration paraphrases"],
    region_context: ["reading the regions", "read the regions"], submit_region_names: ["saving region names", "named regions"],
    scan_gaps: ["scanning gaps", "scanned gaps again"], verify_gap: [`checking outside sources${gap}`, `checked outside sources${gap}`],
    export_gaps: ["exporting gaps", "exported the gaps"], import_collection: ["importing a collection", "imported a collection"],
    project_status: ["reading project status", "read the project status"], open_project: ["opening the project", "opened the project"],
    job_status: ["checking job progress", "checked job progress"], cancel_job: ["requesting cancellation", "requested cancellation"],
  };
  if (!a.ok) return `${a.tool.replace(/_/g, " ")} failed`;
  return labels[a.tool]?.[a.phase === "started" ? 0 : 1] ?? a.tool.replace(/_/g, " ");
}
