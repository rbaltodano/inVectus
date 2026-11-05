import AgentIcon from "./AgentIcon";
import { agentIdentity } from "./agentIdentity";

export default function AgentAttribution({ author }: { author: string }) {
  const { client, label } = agentIdentity(author);
  return <span className="agent-attribution dim small push"><AgentIcon client={client} /><span>{label}</span></span>;
}
