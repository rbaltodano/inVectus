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
    .replace(/[-_]+/g, " ")
    .replace(/\b(low|medium|high|xhigh|mini|nano|codex)\b/gi,
      (word) => word[0].toUpperCase() + word.slice(1).toLowerCase());
  const fallback: Record<string, string> = {
    "claude-code": "Claude", "claude-desktop": "Claude", codex: "Codex", cursor: "Cursor",
  };
  return { client, label: label || fallback[client] || "Agent" };
}
