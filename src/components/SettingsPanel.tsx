import { Icon } from "./Icon";
import { NumberField } from "./NumberField";
import { DateTimeField } from "./DateTimeField";
import type { AnalysisSettings } from "../domain/types";

export function SettingsPanel({ settings, onChange, onClose, onApply, busy }: { settings: AnalysisSettings; onChange: (settings: AnalysisSettings) => void; onClose: () => void; onApply: () => void; busy: boolean }) {
  const update = <K extends keyof AnalysisSettings>(key: K, value: AnalysisSettings[K]) => onChange({ ...settings, [key]: value });
  return (
    <div className="panel-backdrop" role="presentation" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      <section className="settings-panel" role="dialog" aria-modal="true" aria-labelledby="settings-title">
        <header><div><span className="detail-kicker">当前会话</span><h2 id="settings-title">分析参数</h2><p>时间范围和频次规则会重新计算统计与风险；人员筛选在结果列表中应用。</p></div><button className="icon-button" type="button" aria-label="关闭参数" onClick={onClose}><Icon name="close" /></button></header>
        <div className="settings-content analysis-mode-list" role="radiogroup" aria-label="频次分析方式">
          <section className={`analysis-mode-option ${settings.frequencyMode === "rolling" ? "is-selected" : ""}`}>
            <label className="analysis-mode-selector"><input type="radio" name="frequency-mode" value="rolling" checked={settings.frequencyMode === "rolling"} onChange={() => update("frequencyMode", "rolling")} /><span><strong>高频入住阈值</strong><small>按滚动 7、30、365 天统计高频入住，适合日常研判。</small></span></label>
            <fieldset className="analysis-mode-fields" disabled={settings.frequencyMode !== "rolling"}><legend className="sr-only">滚动频次参数</legend><div className="field-grid three"><NumberField label="7 天" value={settings.weekThreshold} onChange={(value) => update("weekThreshold", value ?? 1)} required/><NumberField label="30 天" value={settings.monthThreshold} onChange={(value) => update("monthThreshold", value ?? 1)} required/><NumberField label="365 天" value={settings.yearThreshold} onChange={(value) => update("yearThreshold", value ?? 1)} required/></div></fieldset>
          </section>
          <section className={`analysis-mode-option ${settings.frequencyMode === "selected" ? "is-selected" : ""}`}>
            <label className="analysis-mode-selector"><input type="radio" name="frequency-mode" value="selected" checked={settings.frequencyMode === "selected"} onChange={() => update("frequencyMode", "selected")} /><span><strong>选定入住时间范围</strong><small>仅分析指定起止时间内的记录，并使用范围内入住阈值。</small></span></label>
            <fieldset className="analysis-mode-fields" disabled={settings.frequencyMode !== "selected"}><legend className="sr-only">选定范围参数</legend><div className="field-grid three"><DateTimeField label="开始时间" value={settings.frequencyStart} onChange={(value) => update("frequencyStart", value)} required/><DateTimeField label="结束时间" value={settings.frequencyEnd} onChange={(value) => update("frequencyEnd", value)} required/><NumberField label="范围内入住阈值" value={settings.frequencyThreshold} onChange={(value) => update("frequencyThreshold", value ?? 1)} required/></div></fieldset>
          </section>
        </div>
        <footer><button className="button button-quiet" type="button" onClick={onClose}>取消</button><button className="button button-primary" type="button" disabled={busy} onClick={onApply}>{busy ? "正在计算" : "应用参数并重新分析"}</button></footer>
      </section>
    </div>
  );
}
