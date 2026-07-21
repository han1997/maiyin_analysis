import type {
  AnalysisSettings,
  ExportKind,
  OperationResult,
  ImportedStayRecord,
  PersonDetail,
  PersonPage,
  PersonQuery,
  WorkspaceSnapshot,
} from "../domain/types";

export interface AppApi {
  readonly runtime: "browser" | "tauri";
  bootstrap(): Promise<WorkspaceSnapshot>;
  importFiles(): Promise<WorkspaceSnapshot | null>;
  importFolder(): Promise<WorkspaceSnapshot | null>;
  loadSession(sessionId: string): Promise<WorkspaceSnapshot>;
  mergeSessions(sessionIds: string[]): Promise<WorkspaceSnapshot>;
  deleteSession(sessionId: string): Promise<WorkspaceSnapshot>;
  clearWorkspace(): Promise<WorkspaceSnapshot>;
  reanalyze(settings: AnalysisSettings): Promise<WorkspaceSnapshot>;
  queryPeople(query: PersonQuery): Promise<PersonPage>;
  getPersonDetail(personKey: string): Promise<PersonDetail>;
  getImportedRecords(): Promise<ImportedStayRecord[]>;
  exportResult(kind: ExportKind): Promise<OperationResult>;
  chooseStorageDirectory(): Promise<OperationResult | null>;
}
