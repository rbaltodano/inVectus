// Types and calls for the desktop backend (src-tauri/src/lib.rs). Shapes mirror the engine's
// serialized structs; only the fields the screens read are listed.
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

export const inTauri = "__TAURI_INTERNALS__" in window;

export type Role = "demand" | "coverage" | "neutral";
export const ROLES: Role[] = ["neutral", "demand", "coverage"];

export interface Collection {
  collection_id: string; items: number; chunks: number; by_role: Record<string, number>; updated_at: string | null;
  source_kind: string | null; source_path: string | null; reimportable: boolean;
}
export interface CalibrationStatus { method: string; sample: number; vectors: number; fitted_at: string | null; holdout_auc: number | null }
export interface RegionsStatus { run_id: string; created_at: string | null; stats: Record<string, unknown>; scanned: boolean }
export interface TableInfo {
  collection: string; space_id: string; model_key: string; runtime: string; dimensions: number; origin: string;
  vectors: number; ann_partitions: number | null; calibration: CalibrationStatus | null; regions: RegionsStatus | null;
  index_status: IndexStatus; indexed_at: string | null; index_error: string | null;
}
export type IndexStatus = "indexing" | "indexed" | "outdated" | "interrupted" | "failed" | "cancelled" | "not_indexed" | "imported" | "unknown";
export interface JobRecord { job_id: string; kind: string; params: Record<string, unknown>; state: string; progress: number; checkpoint: Record<string, unknown>; updated_at: string | null }
export interface JobInfo {
  id: number; kind: "index" | "calibrate" | "regions" | "download"; project: string | null; collection: string | null; space: string | null;
  models: string[]; label: string; phase: string | null; done: number; total: number;
  state: "running" | "completed" | "cancelled" | "failed"; error: string | null;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  report: any; started_ms: number; finished_ms: number | null;
}
export interface MissingModel { key: string; repo: string; megabytes: number }
export interface IndexPlan { models: string[]; missing: MissingModel[]; notes: string[] }
export interface ProjectInfo {
  root: string; name: string; warning: string | null; network_enabled: boolean; offline_env: boolean;
  collections: Collection[]; tables: TableInfo[]; activity: JobRecord[]; running: JobInfo | null;
}
export interface ModelInfo { key: string; repo: string; dimensions: number; megabytes: number; installed: boolean; default: boolean; note: string }
export interface LlmInfo { key: string; repo: string; megabytes: number; installed: boolean; default: boolean; note: string }

export interface SourceInfo { kind: "csv" | "parquet" | "folder"; columns: string[]; sample: (string | null)[][]; rows_read: number; unreadable: number }
export interface ImportForm {
  collection: string; source_path: string; text: string[]; title: string; id: string; date: string; url: string;
  role: Role; role_column: string; role_rules: string; chunk_chars: number; chunk_overlap: number; limit: number | null;
}
export interface MappedRow { source_id: string; title: string | null; text: string; date: string | null; unparsed_date: string | null; role: Role }
export interface Preview { columns: string[]; rows: MappedRow[]; skipped: Record<string, number> }
export interface ImportOutcome { report: ImportReport; plan: IndexPlan; job: JobInfo | null; index_error: string | null }
export interface ImportReport {
  collection: string; source_rows: number; items: number; inserted: number; updated: number; unchanged: number;
  chunks_written: number; items_chunked: number; skipped: Record<string, number>; not_in_source: number; warnings: string[];
}

export interface Item { item_id: number; source_id: string; title: string | null; url: string | null; date: string | null; role: Role }
export interface Hit { rank: number; similarity: number; via_chunk: number | null; item: Item }
export interface SearchResult {
  mode: string; exhaustive: boolean; vectors_compared: number; compatibility: string; model_key: string;
  hits: Hit[]; notes: string[]; embed_ms: number; retrieve_ms: number;
}
export interface NearRegion { region_id: number; level: string; label: string; similarity: number; size: number; stability: number | null }
export interface CheckResult {
  verdict: "exists" | "near" | "open"; p_exists: number; summary: string; next_step: string;
  nearest_similarity: number | null; between: NearRegion[]; position: string | null;
  calibration: { method: string; sample: number; fitted_at: string | null; holdout_auc: number | null; stale: boolean; quality: string };
  search: SearchResult; notes: string[];
}

