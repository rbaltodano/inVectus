/** Pick a column whose name matches one of `names`: the whole name, then a word inside it. */
export function guessColumn(columns: string[], names: string[]): string {
  const lower = columns.map((column) => column.toLowerCase());
  for (const name of names) {
    const exact = lower.indexOf(name);
    if (exact >= 0) return columns[exact];
  }
  for (const name of names) {
    const inside = lower.findIndex((column) =>
      column.split(/[^a-z0-9]+/).some((token) => token === name || token === `${name}s`));
    if (inside >= 0) return columns[inside];
  }
  return "";
}

/** First-pass field mapping for a table. Short names match a word, so `width` is not an id. */
export function guessMapping(columns: string[]) {
  return {
    text: guessColumn(columns, ["abstract", "text", "body", "content", "description", "summary"]),
    title: guessColumn(columns, ["title", "name", "headline"]),
    id: guessColumn(columns, ["id", "pmid", "doi", "key", "uid", "identifier"]),
    date: guessColumn(columns, ["date", "pub_date", "published", "year", "created"]),
    url: guessColumn(columns, ["url", "link", "href", "doi"]),
    role: guessColumn(columns, ["pub_types", "publication_types", "type", "kind", "role"]),
  };
}
