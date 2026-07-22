import { demoPeople, demoSnapshot, getDemoDetail } from "../data/demo";
import { DEFAULT_SETTINGS, type AnalysisSettings, type ExportKind, type ImportedStayRecord, type WorkspaceSnapshot } from "../domain/types";
import { filterPeople } from "../lib/filter";
import type { AppApi } from "./contract";

const pause = (duration = 320) => new Promise((resolve) => window.setTimeout(resolve, duration));

function cloneSnapshot(snapshot: WorkspaceSnapshot): WorkspaceSnapshot {
  return structuredClone(snapshot);
}

function emptySnapshot(): WorkspaceSnapshot {
  return {
    mode: "empty",
    title: "尚未载入数据",
    subtitle: "选择 Excel、CSV 或历史会话开始分析",
    stats: { records: 0, people: 0, alerted: 0, high: 0, issues: 0 },
    sessions: cloneSnapshot(demoSnapshot).sessions.map((session) => ({ ...session, active: false })),
    settings: { ...DEFAULT_SETTINGS },
    importStats: { imported: 0, duplicateCount: 0, shortStayCount: 0, missingIdCount: 0 },
    sourceSessionIds: [],
    generatedAt: new Date().toISOString(),
  };
}

function chooseBrowserFiles(directory: boolean): Promise<FileList | null> {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.multiple = true;
    input.accept = ".xls,.xlsx,.csv";
    if (directory) input.setAttribute("webkitdirectory", "");
    input.addEventListener("change", () => resolve(input.files), { once: true });
    input.addEventListener("cancel", () => resolve(null), { once: true });
    input.click();
  });
}

function demoImportedRecord(index: number): ImportedStayRecord {
  const day = String((index % 15) + 1).padStart(2, "0");
  const hour = String(8 + (index % 14)).padStart(2, "0");
  return {
    uid: index + 1,
    sourceFile: `演示入住数据_${Math.floor(index / 400) + 1}.xlsx`,
    sourceRow: index + 2,
    name: `演示人员${String((index % 96) + 1).padStart(3, "0")}`,
    idNo: `341024198809${String(index % 100_000).padStart(5, "0")}`,
    phone: `139${String(index % 100_000_000).padStart(8, "0")}`,
    householdRegion: "安徽省 黄山市 祁门县",
    hotelName: index % 2 === 0 ? "阊江商务酒店" : "牯牛降宾馆",
    region: "安徽省 黄山市 祁门县",
    address: "演示路 18 号",
    roomNo: String(201 + (index % 30)),
    checkIn: `2026-07-${day} ${hour}:20`,
    registerTime: `2026-07-${day} ${hour}:22`,
    checkOut: `2026-07-${day} 23:40`,
    issues: index % 17 === 0 ? ["演示数据问题"] : [],
  };
}

export const browserApi: AppApi = {
  runtime: "browser",

  async bootstrap() {
    await pause(220);
    return cloneSnapshot(demoSnapshot);
  },

  async importFiles() {
    const files = await chooseBrowserFiles(false);
    if (!files?.length) return null;
    await pause(720);
    const next = cloneSnapshot(demoSnapshot);
    next.title = files.length === 1 ? files[0]?.name ?? "已选文件" : `${files.length} 个本地文件`;
    next.subtitle = "浏览器演示模式仅展示交互，文件内容没有上传或解析";
    next.generatedAt = new Date().toISOString();
    return next;
  },

  async importFolder() {
    const files = await chooseBrowserFiles(true);
    if (!files?.length) return null;
    await pause(720);
    const next = cloneSnapshot(demoSnapshot);
    next.title = `演示文件夹（${files.length} 个候选文件）`;
    next.subtitle = "安装 Rust 后通过 Tauri 递归读取并解析文件夹";
    next.generatedAt = new Date().toISOString();
    return next;
  },

  async loadSession(sessionId) {
    await pause();
    const next = cloneSnapshot(demoSnapshot);
    next.sessions = next.sessions.map((session) => ({ ...session, active: session.sessionId === sessionId }));
    const active = next.sessions.find((session) => session.sessionId === sessionId);
    if (active) {
      next.title = active.fileName;
      next.subtitle = `${active.fileCount} 个文件 · 历史演示会话`;
      next.sourceSessionIds = [active.sessionId];
    }
    return next;
  },

  async mergeSessions(sessionIds) {
    await pause(520);
    const next = cloneSnapshot(demoSnapshot);
    next.mode = "combined";
    next.title = `合并分析 · ${sessionIds.length} 个历史会话`;
    next.subtitle = "已跨会话去重，并按当前参数重新计算风险（演示）";
    next.sourceSessionIds = sessionIds;
    next.stats.records = Math.round(next.stats.records * Math.max(1.35, sessionIds.length * 0.82));
    next.generatedAt = new Date().toISOString();
    return next;
  },

  async deleteSession(sessionId) {
    await pause();
    const next = emptySnapshot();
    next.sessions = next.sessions.filter((session) => session.sessionId !== sessionId);
    return next;
  },

  async clearWorkspace() {
    await pause(180);
    return emptySnapshot();
  },

  async reanalyze(settings: AnalysisSettings) {
    await pause(640);
    const next = cloneSnapshot(demoSnapshot);
    next.settings = structuredClone(settings);
    next.subtitle = "已按当前分析参数重新计算（浏览器演示）";
    next.generatedAt = new Date().toISOString();
    return next;
  },

  async queryPeople(query) {
    await pause(80);
    return filterPeople(demoPeople, query);
  },

  async getPersonDetail(personKey) {
    await pause(180);
    return structuredClone(getDemoDetail(personKey));
  },

  async getImportedRecords(query) {
    await pause(180);
    const pageSize = Math.min(500, Math.max(1, query.pageSize));
    const page = Math.max(1, query.page);
    const total = demoSnapshot.stats.records;
    const start = (page - 1) * pageSize;
    const end = Math.min(total, start + pageSize);
    return {
      items: Array.from({ length: Math.max(0, end - start) }, (_, offset) => demoImportedRecord(start + offset)),
      total,
      page,
      pageSize,
    };
  },

  async exportResult(kind: ExportKind) {
    await pause(480);
    const labels: Record<ExportKind, string> = {
      summary_csv: "人员汇总 CSV",
      risk_xlsx: "风险合并 Excel",
      raw_csv: "规范化原始 CSV",
      template_xlsx: "导入模板",
    };
    return { message: `${labels[kind]}将在 Tauri 模式中写入本地文件；当前为浏览器演示。` };
  },

  async chooseStorageDirectory() {
    await pause(180);
    return { message: "存放目录只能在 Tauri 桌面模式中修改。" };
  },
};
