import { useEffect, useRef, useState } from "react";
import { appApi } from "./api";
import { Icon } from "./components/Icon";
import { RiskBadge, SeverityBadge } from "./components/RiskBadge";
import { StatStrip } from "./components/StatStrip";
import type {
  AnalysisSettings,
  ExportKind,
  ImportedRecordsPage,
  ImportedRecordsQuery,
  PersonDetail,
  PersonPage,
  PersonQuery,
  RiskLevel,
  WorkspaceSnapshot,
} from "./domain/types";
import { initialRecordsQuery } from "./domain/types";
import { formatDateTime, formatInteger, maskIdentity, maskPhone } from "./lib/format";

type BusyAction = "boot" | "import" | "reanalyze" | "session" | "export" | "delete" | null;

interface ToastState {
  tone: "info" | "success" | "error";
  message: string;
}

const riskLevels: Array<"全部等级" | RiskLevel> = ["全部等级", "高风险", "中风险", "关注", "正常"];
const exportActions: Array<{ kind: ExportKind; label: string }> = [
  { kind: "summary_csv", label: "人员汇总 CSV" },
  { kind: "risk_xlsx", label: "风险合并 Excel" },
  { kind: "raw_csv", label: "规范化原始 CSV" },
];
const pageSizeOptions = [50, 100, 200] as const;

const initialQuery: PersonQuery = {
  search: "",
  hotelSearch: "",
  hotelProvince: "",
  hotelCity: "",
  hotelCounty: "",
  householdProvince: "",
  householdCity: "",
  householdCounty: "",
  excludeHouseholdProvince: "",
  excludeHouseholdCity: "",
  excludeHouseholdCounty: "",
  minAge: null,
  maxAge: null,
  gender: "",
  level: "全部等级",
  alertState: "全部人员",
  page: 1,
  pageSize: 50,
};

const initialPage: PersonPage = {
  items: [],
  total: 0,
  page: 1,
  pageSize: initialQuery.pageSize,
};

const initialRecordsPage: ImportedRecordsPage = {
  items: [],
  total: 0,
  page: 1,
  pageSize: 50,
};

