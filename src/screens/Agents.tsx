// Agents: the AI agent clients (MCP) that work inside inVectus, and what they have done
// through this app. Data sources and outside checks live on the Integrations page.
import ConnectAgents from "./ConnectAgents";

export default function Agents() {
  return <div className="page integrations-page">
    <div className="integrations-inner">
      <header className="integrations-header">
        <h1>Agents</h1>
        <p>Let your AI agents search collections, inspect evidence and suggest decisions. They run on your own plan, on this computer, and their suggestions wait for you to decide.</p>
      </header>
      <section aria-labelledby="a-clients" className="integrations-section">
        <div className="integrations-section-head">
          <h2 id="a-clients">Clients</h2>
          <p>Connect each agent once; it works in whichever project you have open here.</p>
        </div>
        <ConnectAgents />
      </section>
    </div>
  </div>;
}
