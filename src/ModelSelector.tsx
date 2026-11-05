import { Check } from "lucide-react";
import AgentIcon from "./AgentIcon";

// Review options for the selector UI, not a catalog of connected clients' capabilities.
export const MODEL_GROUPS = [
  { provider: "Claude", client: "claude-code", models: ["Claude Opus 5.5 Medium", "Claude Opus 5.5 High"] },
  { provider: "ChatGPT", client: "codex", models: ["GPT-6.1 Medium", "GPT-6.1 High"] },
];

/** Controlled selector: its caller decides what choosing a model means. */
export default function ModelSelector({ value, onChange }: { value: string; onChange: (model: string) => void }) {
  return <div className="model-selector" aria-label="Choose a model">
    {MODEL_GROUPS.map((group) => <div className="model-provider" key={group.provider} role="group" aria-label={group.provider}>
      <div className="model-provider-heading"><AgentIcon client={group.client} size={16} /><span>{group.provider}</span></div>
      <div className="model-options">
        {group.models.map((model) => <button type="button" key={model} className="model-option" aria-pressed={model === value} onClick={() => onChange(model)}>
          <span>{model}</span>{model === value && <Check size={14} aria-hidden="true" />}
        </button>)}
      </div>
    </div>)}
  </div>;
}