function App() {
  const [snapshot, setSnapshot] = useState<WorkspaceSnapshot | null>(null);
  const [busy, setBusy] = useState<BusyAction>("boot");
  const [toast, setToast] = useState<ToastState | null>(null);
  const [query, setQuery] = useState<PersonQuery>(initialQuery);
  const [page, setPage] = useState<PersonPage>(initialPage);
  const [pageLoading, setPageLoading] = useState(false);
  const [filterDraft, setFilterDraft] = useState<PersonQuery>(initialQuery);
  const [activeView, setActiveView] = useState<"people" | "records">("people");
  const [recordsPage, setRecordsPage] = useState<ImportedRecordsPage>(initialRecordsPage);
  const [recordsQuery, setRecordsQuery] = useState<ImportedRecordsQuery>(initialRecordsQuery);
  const [recordsFilterDraft, setRecordsFilterDraft] = useState<ImportedRecordsQuery>(initialRecordsQuery);
  const [recordsPageLoading, setRecordsPageLoading] = useState(false);
  const [recordsFilterMenuOpen, setRecordsFilterMenuOpen] = useState(false);
  const [filterMenuOpen, setFilterMenuOpen] = useState(false);
  const [exportMenuOpen, setExportMenuOpen] = useState(false);
  const filterMenuRef = useRef<HTMLDivElement>(null);
  const exportMenuRef = useRef<HTMLDivElement>(null);
  const filterTriggerRef = useRef<HTMLButtonElement>(null);
  const exportTriggerRef = useRef<HTMLButtonElement>(null);
  const recordsFilterMenuRef = useRef<HTMLDivElement>(null);
  const recordsFilterTriggerRef = useRef<HTMLButtonElement>(null);
  const [selectedSessions, setSelectedSessions] = useState<Set<string>>(new Set());
  const [detail, setDetail] = useState<PersonDetail | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [detailMaximized, setDetailMaximized] = useState(false);
  const [selectedAlertIndex, setSelectedAlertIndex] = useState<number | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [draftSettings, setDraftSettings] = useState<AnalysisSettings | null>(null);
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [showSensitive, setShowSensitive] = useState(true);
  const [confirmDelete, setConfirmDelete] = useState(false);

  useEffect(() => {
    let active = true;
    appApi
      .bootstrap()
      .then((data) => {
        if (!active) return;
        setSnapshot(data);
        setDraftSettings(structuredClone(data.settings));
        setSelectedSessions(new Set(data.sourceSessionIds));
      })
      .catch((error: unknown) => {
        if (active) setToast({ tone: "error", message: errorMessage(error) });
      })
      .finally(() => {
        if (active) setBusy(null);
      });
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    if (!toast) return;
    const timeout = window.setTimeout(() => setToast(null), 4200);
    return () => window.clearTimeout(timeout);
  }, [toast]);

  useEffect(() => {
    if (!snapshot || snapshot.mode === "empty") {
      return;
    }
    let active = true;
    Promise.resolve()
      .then(() => {
        if (!active) return null;
        setPageLoading(true);
        setPage((current) => ({ ...current, items: [], page: query.page, pageSize: query.pageSize }));
        return appApi.queryPeople({ ...query, page: Math.max(1, query.page) });
      })
      .then((nextPage) => {
        if (active && nextPage) setPage(nextPage);
      })
      .catch((error: unknown) => {
        if (active) setToast({ tone: "error", message: errorMessage(error) });
      })
      .finally(() => {
        if (active) setPageLoading(false);
      });
    return () => {
      active = false;
    };
  }, [snapshot, query]);

  useEffect(() => {
    if (!snapshot || snapshot.mode === "empty" || activeView !== "records") {
      return;
    }
    let active = true;
    Promise.resolve()
      .then(() => {
        if (!active) return null;
        setRecordsPageLoading(true);
        setRecordsPage((current) => ({ ...current, items: [], page: Math.max(1, recordsQuery.page), pageSize: recordsQuery.pageSize }));
        return appApi.getImportedRecords({ ...recordsQuery, page: Math.max(1, recordsQuery.page) });
      })
      .then((nextPage) => {
        if (active && nextPage) setRecordsPage(nextPage);
      })
      .catch((error: unknown) => {
        if (active) setToast({ tone: "error", message: errorMessage(error) });
      })
      .finally(() => {
        if (active) setRecordsPageLoading(false);
      });
    return () => {
      active = false;
    };
  }, [activeView, recordsQuery, snapshot]);

  useEffect(() => {
    if (!filterMenuOpen && !exportMenuOpen && !recordsFilterMenuOpen && !detailMaximized) return;
    const closeOnOutsidePointer = (event: PointerEvent) => {
      if (!filterMenuOpen && !exportMenuOpen && !recordsFilterMenuOpen) return;
      const target = event.target as Node;
      if (filterMenuOpen && !filterMenuRef.current?.contains(target)) setFilterMenuOpen(false);
      if (exportMenuOpen && !exportMenuRef.current?.contains(target)) setExportMenuOpen(false);
      if (recordsFilterMenuOpen && !recordsFilterMenuRef.current?.contains(target)) setRecordsFilterMenuOpen(false);
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      if (detailMaximized) {
        setDetailMaximized(false);
        return;
      }
      if (filterMenuOpen) filterTriggerRef.current?.focus();
      if (exportMenuOpen) exportTriggerRef.current?.focus();
      if (recordsFilterMenuOpen) recordsFilterTriggerRef.current?.focus();
      setFilterMenuOpen(false);
      setExportMenuOpen(false);
      setRecordsFilterMenuOpen(false);
    };
    document.addEventListener("pointerdown", closeOnOutsidePointer);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("pointerdown", closeOnOutsidePointer);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [exportMenuOpen, filterMenuOpen, recordsFilterMenuOpen, detailMaximized]);

  const totalPages = Math.max(1, Math.ceil(page.total / page.pageSize));
  const recordsTotalPages = Math.max(1, Math.ceil(recordsPage.total / recordsPage.pageSize));
  const activeSession = snapshot?.sessions.find((session) => session.active);

  async function runSnapshotAction(action: BusyAction, operation: () => Promise<WorkspaceSnapshot | null>): Promise<boolean> {
    try {
      setBusy(action);
      const next = await operation();
      if (next) {
        setSnapshot(next);
        setPage(initialPage);
        setDraftSettings(structuredClone(next.settings));
        setQuery((current) => ({ ...current, page: 1 }));
        setDetail(null);
        setDetailMaximized(false);
        setSelectedAlertIndex(null);
        setRecordsPage({ ...initialRecordsPage, pageSize: recordsQuery.pageSize });
        setRecordsQuery((current) => ({ ...current, page: 1 }));
        setRecordsFilterMenuOpen(false);
        setFilterMenuOpen(false);
        setExportMenuOpen(false);
        setSelectedSessions(new Set(next.sourceSessionIds));
        return true;
      }
      return false;
    } catch (error) {
      setToast({ tone: "error", message: errorMessage(error) });
      return false;
    } finally {
      setBusy(null);
    }
  }

  async function openPerson(personKey: string) {
    try {
      setDetailLoading(true);
      setSelectedAlertIndex(null);
      const next = await appApi.getPersonDetail(personKey);
      setDetail(next);
    } catch (error) {
      setToast({ tone: "error", message: errorMessage(error) });
    } finally {
      setDetailLoading(false);
    }
  }

  function openImportedRecords() {
    setActiveView("records");
  }

  function openSettings() {
    if (!snapshot) return;
    setDraftSettings(structuredClone(snapshot.settings));
    setSettingsOpen(true);
  }

  async function applySettings() {
    if (!draftSettings) return;
    const activeThresholds = draftSettings.frequencyMode === "selected"
      ? [draftSettings.frequencyThreshold]
      : [draftSettings.weekThreshold, draftSettings.monthThreshold, draftSettings.yearThreshold];
    if (activeThresholds.some((value) => value < 1)) {
      setToast({ tone: "error", message: "频次阈值必须是大于 0 的整数。" });
      return;
    }
    if (draftSettings.frequencyMode === "selected" && (!draftSettings.frequencyStart || !draftSettings.frequencyEnd)) {
      setToast({ tone: "error", message: "选定入住时间范围时，开始时间和结束时间均为必填。" });
      return;
    }
    if (draftSettings.frequencyMode === "selected" && draftSettings.frequencyStart! > draftSettings.frequencyEnd!) {
      setToast({ tone: "error", message: "入住开始时间不能晚于结束时间。" });
      return;
    }
    setSettingsOpen(false);
    const completed = await runSnapshotAction("reanalyze", () => appApi.reanalyze(draftSettings));
    if (completed) setToast({ tone: "success", message: "已按新的分析口径重新计算。" });
  }

  function applyResultFilters() {
    if (filterDraft.minAge != null && filterDraft.maxAge != null && filterDraft.minAge > filterDraft.maxAge) {
      setToast({ tone: "error", message: "最小年龄不能大于最大年龄。" });
      return;
    }
    setFilterMenuOpen(false);
    setQuery({ ...filterDraft, page: 1 });
  }

  function applyRecordsFilters() {
    if (recordsFilterDraft.minAge != null && recordsFilterDraft.maxAge != null && recordsFilterDraft.minAge > recordsFilterDraft.maxAge) {
      setToast({ tone: "error", message: "最小年龄不能大于最大年龄。" });
      return;
    }
    setRecordsFilterMenuOpen(false);
    setRecordsQuery({ ...recordsFilterDraft, page: 1 });
  }

  function clearRecordsFilters() {
    setRecordsFilterDraft((current) => ({ ...initialRecordsQuery, pageSize: current.pageSize }));
    setRecordsQuery((current) => ({ ...initialRecordsQuery, pageSize: current.pageSize, page: 1 }));
  }

  async function exportResult(kind: ExportKind) {
    try {
      setExportMenuOpen(false);
      setBusy("export");
      const result = await appApi.exportResult(kind);
      setToast({ tone: result.path ? "success" : "info", message: result.message });
    } catch (error) {
      setToast({ tone: "error", message: errorMessage(error) });
    } finally {
      setBusy(null);
    }
  }

  async function changeStorageDirectory() {
    try {
      const result = await appApi.chooseStorageDirectory();
      if (result) setToast({ tone: result.path ? "success" : "info", message: result.message });
    } catch (error) {
      setToast({ tone: "error", message: errorMessage(error) });
    }
  }

  function toggleSession(sessionId: string) {
    setSelectedSessions((current) => {
      const next = new Set(current);
      if (next.has(sessionId)) next.delete(sessionId);
      else next.add(sessionId);
      return next;
    });
  }

  async function deleteCurrentSession() {
    if (!activeSession) return;
    setConfirmDelete(false);
    const completed = await runSnapshotAction("delete", () => appApi.deleteSession(activeSession.sessionId));
    if (completed) setToast({ tone: "success", message: "当前历史会话已删除，原始 Excel 文件没有变更。" });
  }

  if (!snapshot && busy === "boot") {
    return <LoadingShell />;
  }

  if (!snapshot) {
    return (
      <main className="fatal-state">
        <Icon name="warning" size={26} />
        <h1>无法初始化工作区</h1>
        <p>请重新启动应用；如果问题持续出现，请检查本地数据目录权限。</p>
      </main>
    );
  }

  return (
    <div className={`app-shell ${sidebarOpen ? "sidebar-visible" : "sidebar-hidden"}`}>
      <header className="topbar">
        <div className="brand-block">
          <button
            className="icon-button sidebar-toggle"
            type="button"
            onClick={() => setSidebarOpen((value) => !value)}
            aria-label={sidebarOpen ? "收起控制区" : "展开控制区"}
          >
            <Icon name="menu" />
          </button>
          <div className="app-mark" aria-hidden="true">麦</div>
          <div>
            <div className="brand-line">
              <strong>麦隐研判</strong>
              <span>内部工具</span>
            </div>
            <p>旅馆业入住数据预警分析</p>
          </div>
        </div>
        <div className="topbar-actions">
          <span className="local-assurance"><Icon name="shield" size={16} /> 数据仅在本机处理</span>
          <button className="button button-quiet" type="button" onClick={() => exportResult("template_xlsx")}>
            <Icon name="download" /> 下载导入模板
          </button>
          <button className="button button-danger-quiet" type="button" disabled={busy !== null} onClick={() => runSnapshotAction("session", () => appApi.clearWorkspace())}>
            <Icon name="trash" /> 清空工作区
          </button>
        </div>
      </header>

      {appApi.runtime === "browser" && (
        <div className="demo-notice" role="status">
          <Icon name="info" size={16} />
          <span><strong>浏览器演示模式</strong>：当前数据用于预览界面，真实 Excel 解析、历史和导出由 Tauri/Rust 提供。</span>
        </div>
      )}

      <aside className="sidebar" aria-label="数据与分析控制">
        <section className="sidebar-section import-section">
          <div className="section-heading">
            <div><span className="step-number">01</span><h2>导入数据</h2></div>
            <span className="section-hint">XLS · XLSX · CSV</span>
          </div>
          <div className="import-actions">
            <button className="button button-primary" type="button" disabled={busy !== null} onClick={() => runSnapshotAction("import", () => appApi.importFiles())}>
              <Icon name="upload" /> 选择文件
            </button>
            <button className="button button-secondary" type="button" disabled={busy !== null} onClick={() => runSnapshotAction("import", () => appApi.importFolder())}>
              <Icon name="folder" /> 选择文件夹
            </button>
          </div>
          {busy === "import" && (
            <div className="inline-progress" role="status">
              <span className="progress-track"><span /></span>
              <p>正在读取并规范化数据，请勿关闭窗口</p>
            </div>
          )}
        </section>

        <section className="sidebar-section history-section">
          <div className="section-heading">
            <div><span className="step-number">02</span><h2>历史数据</h2></div>
            <span className="count-label">{snapshot.sessions.length} 条</span>
          </div>
          <div className="history-list" role="list" aria-label="本地历史会话">
            {snapshot.sessions.map((session) => (
              <label className={`history-item ${session.active ? "is-active" : ""}`} key={session.sessionId}>
                <input
                  type="checkbox"
                  checked={selectedSessions.has(session.sessionId)}
                  disabled={busy !== null}
                  onChange={() => toggleSession(session.sessionId)}
                />
                <span className="custom-checkbox" aria-hidden="true" />
                <button
                  type="button"
                  className="history-content"
                  disabled={busy !== null}
                  onClick={(event) => {
                    event.preventDefault();
                    void runSnapshotAction("session", () => appApi.loadSession(session.sessionId));
                  }}
                >
                  <span className="history-title">{session.fileName}</span>
                  <span className="history-meta">{formatDateTime(session.importedAt)} · {formatInteger(session.records)} 条</span>
                </button>
              </label>
            ))}
          </div>
          <button
            className="button button-secondary full-width"
            type="button"
            disabled={selectedSessions.size < 2 || busy !== null}
            onClick={() => runSnapshotAction("session", () => appApi.mergeSessions([...selectedSessions]))}
          >
            <Icon name="archive" /> 合并所选 {selectedSessions.size > 1 ? `${selectedSessions.size} 条` : ""}
          </button>
          {activeSession && (
            <button className="text-button danger-text" type="button" disabled={busy !== null} onClick={() => setConfirmDelete(true)}>
              删除当前历史会话
            </button>
          )}
          {busy === "delete" && (
            <div className="inline-progress" role="status" aria-label="正在删除历史会话，请稍候…">
              <span className="progress-track"><span /></span>
              <p>正在删除历史会话，请稍候…</p>
            </div>
          )}
        </section>

        <section className="sidebar-section scope-section">
          <div className="section-heading">
            <div><span className="step-number">03</span><h2>当前分析口径</h2></div>
          </div>
          <dl className="scope-summary">
            <div><dt>入住时间</dt><dd>{analysisTimeScopeLabel(snapshot.settings)}</dd></div>
            <div><dt>频次规则</dt><dd>{frequencyScopeLabel(snapshot.settings)}</dd></div>
          </dl>
          <button className="button button-secondary full-width" type="button" disabled={busy !== null} onClick={openSettings}>
            <Icon name="settings" /> 调整分析参数
          </button>
        </section>

        <section className="sidebar-footer">
          <div><Icon name="database" size={17} /><span><strong>数据存放目录</strong><small>MaiyinAnalysisData</small></span></div>
          <button className="text-button" type="button" onClick={changeStorageDirectory}>更改</button>
        </section>
      </aside>

      <main className="workspace" id="main-workspace">
        <section className="workspace-header">
          <div>
            <div className="eyebrow-row">
              <span className={`workspace-mode mode-${snapshot.mode}`}>{modeLabel(snapshot.mode)}</span>
              <span>生成于 {formatDateTime(snapshot.generatedAt)}</span>
            </div>
            <h1>{snapshot.title}</h1>
            <p>{snapshot.subtitle}</p>
          </div>
          <div className="workspace-header-actions">
            <label className="privacy-toggle">
              <input type="checkbox" checked={showSensitive} onChange={(event) => setShowSensitive(event.target.checked)} />
              <span>显示完整身份信息</span>
            </label>
            <button className="button button-secondary" type="button" disabled={busy !== null || snapshot.mode === "empty"} onClick={() => runSnapshotAction("reanalyze", () => appApi.reanalyze(snapshot.settings))}>
              <Icon name="refresh" /> 重新分析
            </button>
          </div>
        </section>

        <StatStrip stats={snapshot.stats} />

        {snapshot.mode !== "empty" && (
          <nav className="workspace-tabs" aria-label="数据视图" role="tablist">
            <button
              id="people-tab"
              className={activeView === "people" ? "is-active" : ""}
              type="button"
              role="tab"
              aria-controls="people-panel"
              aria-selected={activeView === "people"}
              onClick={() => setActiveView("people")}
            >
              <Icon name="users" size={15} />
              <span className="tab-label">人员研判</span>
              <span className="tab-count">{formatInteger(snapshot.stats.people)}</span>
            </button>
            <button
              id="records-tab"
              className={activeView === "records" ? "is-active" : ""}
              type="button"
              role="tab"
              aria-controls="records-panel"
              aria-selected={activeView === "records"}
              onClick={openImportedRecords}
            >
              <Icon name="file" size={15} />
              <span className="tab-label">导入记录</span>
              <span className="tab-count">{formatInteger(snapshot.stats.records)}</span>
            </button>
          </nav>
        )}

        {snapshot.mode === "empty" ? (
          <EmptyWorkspace onFiles={() => runSnapshotAction("import", () => appApi.importFiles())} onFolder={() => runSnapshotAction("import", () => appApi.importFolder())} />
        ) : (
          activeView === "records" ? (
            <ImportedRecordsTable
              page={recordsPage}
              loading={recordsPageLoading}
              showSensitive={showSensitive}
              timeScoped={snapshot.settings.frequencyMode === "selected"}
              totalPages={recordsTotalPages}
              filterDraft={recordsFilterDraft}
              onFilterDraftChange={setRecordsFilterDraft}
              onApplyFilters={applyRecordsFilters}
              onClearFilters={clearRecordsFilters}
              activeFilterCount={activeRecordsFilterCount(recordsFilterDraft)}
              filterMenuOpen={recordsFilterMenuOpen}
              onFilterMenuToggle={() => {
                setRecordsFilterMenuOpen((open) => !open);
                setFilterMenuOpen(false);
                setExportMenuOpen(false);
              }}
              filterMenuRef={recordsFilterMenuRef}
              filterTriggerRef={recordsFilterTriggerRef}
              onPageChange={(nextPage) => setRecordsQuery((current) => ({ ...current, page: nextPage }))}
              onPageSizeChange={(pageSize) => {
                setRecordsFilterDraft((current) => ({ ...current, pageSize }));
                setRecordsQuery((current) => ({ ...current, page: 1, pageSize }));
              }}
            />
          ) : <section className="results-region" id="people-panel" role="tabpanel" aria-labelledby="people-tab" aria-label="人员分析结果">
            <div className="result-toolbar">
              <div className="search-field">
                <Icon name="search" size={17} />
                <input
                  aria-label="搜索人员"
                  placeholder="搜索姓名、证件号、手机号、户籍地或预警"
                  value={filterDraft.search}
                  onChange={(event) => setFilterDraft((current) => ({ ...current, search: event.target.value }))}
                />
                {filterDraft.search && <button type="button" aria-label="清除搜索" onClick={() => setFilterDraft((current) => ({ ...current, search: "" }))}><Icon name="close" size={15} /></button>}
              </div>
              <select aria-label="风险等级" value={filterDraft.level} onChange={(event) => setFilterDraft((current) => ({ ...current, level: event.target.value as PersonQuery["level"] }))}>
                {riskLevels.map((level) => <option key={level}>{level}</option>)}
              </select>
              <button className="button button-primary compact" type="button" onClick={applyResultFilters}>应用筛选</button>
              <div className="toolbar-menu filter-menu" data-open={filterMenuOpen} ref={filterMenuRef}>
                <button
                  className="button button-quiet compact toolbar-trigger"
                  type="button"
                  aria-expanded={filterMenuOpen}
                  aria-controls="filter-popover"
                  ref={filterTriggerRef}
                  onClick={() => {
                    setFilterMenuOpen((open) => !open);
                    setExportMenuOpen(false);
                  }}
                ><Icon name="filter" size={16} /> 更多筛选{activeExtraFilterCount(filterDraft) > 0 && <span className="filter-count">{activeExtraFilterCount(filterDraft)}</span>}</button>
                {filterMenuOpen && <div className="toolbar-popover filter-popover" id="filter-popover">
                  <section className="filter-group" aria-labelledby="hotel-filter-title">
                    <div className="filter-group-heading"><strong id="hotel-filter-title">入住旅馆</strong><span>多个名称用逗号分隔，需全部命中</span></div>
                    <label className="field filter-wide-field"><span>旅馆名称</span><input placeholder="例如：旅馆 A，旅馆 B" value={filterDraft.hotelSearch} onChange={(event) => setFilterDraft((current) => ({ ...current, hotelSearch: event.target.value }))} /></label>
                    <div className="filter-field-grid three">
                      <Field label="旅馆省份" value={filterDraft.hotelProvince ?? ""} onChange={(value) => setFilterDraft((current) => ({ ...current, hotelProvince: value }))} />
                      <Field label="旅馆城市" value={filterDraft.hotelCity ?? ""} onChange={(value) => setFilterDraft((current) => ({ ...current, hotelCity: value }))} />
                      <Field label="旅馆县区" value={filterDraft.hotelCounty ?? ""} onChange={(value) => setFilterDraft((current) => ({ ...current, hotelCounty: value }))} />
                    </div>
                  </section>
                  <section className="filter-group" aria-labelledby="household-filter-title">
                    <div className="filter-group-heading"><strong id="household-filter-title">人员户籍地</strong><span>包含条件与排除条件分别组合匹配</span></div>
                    <div className="filter-subgroup"><span>包含户籍地</span><div className="filter-field-grid three">
                      <Field label="省份" value={filterDraft.householdProvince ?? ""} onChange={(value) => setFilterDraft((current) => ({ ...current, householdProvince: value }))} />
                      <Field label="城市" value={filterDraft.householdCity ?? ""} onChange={(value) => setFilterDraft((current) => ({ ...current, householdCity: value }))} />
                      <Field label="县区" value={filterDraft.householdCounty ?? ""} onChange={(value) => setFilterDraft((current) => ({ ...current, householdCounty: value }))} />
                    </div></div>
                    <div className="filter-subgroup"><span>排除户籍地</span><div className="filter-field-grid three">
                      <Field label="省份" value={filterDraft.excludeHouseholdProvince ?? ""} onChange={(value) => setFilterDraft((current) => ({ ...current, excludeHouseholdProvince: value }))} />
                      <Field label="城市" value={filterDraft.excludeHouseholdCity ?? ""} onChange={(value) => setFilterDraft((current) => ({ ...current, excludeHouseholdCity: value }))} />
                      <Field label="县区" value={filterDraft.excludeHouseholdCounty ?? ""} onChange={(value) => setFilterDraft((current) => ({ ...current, excludeHouseholdCounty: value }))} />
                    </div></div>
                  </section>
                  <section className="filter-group" aria-labelledby="person-filter-title">
                    <div className="filter-group-heading"><strong id="person-filter-title">人员条件</strong><span>仅筛选结果，不改变风险评分</span></div>
                    <div className="filter-field-grid four">
                      <NumberField label="最小年龄" value={filterDraft.minAge ?? null} onChange={(value) => setFilterDraft((current) => ({ ...current, minAge: value }))} />
                      <NumberField label="最大年龄" value={filterDraft.maxAge ?? null} onChange={(value) => setFilterDraft((current) => ({ ...current, maxAge: value }))} />
                      <label className="field"><span>性别</span><select value={filterDraft.gender ?? ""} onChange={(event) => setFilterDraft((current) => ({ ...current, gender: event.target.value as PersonQuery["gender"] }))}><option value="">不限</option><option>男</option><option>女</option></select></label>
                      <label className="field"><span>预警状态</span><select value={filterDraft.alertState} onChange={(event) => setFilterDraft((current) => ({ ...current, alertState: event.target.value as PersonQuery["alertState"] }))}><option>全部人员</option><option>仅预警人员</option><option>未预警人员</option></select></label>
                    </div>
                  </section>
                  <div className="popover-actions"><button className="text-button" type="button" onClick={() => {
                    setFilterDraft((current) => ({ ...initialQuery, pageSize: current.pageSize }));
                    setQuery((current) => ({ ...initialQuery, pageSize: current.pageSize }));
                  }}>清除全部筛选</button></div>
                </div>}
              </div>
              <div className="toolbar-spacer" />
              <div className="toolbar-menu export-menu" data-open={exportMenuOpen} ref={exportMenuRef}>
                <button
                  className="button button-secondary compact toolbar-trigger"
                  type="button"
                  aria-expanded={exportMenuOpen}
                  aria-controls="export-popover"
                  ref={exportTriggerRef}
                  onClick={() => {
                    setExportMenuOpen((open) => !open);
                    setFilterMenuOpen(false);
                  }}
                ><Icon name="download" size={16} /> 导出</button>
                {exportMenuOpen && <div className="toolbar-popover export-popover" id="export-popover" aria-label="导出当前结果">
                  {exportActions.map((action) => <button key={action.kind} type="button" disabled={busy === "export"} onClick={() => exportResult(action.kind)}><span>{action.label}</span><Icon name="chevronRight" size={15} /></button>)}
                </div>}
              </div>
            </div>

            <div className="table-frame" aria-busy={pageLoading}>
              <table className="people-table">
                <thead>
                  <tr>
                    <th scope="col" className="people-col-person">人员</th><th scope="col" className="people-col-household">户籍地</th><th scope="col" className="numeric people-col-count">记录</th>
                    <th scope="col" className="numeric people-col-frequency">7 天</th><th scope="col" className="numeric people-col-frequency">30 天</th><th scope="col" className="numeric people-col-frequency">365 天</th>
                    <th scope="col" className="people-col-alerts">预警依据</th><th scope="col" className="numeric people-col-score">风险分</th><th scope="col" className="people-col-level">等级</th><th scope="col" className="people-col-action"><span className="sr-only">查看</span></th>
                  </tr>
                </thead>
                <tbody>
                  {page.items.map((person) => (
                    <tr key={person.personKey} className={detail?.person.personKey === person.personKey ? "is-selected" : ""}>
                      <td>
                        <button className="person-cell" type="button" onClick={() => openPerson(person.personKey)}>
                          <strong>{person.name}</strong>
                          <span>{showSensitive ? person.idNo : maskIdentity(person.idNo)} · {showSensitive ? person.phone : maskPhone(person.phone)}</span>
                        </button>
                      </td>
                      <td><span className="primary-cell-text">{person.householdRegion}</span><small>{person.gender || "未知"} · {person.age ?? "未知"} 岁</small></td>
                      <td className="numeric strong-number">{person.totalRecords}</td>
                      <td className="numeric">{person.maxWeekCount ?? 0}</td>
                      <td className="numeric">{person.maxMonthCount}</td>
                      <td className="numeric">{person.maxYearCount}</td>
                      <td><div className="alert-summary">{person.alertTitles.length ? person.alertTitles.slice(0, 2).map((title) => <span key={title}>{title}</span>) : <span className="no-alert">未命中预警</span>}</div></td>
                      <td className="numeric score-cell"><strong>{person.score}</strong><span>/100</span></td>
                      <td><RiskBadge level={person.level} /></td>
                      <td><button className="icon-button row-action" type="button" aria-label={`查看 ${person.name} 详情`} onClick={() => openPerson(person.personKey)}><Icon name="chevronRight" size={17} /></button></td>
                    </tr>
                  ))}
                </tbody>
              </table>
              {pageLoading && page.items.length === 0 ? <TableSkeleton /> : page.items.length === 0 && <div className="no-results"><Icon name="search" size={22} /><strong>没有符合条件的人员</strong><span>调整搜索词或筛选条件后重试。</span></div>}
            </div>

            <footer className="table-footer">
              <div className="page-summary"><span>共 {formatInteger(page.total)} 人</span><PageSizeSelect label="人员每页数量" unit="人" value={query.pageSize} onChange={(pageSize) => {
                setFilterDraft((current) => ({ ...current, page: 1, pageSize }));
                setQuery((current) => ({ ...current, page: 1, pageSize }));
              }} /></div>
              <div className="pagination">
                <button className="icon-button" type="button" aria-label="上一页" disabled={pageLoading || query.page <= 1} onClick={() => setQuery((current) => ({ ...current, page: current.page - 1 }))}><Icon name="chevronLeft" /></button>
                <span>第 {query.page} / {totalPages} 页</span>
                <button className="icon-button" type="button" aria-label="下一页" disabled={pageLoading || query.page >= totalPages} onClick={() => setQuery((current) => ({ ...current, page: current.page + 1 }))}><Icon name="chevronRight" /></button>
              </div>
            </footer>
          </section>
        )}
      </main>

      {(detail || detailLoading) && (
        <DetailInspector
          detail={detail}
          loading={detailLoading}
          showSensitive={showSensitive}
          maximized={detailMaximized}
          selectedAlertIndex={selectedAlertIndex}
          onClose={() => {
            setDetail(null);
            setDetailMaximized(false);
            setSelectedAlertIndex(null);
          }}
          onToggleMaximize={() => setDetailMaximized((value) => !value)}
          onSelectAlert={(index) => setSelectedAlertIndex((current) => (current === index ? null : index))}
          onClearAlertFilter={() => setSelectedAlertIndex(null)}
        />
      )}

      {settingsOpen && draftSettings && (
        <SettingsPanel settings={draftSettings} onChange={setDraftSettings} onClose={() => setSettingsOpen(false)} onApply={applySettings} busy={busy === "reanalyze"} />
      )}

      {confirmDelete && activeSession && (
        <ConfirmDialog
          title="删除当前历史会话"
          description={`将删除“${activeSession.fileName}”的本地会话文件。原始 Excel 或 CSV 不会被删除。`}
          confirmLabel="确认删除"
          onCancel={() => setConfirmDelete(false)}
          onConfirm={deleteCurrentSession}
        />
      )}

      {busy && busy !== "boot" && busy !== "import" && <div className="busy-line" aria-hidden="true" />}
      {toast && <div className={`toast toast-${toast.tone}`} role="status"><Icon name={toast.tone === "error" ? "warning" : "info"} size={17} /><span>{toast.message}</span><button type="button" aria-label="关闭提示" onClick={() => setToast(null)}><Icon name="close" size={15} /></button></div>}
    </div>
  );
}

function TableSkeleton({ label = "正在加载人员结果" }: { label?: string }) {
  return <div className="table-skeleton" role="status" aria-label={label}>{Array.from({ length: 6 }, (_, index) => <span key={index} />)}</div>;
}

function ImportedRecordsTable({
  page,
  loading,
  showSensitive,
  timeScoped,
  totalPages,
  filterDraft,
  onFilterDraftChange,
  onApplyFilters,
  onClearFilters,
  activeFilterCount,
  filterMenuOpen,
  onFilterMenuToggle,
  filterMenuRef,
  filterTriggerRef,
  onPageChange,
  onPageSizeChange,
}: {
  page: ImportedRecordsPage;
  loading: boolean;
  showSensitive: boolean;
  timeScoped: boolean;
  totalPages: number;
  filterDraft: ImportedRecordsQuery;
  onFilterDraftChange: (updater: (current: ImportedRecordsQuery) => ImportedRecordsQuery) => void;
  onApplyFilters: () => void;
  onClearFilters: () => void;
  activeFilterCount: number;
  filterMenuOpen: boolean;
  onFilterMenuToggle: () => void;
  filterMenuRef: React.RefObject<HTMLDivElement | null>;
  filterTriggerRef: React.RefObject<HTMLButtonElement | null>;
  onPageChange: (page: number) => void;
  onPageSizeChange: (pageSize: number) => void;
}) {
  return (
    <section className="results-region records-region" id="records-panel" role="tabpanel" aria-labelledby="records-tab" aria-label="导入入住记录">
      <div className="result-toolbar records-toolbar">
        <div className="records-scope-note">
          <strong>{timeScoped ? "选定时间范围" : "全部有效入住"}</strong>
          <span>{timeScoped ? "按入住时间边界筛选" : "未启用时间范围筛选"}</span>
        </div>
        <div className="search-field">
          <Icon name="search" size={17} />
          <input
            aria-label="搜索导入记录"
            placeholder="搜索姓名、证件号、手机号、旅馆或户籍地"
            value={filterDraft.search}
            onChange={(event) => onFilterDraftChange((current) => ({ ...current, search: event.target.value }))}
          />
          {filterDraft.search && <button type="button" aria-label="清除搜索" onClick={() => onFilterDraftChange((current) => ({ ...current, search: "" }))}><Icon name="close" size={15} /></button>}
        </div>
        <button className="button button-primary compact" type="button" onClick={onApplyFilters}>应用筛选</button>
        <div className="toolbar-menu filter-menu" data-open={filterMenuOpen} ref={filterMenuRef}>
          <button
            className="button button-quiet compact toolbar-trigger"
            type="button"
            aria-expanded={filterMenuOpen}
            aria-controls="records-filter-popover"
            ref={filterTriggerRef}
            onClick={onFilterMenuToggle}
          ><Icon name="filter" size={16} /> 更多筛选{activeFilterCount > 0 && <span className="filter-count">{activeFilterCount}</span>}</button>
          {filterMenuOpen && <div className="toolbar-popover filter-popover" id="records-filter-popover">
            <section className="filter-group" aria-labelledby="records-hotel-filter-title">
              <div className="filter-group-heading"><strong id="records-hotel-filter-title">入住旅馆</strong><span>多个名称用逗号分隔，需全部命中</span></div>
              <label className="field filter-wide-field"><span>旅馆名称</span><input placeholder="例如：旅馆 A，旅馆 B" value={filterDraft.hotelSearch ?? ""} onChange={(event) => onFilterDraftChange((current) => ({ ...current, hotelSearch: event.target.value }))} /></label>
              <div className="filter-field-grid three">
                <Field label="旅馆省份" value={filterDraft.hotelProvince ?? ""} onChange={(value) => onFilterDraftChange((current) => ({ ...current, hotelProvince: value }))} />
                <Field label="旅馆城市" value={filterDraft.hotelCity ?? ""} onChange={(value) => onFilterDraftChange((current) => ({ ...current, hotelCity: value }))} />
                <Field label="旅馆县区" value={filterDraft.hotelCounty ?? ""} onChange={(value) => onFilterDraftChange((current) => ({ ...current, hotelCounty: value }))} />
              </div>
            </section>
            <section className="filter-group" aria-labelledby="records-household-filter-title">
              <div className="filter-group-heading"><strong id="records-household-filter-title">人员户籍地</strong><span>包含条件与排除条件分别组合匹配</span></div>
              <div className="filter-subgroup"><span>包含户籍地</span><div className="filter-field-grid three">
                <Field label="省份" value={filterDraft.householdProvince ?? ""} onChange={(value) => onFilterDraftChange((current) => ({ ...current, householdProvince: value }))} />
                <Field label="城市" value={filterDraft.householdCity ?? ""} onChange={(value) => onFilterDraftChange((current) => ({ ...current, householdCity: value }))} />
                <Field label="县区" value={filterDraft.householdCounty ?? ""} onChange={(value) => onFilterDraftChange((current) => ({ ...current, householdCounty: value }))} />
              </div></div>
              <div className="filter-subgroup"><span>排除户籍地</span><div className="filter-field-grid three">
                <Field label="省份" value={filterDraft.excludeHouseholdProvince ?? ""} onChange={(value) => onFilterDraftChange((current) => ({ ...current, excludeHouseholdProvince: value }))} />
                <Field label="城市" value={filterDraft.excludeHouseholdCity ?? ""} onChange={(value) => onFilterDraftChange((current) => ({ ...current, excludeHouseholdCity: value }))} />
                <Field label="县区" value={filterDraft.excludeHouseholdCounty ?? ""} onChange={(value) => onFilterDraftChange((current) => ({ ...current, excludeHouseholdCounty: value }))} />
              </div></div>
            </section>
            <section className="filter-group" aria-labelledby="records-person-filter-title">
              <div className="filter-group-heading"><strong id="records-person-filter-title">人员条件</strong><span>仅筛选结果</span></div>
              <div className="filter-field-grid three">
                <NumberField label="最小年龄" value={filterDraft.minAge ?? null} onChange={(value) => onFilterDraftChange((current) => ({ ...current, minAge: value }))} />
                <NumberField label="最大年龄" value={filterDraft.maxAge ?? null} onChange={(value) => onFilterDraftChange((current) => ({ ...current, maxAge: value }))} />
                <label className="field"><span>性别</span><select value={filterDraft.gender ?? ""} onChange={(event) => onFilterDraftChange((current) => ({ ...current, gender: event.target.value as ImportedRecordsQuery["gender"] }))}><option value="">不限</option><option>男</option><option>女</option></select></label>
              </div>
            </section>
            <div className="popover-actions"><button className="text-button" type="button" onClick={onClearFilters}>清除全部筛选</button></div>
          </div>}
        </div>
      </div>
      <div className="table-frame" aria-busy={loading}>
        <table className="records-table">
          <thead><tr><th>人员</th><th>旅馆 / 房号</th><th>入住时间</th><th>退房时间</th><th>户籍地</th><th>来源</th><th>数据状态</th></tr></thead>
          <tbody>{page.items.map((record) => (
            <tr key={record.uid}>
              <td title={`${record.name} ${record.idNo} ${record.phone}`}><strong>{record.name || "未填"}</strong><small>{showSensitive ? record.idNo : maskIdentity(record.idNo)} · {showSensitive ? record.phone : maskPhone(record.phone)}</small></td>
              <td title={`${record.hotelName} ${record.address}`}><span className="primary-cell-text">{record.hotelName || "未填旅馆"}</span><small>房号 {record.roomNo || "未填"}</small></td>
              <td className="numeric" title={record.checkIn}>{record.checkIn || "未识别"}</td>
              <td className="numeric" title={record.checkOut}>{record.checkOut || "未退房"}</td>
              <td title={record.householdRegion}>{record.householdRegion || "未识别"}</td>
              <td title={record.sourceFile}>{record.sourceFile}<small>第 {record.sourceRow} 行</small></td>
              <td>{record.issues.length ? <span className="issue-tag" title={record.issues.join("；")}>{record.issues.length} 项问题</span> : <span className="record-ok">正常</span>}</td>
            </tr>
          ))}</tbody>
        </table>
        {loading && page.items.length === 0 ? <TableSkeleton label="正在加载导入记录" /> : page.items.length === 0 && <div className="no-results"><Icon name="file" size={22} /><strong>{timeScoped ? "当前选定时间范围内没有入住记录" : "当前会话没有有效入住记录"}</strong><span>{timeScoped ? "可调整分析时间范围或筛选条件后重试。" : "请检查导入文件中的入住时间字段或调整筛选条件。"}</span></div>}
      </div>
      <footer className="table-footer">
        <div className="page-summary"><span>共 {formatInteger(page.total)} 条</span><PageSizeSelect label="导入记录每页数量" unit="条" value={filterDraft.pageSize} onChange={onPageSizeChange} /></div>
        <div className="pagination">
          <button className="icon-button" type="button" aria-label="导入记录上一页" disabled={loading || page.page <= 1} onClick={() => onPageChange(page.page - 1)}><Icon name="chevronLeft" /></button>
          <span>第 {page.page} / {totalPages} 页</span>
          <button className="icon-button" type="button" aria-label="导入记录下一页" disabled={loading || page.page >= totalPages} onClick={() => onPageChange(page.page + 1)}><Icon name="chevronRight" /></button>
        </div>
      </footer>
    </section>
  );
}

function PageSizeSelect({ label, unit, value, onChange }: { label: string; unit: string; value: number; onChange: (value: number) => void }) {
  return (
    <label className="page-size-control">
      <span>每页</span>
      <select aria-label={label} value={value} onChange={(event) => onChange(Number(event.target.value))}>
        {pageSizeOptions.map((option) => <option key={option} value={option}>{option}</option>)}
      </select>
      <span>{unit}</span>
    </label>
  );
}

function DetailInspector({ detail, loading, showSensitive, maximized, selectedAlertIndex, onClose, onToggleMaximize, onSelectAlert, onClearAlertFilter }: {
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
                <div><dt>7 天最大</dt><dd>{detail.person.maxWeekCount ?? 0} 次</dd></div><div><dt>30 天最大</dt><dd>{detail.person.maxMonthCount} 次</dd></div><div><dt>365 天最大</dt><dd>{detail.person.maxYearCount} 次</dd></div>
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

function SettingsPanel({ settings, onChange, onClose, onApply, busy }: { settings: AnalysisSettings; onChange: (settings: AnalysisSettings) => void; onClose: () => void; onApply: () => void; busy: boolean }) {
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

function Field({ label, value, onChange, placeholder = "不限" }: { label: string; value: string; onChange: (value: string) => void; placeholder?: string }) {
  return <label className="field"><span>{label}</span><input value={value} placeholder={placeholder} onChange={(event) => onChange(event.target.value)} /></label>;
}

function NumberField({ label, value, onChange, required = false }: { label: string; value: number | null; onChange: (value: number | null) => void; required?: boolean }) {
  return <label className="field"><span>{label}</span><input type="number" min="0" required={required} value={value ?? ""} placeholder="不限" onChange={(event) => onChange(event.target.value === "" ? null : Number(event.target.value))} /></label>;
}

function DateTimeField({ label, value, onChange, required = false }: { label: string; value: string | null; onChange: (value: string | null) => void; required?: boolean }) {
  return <label className="field"><span>{label}</span><input type="datetime-local" required={required} value={value?.slice(0, 16) ?? ""} onChange={(event) => onChange(event.target.value ? `${event.target.value}:00` : null)} /></label>;
}

function ConfirmDialog({ title, description, confirmLabel, onCancel, onConfirm }: { title: string; description: string; confirmLabel: string; onCancel: () => void; onConfirm: () => void }) {
  return <div className="panel-backdrop confirm-backdrop"><section className="confirm-dialog" role="alertdialog" aria-modal="true" aria-labelledby="confirm-title"><span className="confirm-icon"><Icon name="trash" /></span><h2 id="confirm-title">{title}</h2><p>{description}</p><div><button className="button button-quiet" type="button" onClick={onCancel}>取消</button><button className="button button-danger" type="button" onClick={onConfirm}>{confirmLabel}</button></div></section></div>;
}

function EmptyWorkspace({ onFiles, onFolder }: { onFiles: () => void; onFolder: () => void }) {
  return <section className="empty-workspace"><div className="empty-illustration" aria-hidden="true"><Icon name="file" size={38}/><span><Icon name="search" size={20}/></span></div><span className="empty-kicker">第一步</span><h2>导入入住数据</h2><p>选择 Excel、CSV 文件或整个文件夹。导入后会自动清洗记录、计算风险并保留核查证据。</p><div><button className="button button-primary" type="button" onClick={onFiles}><Icon name="upload"/>选择文件</button><button className="button button-secondary" type="button" onClick={onFolder}><Icon name="folder"/>选择文件夹</button></div><small><Icon name="shield" size={15}/> 全程在本机处理，不上传文件</small></section>;
}

function LoadingShell() {
  return <div className="loading-shell"><div className="loading-top"/><div className="loading-side"><span/><span/><span/><span/></div><div className="loading-main"><span className="loading-title"/><span className="loading-subtitle"/><div className="loading-stats"><i/><i/><i/><i/><i/></div><div className="loading-table"><i/><i/><i/><i/><i/></div></div></div>;
}

function modeLabel(mode: WorkspaceSnapshot["mode"]): string {
  if (mode === "demo") return "演示数据";
  if (mode === "combined") return "合并分析";
  if (mode === "session") return "历史会话";
  return "空工作区";
}

function frequencyScopeLabel(settings: AnalysisSettings): string {
  if (settings.frequencyMode === "selected") return `选定范围 ≥ ${settings.frequencyThreshold} 次`;
  return `7/30/365 天：${settings.weekThreshold}/${settings.monthThreshold}/${settings.yearThreshold} 次`;
}

function analysisTimeScopeLabel(settings: AnalysisSettings): string {
  if (settings.frequencyMode !== "selected") return "全部有效入住";
  const start = settings.frequencyStart?.replace("T", " ") ?? "未设置";
  const end = settings.frequencyEnd?.replace("T", " ") ?? "未设置";
  return `${start} 至 ${end}`;
}

function activeExtraFilterCount(query: PersonQuery): number {
  const hasAny = (values: Array<string | undefined>) => values.some((value) => Boolean(value?.trim()));
  return Number(Boolean(query.hotelSearch?.trim()))
    + Number(hasAny([query.hotelProvince, query.hotelCity, query.hotelCounty]))
    + Number(hasAny([query.householdProvince, query.householdCity, query.householdCounty]))
    + Number(hasAny([query.excludeHouseholdProvince, query.excludeHouseholdCity, query.excludeHouseholdCounty]))
    + Number(query.minAge != null || query.maxAge != null || Boolean(query.gender))
    + Number((query.alertState ?? "全部人员") !== "全部人员");
}

function activeRecordsFilterCount(query: ImportedRecordsQuery): number {
  const hasAny = (values: Array<string | undefined>) => values.some((value) => Boolean(value?.trim()));
  return Number(Boolean(query.hotelSearch?.trim()))
    + Number(hasAny([query.hotelProvince, query.hotelCity, query.hotelCounty]))
    + Number(hasAny([query.householdProvince, query.householdCity, query.householdCounty]))
    + Number(hasAny([query.excludeHouseholdProvince, query.excludeHouseholdCity, query.excludeHouseholdCounty]))
    + Number(query.minAge != null || query.maxAge != null || Boolean(query.gender));
}

function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  if (error && typeof error === "object" && "message" in error && typeof error.message === "string") {
    return error.message;
  }
  return "操作未完成，请重试。";
}

export default App;
