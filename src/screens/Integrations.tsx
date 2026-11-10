// Integrations: what inVectus connects to. AI agents (moved from Settings) work today; database
// sources are a placeholder until a connector design lands (not in SPEC v1; see PROGRESS.md).
import { Database } from "lucide-react";
import ConnectAgents from "./ConnectAgents";

export default function Integrations() {
  return <div className="page integrations-page">
    <div className="integrations-inner">
      <header className="integrations-header">
        <h1>Integrations</h1>
        <p>Connect inVectus to your AI agents and, later, to your data.</p>
      </header>
      <section aria-labelledby="i-agents" className="integrations-section">
        <div className="integrations-section-head">
          <h2 id="i-agents">AI agents</h2>
          <p>Let your agents search collections, inspect evidence and suggest decisions. Works for all projects.</p>
        </div>
        <ConnectAgents />
      </section>
      <section aria-labelledby="i-databases" className="integrations-section">
        <div className="integrations-section-head">
          <h2 id="i-databases">Databases</h2>
          <p>Bring a collection in straight from a database.</p>
        </div>
        <ul className="integration-grid">
          <li className="integration-card" data-disabled="true">
            <div className="integration-card-top">
              <span className="integration-mark"><Database size={22} strokeWidth={1.5} aria-hidden="true" /></span>
              <span className="integration-badge integration-badge-muted">Coming later</span>
            </div>
            <div className="integration-name">Database sources</div>
            <p className="integration-desc">For now, export a table to CSV, TSV or Parquet and add it from a project's Collections tab.</p>
          </li>
        </ul>
      </section>
    </div>
  </div>;
}
