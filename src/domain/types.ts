export type RiskLevel = "高风险" | "中风险" | "关注" | "正常";
export type Severity = "高" | "中" | "低";

export interface AnalysisSettings {
  frequencyStart: string | null;
  frequencyEnd: string | null;
  frequencyThreshold: number;
  weekThreshold: number;
  monthThreshold: number;
  yearThreshold: number;
}

export interface AnalysisStats {
  records: number;
  people: number;
  alerted: number;
  high: number;
  issues: number;
}

export interface AlertSummary {
  kind: "overlap" | "same_day_many" | "window_frequency" | "week_frequency" | "month_frequency" | "year_frequency";
  severity: Severity;
  score: number;
  title: string;
  detail: string;
  evidenceCount: number;
}

export interface PersonSummary {
  personKey: string;
  name: string;
  idNo: string;
  phone: string;
  householdRegion: string;
  age: number | null;
  gender: string;
  totalRecords: number;
  maxWeekCount?: number;
  maxMonthCount: number;
  maxYearCount: number;
  overlapDays: number;
  sequentialDays: number;
  score: number;
  level: RiskLevel;
  alertCount: number;
  alertTitles: string[];
  hotelNames?: string[];
  hotelRegions?: HotelRegion[];
}

export interface HotelRegion {
  province: string;
  city: string;
  county: string;
  region: string;
}

export interface EvidenceRecord {
  uid: number;
  sourceFile: string;
  sourceRow: number;
  hotelName: string;
  region: string;
  address: string;
  roomNo: string;
  checkIn: string;
  checkOut: string;
  issues: string[];
}

export interface ImportedStayRecord {
  uid: number;
  sourceFile: string;
  sourceRow: number;
  name: string;
  idNo: string;
  phone: string;
  householdRegion: string;
  hotelName: string;
  region: string;
  address: string;
  roomNo: string;
  checkIn: string;
  registerTime: string;
  checkOut: string;
  issues: string[];
}

export interface PersonDetail {
  person: PersonSummary;
  alerts: AlertSummary[];
  evidence: EvidenceRecord[];
}

export interface SessionSummary {
  sessionId: string;
  fileName: string;
  importedAt: string;
  fileCount: number;
  records: number;
  people: number;
  duplicateCount: number;
  shortStayCount: number;
  active: boolean;
}

export interface ImportStats {
  imported: number;
  duplicateCount: number;
  shortStayCount: number;
  missingIdCount: number;
}

export interface WorkspaceSnapshot {
  mode: "empty" | "demo" | "session" | "combined";
  title: string;
  subtitle: string;
  stats: AnalysisStats;
  sessions: SessionSummary[];
  settings: AnalysisSettings;
  importStats: ImportStats;
  sourceSessionIds: string[];
  generatedAt: string;
}

export interface PersonQuery {
  search: string;
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
  gender?: "" | "男" | "女";
  level: "全部等级" | RiskLevel;
  alertState: "全部人员" | "仅预警人员" | "未预警人员";
  page: number;
  pageSize: number;
}

export interface PersonPage {
  items: PersonSummary[];
  total: number;
  page: number;
  pageSize: number;
}

export type ExportKind = "summary_csv" | "risk_xlsx" | "raw_csv" | "template_xlsx";

export interface OperationResult {
  message: string;
  path?: string;
}

export const DEFAULT_SETTINGS: AnalysisSettings = {
  frequencyStart: null,
  frequencyEnd: null,
  frequencyThreshold: 3,
  weekThreshold: 3,
  monthThreshold: 12,
  yearThreshold: 144,
};
