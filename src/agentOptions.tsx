// Who can do a project's writing work: the connected agent clients and the on-device model,
// each labelled Cloud or Local. Shared by the New project screen and the project header.
import { useCallback, useEffect, useMemo, useState } from "react";
import { Cpu } from "lucide-react";
import AgentIcon, { markFor } from "./AgentIcon";
import { useAgentActivity } from "./agents";
import { api, inTauri, local, type ConnectStatus, type LlmInfo } from "./api";

/** Agents that run on this computer; every other client runs its model in the cloud. */
const LOCAL_CLIENTS = new Set(["on-device", "lmstudio", "ollama", "jan"]);
export const LAST_AGENT = "invectus.last-agent";
export interface AgentOption { id: string; name: string; local: boolean; note: string }

export function useAgentOptions() {
  const [clients, setClients] = useState<ConnectStatus[]>([]);
  const [llm, setLlm] = useState<LlmInfo>();
  const activity = useAgentActivity();
  const refresh = useCallback(() => {
    if (!inTauri) return;
    api.connectStatus().then((c) => setClients(c ?? [])).catch(() => undefined);
    api.llms().then((ls) => setLlm(ls?.find((l) => l.default) ?? ls?.[0])).catch(() => undefined);
  }, []);
  useEffect(refresh, [refresh]);
  const agents = useMemo<AgentOption[]>(() => [
    ...clients.filter((c) => c.connected).map((c) => ({
      id: c.client, name: c.display_name, local: LOCAL_CLIENTS.has(c.client),
      note: LOCAL_CLIENTS.has(c.client) ? "Its model runs on this computer" : "Uses your plan with its provider",
    })),
    { id: "on-device", name: "On-device model", local: true,
      note: llm ? `${llm.key}${llm.installed ? "" : ` · ${(llm.megabytes / 1024).toFixed(1)} GB download when first used`}` : "Runs on this computer, offline" },
  ], [clients, llm]);
  /** The agent chosen last time, else the one that last worked here, else the on-device model. */
  const fallback = useMemo(() => {
    const remembered = local.get<string | null>(LAST_AGENT, null);
    const recent = activity[0] && agents.find((a) => a.id !== "on-device" && markFor(a.id) && markFor(a.id) === markFor(activity[0].client));
    return agents.find((a) => a.id === remembered)?.id ?? recent?.id ?? "on-device";
  }, [agents, activity]);
  return { agents, fallback, refresh };
}

export function AgentMark({ id, size = 18 }: { id: string; size?: number }) {
  return id === "on-device" ? <Cpu size={size} strokeWidth={1.5} aria-hidden="true" /> : <AgentIcon client={id} size={size} />;
}
