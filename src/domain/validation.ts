// 须与 src-tauri/src/commands.rs::validate_settings 同步
import type { AnalysisSettings } from "./types";

export const THRESHOLD_MIN = 1;
export const THRESHOLD_MAX = 99999;

export const THRESHOLD_LABELS = {
  weekThreshold: "7 天",
  monthThreshold: "30 天",
  yearThreshold: "365 天",
  frequencyThreshold: "时间窗口",
} as const;

type ThresholdKey = "weekThreshold" | "monthThreshold" | "yearThreshold" | "frequencyThreshold";

export function validateAnalysisSettings(settings: AnalysisSettings): string | null {
  const keys: ThresholdKey[] = settings.frequencyMode === "selected"
    ? ["frequencyThreshold"]
    : ["weekThreshold", "monthThreshold", "yearThreshold"];
  for (const key of keys) {
    const value = settings[key];
    if (!(THRESHOLD_MIN <= value && value <= THRESHOLD_MAX)) {
      return `${THRESHOLD_LABELS[key]}阈值应在 1 到 99999 之间`;
    }
  }
  if (settings.frequencyMode === "selected") {
    if (!settings.frequencyStart || !settings.frequencyEnd) {
      return "选定入住时间范围时，开始时间和结束时间均为必填";
    }
    if (settings.frequencyStart > settings.frequencyEnd) {
      return "入住开始时间不能晚于结束时间";
    }
  }
  return null;
}
