import type { ReactNode } from "react";

export function SettingsSection({
  title,
  children,
}: {
  title: string;
  children: ReactNode;
}) {
  return (
    <details className="settings-section">
      <summary>{title}</summary>
      <div className="settings-section-body">{children}</div>
    </details>
  );
}
