/** Last path segment. Accepts both `/` and `\` so a Windows path is not shown whole. */
export function baseName(path: string): string {
  const parts = path.split(/[/\\]/).filter((part) => part && part !== ".");
  return parts[parts.length - 1] ?? path;
}

/** File name without its last extension. */
export function fileStem(path: string): string {
  return baseName(path).replace(/\.[^.]+$/, "");
}

/** A collection id guessed from a file or folder name. Empty when nothing usable remains. */
export function collectionIdFromPath(path: string): string {
  return fileStem(path).replace(/[^A-Za-z0-9_-]+/g, "-").replace(/^-+|-+$/g, "").toLowerCase();
}

/** Join a directory and one name, keeping the separator the directory already uses. */
export function joinPath(dir: string, name: string): string {
  const trimmed = dir.trim().replace(/[/\\]+$/, "");
  if (!trimmed) return name;
  const sep = trimmed.includes("\\") && !trimmed.includes("/") ? "\\" : "/";
  return `${trimmed}${sep}${name}`;
}
