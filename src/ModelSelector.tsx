import { useId, useState } from "react";
import { Check, ChevronDown } from "lucide-react";
import AgentIcon from "./AgentIcon";

// Requested model preferences; availability and switching belong to the external client.
export const MODEL_GROUPS = [
  { provider: "Claude", client: "claude-code", models: ["Claude Opus 5.5 Medium", "Claude Opus 5.5 High"] },
  { provider: "ChatGPT", client: "codex", models: ["GPT-6.1 Sol Medium", "GPT-6.1 Sol High"] },
  { provider: "Grok", client: "grok", models: ["Grok 4.6 Medium", "Grok 4.6 High"] },
];

/** Controlled selector: its caller decides what choosing a model means. */
export default function ModelSelector({ value, onChange, disabled = false, followCurrent = false }: { value: string; onChange: (model: string) => void; disabled?: boolean; followCurrent?: boolean }) {
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});
  const id = useId();
  return <div className="model-selector" aria-label="Choose a model" data-follow-current={followCurrent}>
    {MODEL_GROUPS.map((group) => <div className="model-provider" key={group.provider} role="group" aria-label={group.provider}>
      <button type="button" className="model-provider-heading" aria-expanded={!collapsed[group.provider]} aria-controls={`${id}-${group.client}`} onClick={() => setCollapsed((previous) => ({ ...previous, [group.provider]: !previous[group.provider] }))}>
        <AgentIcon client={group.client} size={16} /><span>{group.provider}</span><ChevronDown size={12} className="push" aria-hidden="true" />
      </button>
      <div className="model-options" id={`${id}-${group.client}`} hidden={!!collapsed[group.provider]}>
        {group.models.map((model) => <button type="button" key={model} disabled={disabled} className="model-option" aria-pressed={model === value} onClick={() => onChange(model)}>
          <span>{model}</span>{model === value && <Check size={14} aria-hidden="true" />}
        </button>)}
      </div>
    </div>)}
  </div>;
}
