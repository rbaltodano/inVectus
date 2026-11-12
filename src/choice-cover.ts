// Overlap between listed source choices (SourcePicker): a whole library includes its
// collections, a collection its sub-collections, a table its views, Canny's "All boards" each
// board. A covered choice cannot be picked beside the one covering it (its items would be
// imported twice).

export interface Coverable { spec: string; title: string; group?: string }

const whole = (c: Coverable) => c.title === "Whole library" || c.spec === "canny:all";

/** Whether picking `a` already brings in everything in `b`. */
export function covers(a: Coverable, b: Coverable): boolean {
  if (a.spec === b.spec) return false;
  if (a.spec === "canny:all") return b.spec.startsWith("canny:board:");
  if ((a.group ?? "") !== (b.group ?? "")) return false;
  return whole(a) || b.title.startsWith(`${a.title} / `);
}

/** The picked choice that covers `c`, if any. */
export function coveredBy<T extends Coverable>(c: T, picked: T[]): T | undefined {
  return picked.find((p) => covers(p, c));
}

/** Toggle `c` in `picked`: picking it drops the choices it covers; a covered choice stays out. */
export function toggle<T extends Coverable>(c: T, picked: T[]): T[] {
  if (picked.some((p) => p.spec === c.spec)) return picked.filter((p) => p.spec !== c.spec);
  if (coveredBy(c, picked)) return picked;
  return [...picked.filter((p) => !covers(c, p)), c];
}
