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
  // No column named like text: search the columns that hold prose in the sample, so the table
  // opens ready to import instead of with nothing searched.
  const searched = g.text ? [g.text] : proseColumns(columns, sample).filter((c) => c !== g.id && c !== g.url);
  const base: ColumnDefaults = { searched, title: g.title, id: g.id, date: g.date, url: g.url && g.url !== g.id ? g.url : "", role: g.role };
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
    case "notion": {
      // The pull writes page_id, url, created_time, last_edited_time, then the title property
      // (whatever the database calls it), the other properties, and body last. Search the
      // title, the properties that hold prose, and the page text.
      const fixed = ["page_id", "url", "created_time", "last_edited_time"];
      const title = columns[0] === "page_id" && columns.length > 5 ? columns[4] : base.title;
      const props = columns.filter((c) => !fixed.includes(c) && c !== title && c !== "body");
      const prose = proseColumns(props, sample.map((row) => props.map((c) => row[columns.indexOf(c)] ?? null)));
      const searched = [title, ...prose, has(columns, "body")].filter(Boolean);
      return { ...base, searched: searched.length ? searched : base.searched, title, id: has(columns, "page_id") || base.id, date: base.date || has(columns, "created_time"), url: has(columns, "url") || base.url };
    }
    case "airtable":
      return { ...base, id: has(columns, "record_id") || base.id, date: base.date || has(columns, "created_time") };
    default:
      return base;
  }
}
