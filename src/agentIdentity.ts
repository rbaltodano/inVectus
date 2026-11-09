import type { ClientInfo } from "./api";

/** Names by client id and handshake alias. Starts with the clients known before the catalog
 * loads; `registerClients` adds the backend's catalog. Older records keep Codex's handshake
 * name, and Claude Desktop records its handshake name `claude-ai`. */
const names: Record<string, string> = {
  "claude-code": "Claude Code", "claude-desktop": "Claude Desktop", "claude-ai": "Claude Desktop",
  codex: "Codex", "codex-mcp-client": "Codex", cursor: "Cursor", grok: "Grok Build",
};
const prefixes: [string, string][] = [["grok-shell-", "Grok Build"]];

/** Learn every client's name from the backend's catalog (`connect_clients`). */
export function registerClients(clients: ClientInfo[]) {
  for (const c of clients) {
    names[c.id] = c.display_name;
    for (const a of c.aliases) {
      if (a.endsWith("*")) prefixes.push([a.slice(0, -1).toLowerCase(), c.display_name]);
      else names[a.toLowerCase()] = c.display_name;
    }
  }
}

/** A client's name as people know it. */
export function clientName(client: string | null | undefined): string {
  if (!client) return "An agent";
  const c = client.trim().toLowerCase();
  return names[c] ?? prefixes.find(([p]) => c.startsWith(p))?.[1] ?? client;
}

/** Display only the reported model; never infer a reasoning effort that wasn't supplied. */
export function agentIdentity(author: string): { client: string; label: string } {
  const [client = "", ...parts] = author.replace(/^agent:/, "").split("/");
  const model = parts.join(" ").trim();
  const label = model
    .replace(/[-_]\d{8}(?=$|[-_\s])/g, "")
    .replace(/^claude[-_ ](opus|sonnet|haiku)[-_ ](\d+)(?:[-_.](\d+))?/i,
      (_, family: string, major: string, minor: string | undefined) =>
        `Claude ${family[0].toUpperCase()}${family.slice(1).toLowerCase()} ${major}${minor ? `.${minor}` : ""}`)
    .replace(/^gpt[-_](\d+)(?:[-_.](\d+))?/i,
      (_, major: string, minor: string | undefined) => `GPT-${major}${minor ? `.${minor}` : ""}`)
    .replace(/^grok[-_ ](?=\d)/i, "Grok ")
    .replace(/[-_]+/g, " ")
    .replace(/^GPT (?=\d)/, "GPT-")
    .replace(/\b(low|medium|high|xhigh|mini|nano|codex|sol|terra|luna)\b/gi,
      (word) => word[0].toUpperCase() + word.slice(1).toLowerCase());
  const fallback: Record<string, string> = {
    "claude-code": "Claude", "claude-desktop": "Claude", codex: "Codex", cursor: "Cursor", grok: "Grok",
  };
  return { client, label: label || fallback[client] || "Agent" };
}
