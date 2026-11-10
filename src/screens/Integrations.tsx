// Integrations: what inVectus connects to. AI agents (moved from Settings) work today; database
// sources are a placeholder until a connector design lands (not in SPEC v1; see PROGRESS.md).
import { Blocks, Database } from "lucide-react";
import ConnectAgents from "./ConnectAgents";

export default function Integrations() {
  return <div className="page settings-page integrations-page">
    <header className="settings-header">
      <div className="settings-title"><Blocks size={19} aria-hidden="true" /><h1>Integrations</h1></div>
    </header>
    <div className="settings-main">
      <div className="settings-content">
        <div className="settings-content-inner">
          <section aria-labelledby="i-agents" className="stack gap">
            <div className="settings-page-heading">
              <div className="settings-heading-row"><h2 id="i-agents">AI agents</h2><span className="settings-scope">All projects</span></div>
              <p>Connect your agents and review their recent activity.</p>
            </div>
            <ConnectAgents />
          </section>
          <section aria-labelledby="i-databases" className="stack gap">
            <div className="settings-page-heading">
              <div className="settings-heading-row"><h2 id="i-databases">Databases</h2><span className="settings-scope">Not available yet</span></div>
              <p>Bring a collection in straight from a database.</p>
            </div>
            <div className="settings-project-empty">
              <Database size={24} aria-hidden="true" />
              <h3>Database sources are coming later</h3>
              <p>For now, export a table to CSV, TSV or Parquet and add it from a project's Collections tab.</p>
            </div>
          </section>
        </div>
      </div>
    </div>
  </div>;
}
