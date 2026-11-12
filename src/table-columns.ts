// Pure helpers for Import's table view (no UI), so they can be tested.

/** Columns that hold prose (not numbers, years, IDs or links), for "search every text column". */
export function proseColumns(columns: string[], sample: (string | null)[][]): string[] {
  return columns.filter((_, i) => {
    const vals = sample.map((r) => (r[i] ?? "").trim()).filter(Boolean);
    if (!vals.length) return false;
    const prose = vals.filter((v) => /[A-Za-z]{3,}/.test(v) && /\s/.test(v) && !/^https?:\/\//.test(v) && !/^10\.\d{4,}\//.test(v));
    return prose.length / vals.length >= 0.6;
  });
}
