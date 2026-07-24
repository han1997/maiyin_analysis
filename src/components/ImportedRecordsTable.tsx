import type React from "react";
import { Icon } from "./Icon";
import { Field } from "./Field";
import { NumberField } from "./NumberField";
import { TableSkeleton } from "./TableSkeleton";
import { PageSizeSelect } from "./PageSizeSelect";
import type { ImportedRecordsPage, ImportedRecordsQuery } from "../domain/types";
import { formatInteger, maskIdentity, maskPhone } from "../lib/format";
import { regionFilterPlaceholder } from "../lib/appHelpers";

export function ImportedRecordsTable({
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
              <div className="filter-group-heading"><strong id="records-hotel-filter-title">入住旅馆</strong><span>名称多项需全部命中；省市县支持模糊多选</span></div>
              <label className="field filter-wide-field"><span>旅馆名称</span><input placeholder="例如：旅馆 A，旅馆 B" value={filterDraft.hotelSearch ?? ""} onChange={(event) => onFilterDraftChange((current) => ({ ...current, hotelSearch: event.target.value }))} /></label>
              <div className="filter-field-grid three">
                <Field label="旅馆省份" value={filterDraft.hotelProvince ?? ""} placeholder={regionFilterPlaceholder} onChange={(value) => onFilterDraftChange((current) => ({ ...current, hotelProvince: value }))} />
                <Field label="旅馆城市" value={filterDraft.hotelCity ?? ""} placeholder={regionFilterPlaceholder} onChange={(value) => onFilterDraftChange((current) => ({ ...current, hotelCity: value }))} />
                <Field label="旅馆县区" value={filterDraft.hotelCounty ?? ""} placeholder={regionFilterPlaceholder} onChange={(value) => onFilterDraftChange((current) => ({ ...current, hotelCounty: value }))} />
              </div>
            </section>
            <section className="filter-group" aria-labelledby="records-household-filter-title">
              <div className="filter-group-heading"><strong id="records-household-filter-title">人员户籍地</strong><span>省市县支持模糊多选；包含字段间同时满足，排除任一命中即排除</span></div>
              <div className="filter-subgroup"><span>包含户籍地</span><div className="filter-field-grid three">
                <Field label="省份" value={filterDraft.householdProvince ?? ""} placeholder={regionFilterPlaceholder} onChange={(value) => onFilterDraftChange((current) => ({ ...current, householdProvince: value }))} />
                <Field label="城市" value={filterDraft.householdCity ?? ""} placeholder={regionFilterPlaceholder} onChange={(value) => onFilterDraftChange((current) => ({ ...current, householdCity: value }))} />
                <Field label="县区" value={filterDraft.householdCounty ?? ""} placeholder={regionFilterPlaceholder} onChange={(value) => onFilterDraftChange((current) => ({ ...current, householdCounty: value }))} />
              </div></div>
              <div className="filter-subgroup"><span>排除户籍地</span><div className="filter-field-grid three">
                <Field label="省份" value={filterDraft.excludeHouseholdProvince ?? ""} placeholder={regionFilterPlaceholder} onChange={(value) => onFilterDraftChange((current) => ({ ...current, excludeHouseholdProvince: value }))} />
                <Field label="城市" value={filterDraft.excludeHouseholdCity ?? ""} placeholder={regionFilterPlaceholder} onChange={(value) => onFilterDraftChange((current) => ({ ...current, excludeHouseholdCity: value }))} />
                <Field label="县区" value={filterDraft.excludeHouseholdCounty ?? ""} placeholder={regionFilterPlaceholder} onChange={(value) => onFilterDraftChange((current) => ({ ...current, excludeHouseholdCounty: value }))} />
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