export interface Region {
  region_id: number; level: "fine" | "broad"; parent_id: number | null; label: string; terms: string[]; size: number;
  stability: number | null; x: number; y: number; radius: number; llm_label: string | null;
}
export interface MapPoint { item_id: number; title: string | null; role: Role; date: string | null; url: string | null; x: number; y: number; fine_id: number; broad_id: number; noise: boolean }
export interface MapData { run: { run_id: string; stats: Record<string, unknown> }; regions: Region[]; points: MapPoint[]; note: string }

export type GapKind = "coverage" | "stale" | "bridge";
export interface Term { name: string; value: number; z: number; weight: number; contribution: number }
export interface YearCount { year: number; demand: number; coverage: number }
export interface OutsideHit { title: string; source: string; year: number | null; url: string | null; similarity: number }
export interface GapVerdictRecord { kind: string; verdict: string; note: string; updated_at: string | null }
export interface GapView {
  gap_id: number; kind: GapKind; score: number; terms: Term[]; region_a: number; region_b: number | null; label: string;
  explanation: string; demand: number; coverage: number; demand_since: number; last_coverage: string | null;
  momentum: number | null; similarity: number | null; bridge_items: number | null; timeline: YearCount[]; core: number[];
  verdict: GapVerdictRecord | null; verification: { matches: number; summary: string; closest: OutsideHit[] } | null;
}
export interface GapList { run_id: string; stats: Record<string, unknown>; gaps: GapView[] }
export interface ItemSummary { item_id: number; title: string | null; date: string | null; role: Role; url: string | null }
export type GapDetail = GapView & { core_items: ItemSummary[] };
export interface ScanReport { coverage: number; stale: number; bridge: number; unstable_regions: number; latest_date: string | null; notes: string[] }
export interface Weights { demand_since: number; coverage_ratio: number; momentum: number; stale_share: number; bridge: number; saturation: number }

export interface Attempt {
  title: string; description: string; seconds: number;
  on_target: { passed: boolean; to_target: number };
  novelty: { passed: boolean; verdict: string; p_exists: number; nearest_title: string | null; nearest_similarity: number | null } | null;
}
export interface DescribeResult { label: string; accepted: number | null; attempts: Attempt[]; notes: string[] }
export interface RankedHit { source: string; title: string; year: number | null; url: string | null; similarity: number; looks_like_match: boolean }
export interface Verification {
  summary: string; matches: number; match_at: number; compared_with: string; hits: RankedHit[]; notes: string[];
  sources: { source: string; query: string; total: number | null; fetched: number; cached: boolean; error: string | null }[];
}

/** The collection and vector space the screens work on. */
export interface TableRef { collection: string; space: string | null }

export const api = {
  version: () => invoke<string>("engine_version"),
  defaultDir: () => invoke<string>("default_projects_dir"),
  open: (path: string, create: boolean) => invoke<ProjectInfo>("open_project", { path, create }),
  info: () => invoke<ProjectInfo>("project_info"),
  rename: (path: string, name: string) => invoke<string>("rename_project", { path, name }),
  setNetwork: (enabled: boolean) => invoke<ProjectInfo>("set_network", { enabled }),
  inspect: (path: string) => invoke<SourceInfo>("inspect_source", { path }),
  preview: (form: ImportForm, rows = 20) => invoke<Preview>("preview_import", { form, rows }),
  importCollection: (form: ImportForm) => invoke<ImportOutcome>("import_collection", { form }),
  reimport: (collection: string) => invoke<ImportOutcome>("reimport_collection", { collection }),
  indexPlan: (collection: string) => invoke<IndexPlan>("index_plan", { collection }),
  startIndex: (collection: string, models: string[] | null, map = false) => invoke<JobInfo>("start_index", { collection, models, map }),
  jobs: () => invoke<{ current: JobInfo | null; history: JobInfo[] }>("jobs"),
  models: () => invoke<ModelInfo[]>("list_models"),
  llms: () => invoke<LlmInfo[]>("list_llms"),
  installModel: (model: string) => invoke<JobInfo>("install_model", { model }),
  startCalibration: (t: TableRef, llm: boolean) => invoke<JobInfo>("start_calibration", { collection: t.collection, space: t.space, llm }),
  startRegions: (t: TableRef) => invoke<JobInfo>("start_regions", { collection: t.collection, space: t.space }),
  cancel: () => invoke<void>("cancel_job"),
  search: (t: TableRef, query: string, role: Role | null, exhaustive: boolean) =>
    invoke<SearchResult>("search", { form: { collection: t.collection, space: t.space, query, k: 10, role, exhaustive } }),
  check: (t: TableRef, query: string, role: Role | null, fast: boolean) =>
    invoke<CheckResult>("check_idea", { form: { collection: t.collection, space: t.space, query, k: 10, role, fast } }),
  map: (t: TableRef) => invoke<MapData>("region_map", { collection: t.collection, space: t.space }),
  scan: (t: TableRef, weights: Weights | null) => invoke<ScanReport>("scan_gaps", { form: t, weights }),
  gaps: (t: TableRef) => invoke<GapList>("list_gaps", { form: t }),
  gap: (t: TableRef, gapId: number) => invoke<GapDetail>("gap_detail", { form: t, gapId }),
  setVerdict: (t: TableRef, gapId: number, verdict: string, note: string) =>
    invoke<GapView>("set_gap_verdict", { form: t, gapId, verdict, note }),
  describe: (t: TableRef, gapId: number, at: number | null) => invoke<DescribeResult>("describe_gap", { form: t, gapId, t: at }),
  verify: (t: TableRef, gapId: number) => invoke<Verification>("verify_gap", { form: t, gapId, network: false }),
  defaultWeights: () => invoke<Weights>("default_gap_weights"),
  exportGaps: (t: TableRef, format: "csv" | "markdown") => invoke<string>("export_gaps", { form: t, format }),
};

