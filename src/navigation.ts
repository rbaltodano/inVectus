import type { Page } from "./App";

export type ProjectTab = "search" | "gaps" | "saved" | "collections";
export const PROJECT_TABS: { id: ProjectTab; label: string; page: Page }[] = [
  { id: "search", label: "Search", page: { kind: "search" } },
  { id: "gaps", label: "Gaps", page: { kind: "gaps" } },
  { id: "saved", label: "Saved", page: { kind: "saved" } },
  { id: "collections", label: "Collections", page: { kind: "index" } },
];
export const pageTab = (p: Page): ProjectTab => p.kind === "saved" ? "saved" : p.kind === "search" ? "search" : p.kind === "gaps" || p.kind === "gap" ? "gaps" : p.kind === "index" || p.kind === "import" || p.kind === "add" ? "collections" : "search";

export function projectTab(value: unknown): ProjectTab | undefined {
  return PROJECT_TABS.some((tab) => tab.id === value) ? value as ProjectTab : undefined;
}

/** A plain open restores the last section. An import or a search with a query wins over that memory. */
export function openingTab(initialStack: Page[], remembered: unknown): ProjectTab {
  const last = initialStack[initialStack.length - 1];
  if (!last) return "search";
  const plain = initialStack.length === 1 && last.kind === "search" && last.query === undefined;
  return plain ? projectTab(remembered) ?? "search" : pageTab(last);
}


export type TabStacks = Partial<Record<ProjectTab, Page[]>>;

/** Open a destination without losing the histories of the other tabs. */
export function navigateTo(stacks: TabStacks, from: ProjectTab, page: Page): { tab: ProjectTab; stacks: TabStacks } {
  const tab = page.kind === "item" || page.kind === "gap" ? from : pageTab(page);
  const root = PROJECT_TABS.find((t) => t.id === tab)!.page;
  const pages = stacks[tab] ?? [root];
  return { tab, stacks: { ...stacks, [tab]: page.kind === root.kind ? [page] : [...pages, page] } };
}

/** Saved and Gaps details return to their lists; other details return to Search. */
export function returnFromDetail(stacks: TabStacks, from: ProjectTab): { tab: ProjectTab; stacks: TabStacks } {
  const root = PROJECT_TABS.find((t) => t.id === from)!.page;
  const sourceRoot = stacks[from]?.find((p) => p.kind === root.kind) ?? root;
  if (from === "saved") return { tab: "saved", stacks: { ...stacks, saved: [sourceRoot] } };
  if (from === "gaps") return { tab: "gaps", stacks: { ...stacks, gaps: [sourceRoot] } };
  const search = stacks.search?.find((p) => p.kind === "search") ?? { kind: "search" };
  return { tab: "search", stacks: { ...stacks, [from]: [sourceRoot], search: [search] } };
}

export function tabKeyIndex(key: string, current: number): number | undefined {
  const count = PROJECT_TABS.length;
  return key === "ArrowRight" ? (current + 1) % count : key === "ArrowLeft" ? (current + count - 1) % count : key === "Home" ? 0 : key === "End" ? count - 1 : undefined;
}
