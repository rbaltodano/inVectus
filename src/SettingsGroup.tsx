import type { ReactNode } from "react";

/** One Settings row: the group's name and purpose on the left, its controls on the right. */
export default function SettingsGroup({ id, title, description, action, children }: {
  id: string; title: string; description?: ReactNode; action?: ReactNode; children: ReactNode;
}) {
  return <section aria-labelledby={id} className="settings-group">
    <div className="settings-group-heading">
      <h3 id={id}>{title}</h3>
      {description && <p>{description}</p>}
      {action && <div className="settings-group-action">{action}</div>}
    </div>
    <div className="settings-group-body">{children}</div>
  </section>;
}