/** A web link for an item's url field, which may hold a bare DOI. */
export function webLink(url: string | null | undefined): string | undefined {
  if (!url) return undefined;
  const u = url.trim();
  if (/^https?:\/\//i.test(u)) return u;
  if (/^(doi:)?10\.\d{4,}\//i.test(u)) return `https://doi.org/${u.replace(/^doi:/i, "")}`;
  if (/^(www\.|[a-z0-9-]+\.[a-z]{2,}\/)/i.test(u)) return `https://${u}`;
  return undefined;
}
export const openLink = (url: string) => invoke<void>("open_url", { url });

export const fmt = (n: number) => n.toLocaleString();
/** A DuckDB timestamp ("2026-10-29 07:15:00.123") or epoch ms, shown in local time. */
export function when(t: string | number | null | undefined): string {
  if (t === null || t === undefined) return "";
  const d = typeof t === "number" ? new Date(t) : new Date(t.replace(" ", "T"));
  return Number.isNaN(d.getTime()) ? String(t) : d.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}
export const STATUS_LABEL: Record<IndexStatus, string> = {
  indexing: "Indexing", indexed: "Indexed", outdated: "Imported since last index", interrupted: "Interrupted",
  failed: "Indexing failed", cancelled: "Indexing cancelled", not_indexed: "Not indexed",
  imported: "Imported vectors (query-only)", unknown: "Index state unknown",
};
export const year = (d: string | null) => d?.slice(0, 4) ?? "";
export const KIND_LABEL: Record<GapKind, string> = { coverage: "Coverage gap", stale: "Stale coverage", bridge: "Bridge" };
export const VERDICTS: [string, string][] = [
  ["open", "Open: not decided"], ["confirmed_gap", "Confirmed gap"], ["already_exists", "Already exists"],
  ["not_viable", "Not viable"], ["selected", "Selected to work on"],
];
export const verdictLabel = (v: string | undefined) => VERDICTS.find(([id]) => id === v)?.[1].replace(": not decided", "") ?? "Not decided";

/** The system's own file or folder picker (never a web upload form). Returns a path or null. */
export async function pickSource(folder = false): Promise<string | null> {
  const r = await openDialog(folder
    ? { directory: true, multiple: false, title: "Choose a folder of .txt or .md files" }
    : { directory: false, multiple: false, title: "Choose a dataset", filters: [{ name: "Datasets", extensions: ["csv", "tsv", "parquet", "pq"] }] });
  return typeof r === "string" ? r : null;
}

/** The system's folder picker for an existing project (a `.invectus` folder). */
export async function pickProject(): Promise<string | null> {
  const r = await openDialog({ directory: true, multiple: false, title: "Open an inVectus project (.invectus folder)" });
  return typeof r === "string" ? r : null;
}

/** Per-viewer conveniences kept in browser storage; the app works without them. */
export const local = {
  get<T>(key: string, fallback: T): T {
    try { const v = localStorage.getItem(key); return v ? (JSON.parse(v) as T) : fallback; } catch { return fallback; }
  },
  set(key: string, value: unknown) {
    try { localStorage.setItem(key, JSON.stringify(value)); } catch { /* storage unavailable */ }
  },
};
