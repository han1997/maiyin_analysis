import type {
  AnalysisSettings,
  ExportKind,
  OperationResult,
  ImportedRecordsPage,
  ImportedRecordsQuery,
  PersonDetail,
  PersonPage,
  PersonQuery,
  Progress,
  WorkspaceSnapshot,
} from "../domain/types";

export interface AppApi {
  readonly runtime: "browser" | "tauri";
  bootstrap(): Promise<WorkspaceSnapshot>;
  importFiles(onProgress?: (p: Progress) => void): Promise<WorkspaceSnapshot | null>;
  importFolder(onProgress?: (p: Progress) => void): Promise<WorkspaceSnapshot | null>;
  loadSession(sessionId: string): Promise<WorkspaceSnapshot>;
  mergeSessions(sessionIds: string[], onProgress?: (p: Progress) => void): Promise<WorkspaceSnapshot>;
  deleteSession(sessionId: string): Promise<WorkspaceSnapshot>;
  clearWorkspace(): Promise<WorkspaceSnapshot>;
  reanalyze(settings: AnalysisSettings, onProgress?: (p: Progress) => void): Promise<WorkspaceSnapshot>;
  queryPeople(query: PersonQuery): Promise<PersonPage>;
  getPersonDetail(personKey: string): Promise<PersonDetail>;
  getImportedRecords(query: ImportedRecordsQuery): Promise<ImportedRecordsPage>;
  exportResult(kind: ExportKind): Promise<OperationResult>;
  chooseStorageDirectory(): Promise<OperationResult | null>;
}
