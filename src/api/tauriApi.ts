import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import type { AnalysisSettings, ExportKind, ImportedRecordsPage, ImportedRecordsQuery, OperationResult, PersonDetail, PersonPage, PersonQuery } from "../domain/types";
import type { AppApi } from "./contract";

async function selectPaths(directory: boolean): Promise<string[]> {
  const selection = await open({
    directory,
    multiple: !directory,
    filters: directory
      ? undefined
      : [{ name: "旅馆业数据", extensions: ["xls", "xlsx", "csv"] }],
  });
  if (!selection) return [];
  return Array.isArray(selection) ? selection : [selection];
}

export const tauriApi: AppApi = {
  runtime: "tauri",
  bootstrap: () => invoke("bootstrap_workspace"),

  async importFiles() {
    const paths = await selectPaths(false);
    return paths.length ? invoke("import_paths", { paths }) : null;
  },

  async importFolder() {
    const paths = await selectPaths(true);
    return paths.length ? invoke("import_folders", { paths }) : null;
  },

  loadSession: (sessionId) => invoke("load_session", { sessionId }),
  mergeSessions: (sessionIds) => invoke("merge_sessions", { sessionIds }),
  deleteSession: (sessionId) => invoke("delete_session", { sessionId }),
  clearWorkspace: () => invoke("clear_workspace"),
  reanalyze: (settings: AnalysisSettings) => invoke("reanalyze", { settings }),
  queryPeople: (query: PersonQuery): Promise<PersonPage> => invoke("query_people", { query }),
  getPersonDetail: (personKey): Promise<PersonDetail> => invoke("get_person_detail", { personKey }),
  getImportedRecords: (query: ImportedRecordsQuery): Promise<ImportedRecordsPage> => invoke("get_imported_records", { query }),
  async exportResult(kind: ExportKind): Promise<OperationResult> {
    const definitions: Record<ExportKind, { defaultPath: string; name: string; extensions: string[] }> = {
      summary_csv: { defaultPath: "人员汇总.csv", name: "CSV 文件", extensions: ["csv"] },
      risk_xlsx: { defaultPath: "风险合并.xlsx", name: "Excel 工作簿", extensions: ["xlsx"] },
      raw_csv: { defaultPath: "规范化原始明细.csv", name: "CSV 文件", extensions: ["csv"] },
      template_xlsx: { defaultPath: "旅馆业数据导入模板.xlsx", name: "Excel 工作簿", extensions: ["xlsx"] },
    };
    const definition = definitions[kind];
    const path = await save({
      defaultPath: definition.defaultPath,
      filters: [{ name: definition.name, extensions: definition.extensions }],
    });
    if (!path) return { message: "已取消导出。" };
    return invoke("export_result", { kind, path });
  },

  async chooseStorageDirectory() {
    const path = await open({ directory: true, multiple: false });
    return path ? invoke("set_storage_directory", { path }) : null;
  },
};
