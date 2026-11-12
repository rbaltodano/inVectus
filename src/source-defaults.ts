// First guesses for sources whose columns are known: each integration's pull always writes the
// same fixed columns (crates/ingest/src/remote*.rs), so the table can open already right. Roles
// are never guessed. Files, Google Sheets and user-built tables (Notion, Airtable) keep the
// name-based guess, with their fixed id and date columns filled in.
import { guessMapping } from "./column-guess.ts";
import { proseColumns } from "./table-columns.ts";

export interface ColumnDefaults { searched: string[]; title: string; id: string; date: string; url: string; role: string }

const has = (columns: string[], name: string) => (columns.includes(name) ? name : "");
const all = (columns: string[], names: string[]) => names.filter((n) => columns.includes(n));

export function columnDefaults(kind: string, columns: string[], sample: (string | null)[][]): ColumnDefaults {
  const g = guessMapping(columns);
  const base: ColumnDefaults = { searched: g.text ? [g.text] : [], title: g.title, id: g.id, date: g.date, url: g.url && g.url !== g.id ? g.url : "", role: g.role };
  switch (kind) {
    case "zotero":
      return { ...base, searched: all(columns, ["title", "abstract"]), title: has(columns, "title"), id: has(columns, "key"), date: has(columns, "date"), url: has(columns, "url") };
    case "canny":
      return { ...base, searched: all(columns, ["title", "details"]), title: has(columns, "title"), id: has(columns, "post_id"), date: has(columns, "created"), url: has(columns, "url") };
    case "productboard":
      return columns.includes("note_id")
        ? { ...base, searched: all(columns, ["title", "content"]), title: has(columns, "title"), id: "note_id", date: has(columns, "created"), url: has(columns, "url") }
        : { ...base, searched: all(columns, ["name", "description"]), title: has(columns, "name"), id: has(columns, "feature_id"), date: "", url: has(columns, "url") };
    case "typeform":
    case "tally": {
      // Answers are the columns after the id and timestamp; hidden fields are not answers.
      const id = kind === "typeform" ? "response_id" : "submission_id";
      const answers = columns.filter((c) => c !== id && c !== "submitted_at" && !c.startsWith("hidden_"));
      const open = proseColumns(answers, sample.map((row) => answers.map((c) => row[columns.indexOf(c)] ?? null)));
      return { ...base, searched: open.length ? open : base.searched, title: "", id: has(columns, id), date: has(columns, "submitted_at"), url: "" };
    }
    case "notion":
      return { ...base, id: has(columns, "page_id") || base.id, date: base.date || has(columns, "created_time"), url: has(columns, "url") || base.url };
    case "airtable":
      return { ...base, id: has(columns, "record_id") || base.id, date: base.date || has(columns, "created_time") };
    default:
      return base;
  }
}
