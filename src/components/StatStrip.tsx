import type { AnalysisStats } from "../domain/types";
import { formatInteger } from "../lib/format";

const definitions: Array<{ key: keyof AnalysisStats; label: string; tone?: string }> = [
  { key: "records", label: "有效入住" },
  { key: "people", label: "人员" },
  { key: "alerted", label: "预警人员", tone: "attention" },
  { key: "high", label: "高风险", tone: "danger" },
  { key: "issues", label: "数据问题", tone: "muted" },
];

export function StatStrip({ stats }: { stats: AnalysisStats }) {
  return (
    <section className="stat-strip" aria-label="分析汇总">
      {definitions.map(({ key, label, tone }) => (
        <div className={`stat-item ${tone ? `stat-${tone}` : ""}`} key={key}>
          <span>{label}</span>
          <strong>{formatInteger(stats[key])}</strong>
        </div>
      ))}
    </section>
  );
}

