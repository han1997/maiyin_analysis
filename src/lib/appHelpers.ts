import type { AnalysisSettings, ImportedRecordsQuery, PersonQuery, WorkspaceSnapshot } from "../domain/types";
import { splitFilterTerms } from "./filter";

export const regionFilterPlaceholder = "例如：安徽，浙江";

export function modeLabel(mode: WorkspaceSnapshot["mode"]): string {
  if (mode === "demo") return "演示数据";
  if (mode === "combined") return "合并分析";
  if (mode === "session") return "历史会话";
  return "空工作区";
}

export function frequencyScopeLabel(settings: AnalysisSettings): string {
  if (settings.frequencyMode === "selected") return `选定范围 ≥ ${settings.frequencyThreshold} 次`;
  return `7/30/365 天：${settings.weekThreshold}/${settings.monthThreshold}/${settings.yearThreshold} 次`;
}

export function analysisTimeScopeLabel(settings: AnalysisSettings): string {
  if (settings.frequencyMode !== "selected") return "全部有效入住";
  const start = settings.frequencyStart?.replace("T", " ") ?? "未设置";
  const end = settings.frequencyEnd?.replace("T", " ") ?? "未设置";
  return `${start} 至 ${end}`;
}

interface FilterCountQuery {
  hotelSearch?: string;
  hotelProvince?: string;
  hotelCity?: string;
  hotelCounty?: string;
  householdProvince?: string;
  householdCity?: string;
  householdCounty?: string;
  excludeHouseholdProvince?: string;
  excludeHouseholdCity?: string;
  excludeHouseholdCounty?: string;
  minAge?: number | null;
  maxAge?: number | null;
  gender?: string;
}

function activeSharedFilterCount(query: FilterCountQuery): number {
  return Number(splitFilterTerms(query.hotelSearch ?? "").length > 0)
    + activeDelimitedFieldCount([query.hotelProvince, query.hotelCity, query.hotelCounty])
    + activeDelimitedFieldCount([query.householdProvince, query.householdCity, query.householdCounty])
    + activeDelimitedFieldCount([query.excludeHouseholdProvince, query.excludeHouseholdCity, query.excludeHouseholdCounty])
    + Number(query.minAge != null || query.maxAge != null || Boolean(query.gender));
}

export function activeExtraFilterCount(query: PersonQuery): number {
  return activeSharedFilterCount(query)
    + Number((query.alertState ?? "全部人员") !== "全部人员");
}

export function activeRecordsFilterCount(query: ImportedRecordsQuery): number {
  return activeSharedFilterCount(query);
}

export function activeDelimitedFieldCount(values: Array<string | undefined>): number {
  return values.reduce(
    (count, value) => count + Number(splitFilterTerms(value ?? "").length > 0),
    0,
  );
}

export function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  if (error && typeof error === "object" && "message" in error && typeof error.message === "string") {
    return error.message;
  }
  return "操作未完成，请重试。";
}
