import type { RiskLevel, Severity } from "../domain/types";

export function RiskBadge({ level }: { level: RiskLevel }) {
  return (
    <span className={`risk-badge risk-${level}`}>
      <span className="risk-badge-mark" aria-hidden="true" />
      {level}
    </span>
  );
}

export function SeverityBadge({ severity }: { severity: Severity }) {
  return <span className={`severity-badge severity-${severity}`}>{severity}级</span>;
}

