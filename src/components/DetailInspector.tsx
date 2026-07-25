import { Icon } from "./Icon";
import { RiskBadge, SeverityBadge } from "./RiskBadge";
import type { PersonDetail } from "../domain/types";
import { maskIdentity, maskPhone } from "../lib/format";

export function DetailInspector({ detail, loading, showSensitive, maximized, selectedAlertIndex, onClose, onToggleMaximize, onSelectAlert, onClearAlertFilter }: {
  detail: PersonDetail | null;
  loading: boolean;
  showSensitive: boolean;
  maximized: boolean;
  selectedAlertIndex: number | null;
  onClose: () => void;
  onToggleMaximize: () => void;
  onSelectAlert: (index: number) => void;
  onClearAlertFilter: () => void;
}) {
  const filteredEvidence = (() => {
    if (!detail || selectedAlertIndex === null) return detail?.evidence ?? [];
    const alert = detail.alerts[selectedAlertIndex];
    if (!alert) return [];
    const ids = alert.evidenceIds;
    return detail.evidence.filter((record) => ids.includes(record.uid));
  })();

  return (
    <aside className="detail-inspector" aria-label="人员详情" data-maximized={maximized ? "true" : "false"}>
      {loading || !detail ? (
        <div className="detail-skeleton"><span /><span /><span /><span /><span /></div>
      ) : (
        <>
          <header className="detail-header">
            <div><span className="detail-kicker">人员核查详情</span><h2>{detail.person.name}</h2><p>{showSensitive ? detail.person.idNo : maskIdentity(detail.person.idNo)} · {showSensitive ? detail.person.phone : maskPhone(detail.person.phone)}</p></div>
            <div className="detail-header-actions">
              <button
                className="icon-button"
                type="button"
                aria-label={maximized ? "还原详情" : "最大化详情"}
                aria-pressed={maximized}
                onClick={onToggleMaximize}
              ><Icon name={maximized ? "restore" : "maximize"} /></button>
              <button className="icon-button" type="button" aria-label="关闭详情" onClick={onClose}><Icon name="close" /></button>
            </div>
          </header>
          <div className="detail-risk-line"><RiskBadge level={detail.person.level} /><strong>{detail.person.score}<span>/100</span></strong><span>{detail.person.alertCount} 项预警 · {detail.person.totalRecords} 条有效入住</span></div>
          <div className="detail-scroll">
            <section className="detail-section">
              <h3>人员信息</h3>
              <dl className="person-facts">
                <div><dt>户籍地</dt><dd>{detail.person.householdRegion}</dd></div><div><dt>年龄 / 性别</dt><dd>{detail.person.age ?? "未知"} 岁 · {detail.person.gender || "未知"}</dd></div>
                <div><dt>7 天最大</dt><dd>{detail.person.maxWeekCount ?? 0} 次</dd></div><div><dt>30 天最大</dt><dd>{detail.person.maxMonthCount ?? 0} 次</dd></div><div><dt>365 天最大</dt><dd>{detail.person.maxYearCount ?? 0} 次</dd></div>
              </dl>
            </section>
            <section className="detail-section">
              <div className="detail-section-heading"><h3>预警说明</h3><span>{detail.alerts.length} 项</span></div>
              <div className="alert-list">
                {detail.alerts.length ? detail.alerts.map((alert, index) => {
                  const selected = selectedAlertIndex === index;
                  return (
                    <article
                      className={`alert-item ${selected ? "is-selected" : ""}`}
                      key={`${alert.kind}-${alert.title}`}
                      role="button"
                      tabIndex={0}
                      aria-pressed={selected}
                      onClick={() => onSelectAlert(index)}
                      onKeyDown={(event) => {
                        if (event.key === "Enter" || event.key === " ") {
                          event.preventDefault();
                          onSelectAlert(index);
                        }
                      }}
                    >
                      <div className="alert-heading"><SeverityBadge severity={alert.severity} /><strong>{alert.title}</strong><span>+{alert.score} 分</span></div>
                      <p>{alert.detail}</p>
                      <small>{alert.evidenceCount} 条关联证据{selected ? " · 已筛选证据" : ""}</small>
                    </article>
                  );
                }) : <p className="detail-empty">当前人员未命中预警规则。</p>}
              </div>
            </section>
            <section className="detail-section evidence-section">
              <div className="detail-section-heading">
                <h3>住宿证据</h3>
                <div className="evidence-controls">
                  <button
                    type="button"
                    className={`text-button evidence-all-toggle ${selectedAlertIndex === null ? "is-active" : ""}`}
                    aria-pressed={selectedAlertIndex === null}
                    onClick={onClearAlertFilter}
                  >全部证据</button>
                  <span>{filteredEvidence.length} 条</span>
                </div>
              </div>
              <div className="evidence-list">
                {filteredEvidence.length ? filteredEvidence.map((record) => (
                  <article className="evidence-item" key={record.uid}>
                    <div><strong>{record.hotelName}</strong><span>房间 {record.roomNo || "未填"}</span></div>
                    <p>{record.checkIn} 至 {record.checkOut || "未退房"}</p>
                    <p>{record.region} · {record.address}</p>
                    <small>{record.sourceFile} · 第 {record.sourceRow} 行</small>
                    {record.issues.map((issue) => <span className="issue-tag" key={issue}>{issue}</span>)}
                  </article>
                )) : (
                  selectedAlertIndex !== null
                    ? <p className="detail-empty">该预警无关联证据。</p>
                    : <p className="detail-empty">当前人员没有有效住宿证据。</p>
                )}
              </div>
            </section>
          </div>
        </>
      )}
    </aside>
  );
}
