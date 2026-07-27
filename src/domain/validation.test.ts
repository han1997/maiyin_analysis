import { describe, expect, it } from "vitest";
import { DEFAULT_SETTINGS } from "./types";
import { THRESHOLD_MAX, THRESHOLD_MIN, validateAnalysisSettings } from "./validation";

describe("validateAnalysisSettings", () => {
  it("accepts default rolling settings", () => {
    expect(validateAnalysisSettings(DEFAULT_SETTINGS)).toBeNull();
  });

  it("rejects a week threshold below the minimum", () => {
    expect(validateAnalysisSettings({ ...DEFAULT_SETTINGS, weekThreshold: 0 })).toBe(
      "7 天阈值应在 1 到 99999 之间",
    );
  });

  it("rejects a week threshold above the maximum", () => {
    expect(
      validateAnalysisSettings({ ...DEFAULT_SETTINGS, weekThreshold: THRESHOLD_MAX + 1 }),
    ).toBe("7 天阈值应在 1 到 99999 之间");
  });

  it("accepts thresholds at the inclusive boundaries", () => {
    expect(
      validateAnalysisSettings({
        ...DEFAULT_SETTINGS,
        weekThreshold: THRESHOLD_MIN,
        monthThreshold: THRESHOLD_MAX,
        yearThreshold: THRESHOLD_MIN,
      }),
    ).toBeNull();
  });

  it("rejects each rolling threshold with its own label", () => {
    expect(validateAnalysisSettings({ ...DEFAULT_SETTINGS, monthThreshold: 0 })).toBe(
      "30 天阈值应在 1 到 99999 之间",
    );
    expect(validateAnalysisSettings({ ...DEFAULT_SETTINGS, yearThreshold: 100000 })).toBe(
      "365 天阈值应在 1 到 99999 之间",
    );
  });

  it("rejects selected mode without time boundaries", () => {
    expect(
      validateAnalysisSettings({
        ...DEFAULT_SETTINGS,
        frequencyMode: "selected",
        frequencyStart: null,
        frequencyEnd: null,
      }),
    ).toBe("选定入住时间范围时，开始时间和结束时间均为必填");
  });

  it("rejects selected mode with only one boundary", () => {
    expect(
      validateAnalysisSettings({
        ...DEFAULT_SETTINGS,
        frequencyMode: "selected",
        frequencyStart: "2026-07-01T00:00",
        frequencyEnd: null,
      }),
    ).toBe("选定入住时间范围时，开始时间和结束时间均为必填");
  });

  it("rejects selected mode when start is after end", () => {
    expect(
      validateAnalysisSettings({
        ...DEFAULT_SETTINGS,
        frequencyMode: "selected",
        frequencyStart: "2026-07-02T00:00",
        frequencyEnd: "2026-07-01T00:00",
      }),
    ).toBe("入住开始时间不能晚于结束时间");
  });

  it("accepts selected mode with valid boundaries and threshold", () => {
    expect(
      validateAnalysisSettings({
        ...DEFAULT_SETTINGS,
        frequencyMode: "selected",
        frequencyStart: "2026-07-01T00:00",
        frequencyEnd: "2026-07-02T00:00",
        frequencyThreshold: 3,
      }),
    ).toBeNull();
  });

  it("rejects the frequency threshold out of range in selected mode", () => {
    expect(
      validateAnalysisSettings({
        ...DEFAULT_SETTINGS,
        frequencyMode: "selected",
        frequencyStart: "2026-07-01T00:00",
        frequencyEnd: "2026-07-02T00:00",
        frequencyThreshold: 0,
      }),
    ).toBe("时间窗口阈值应在 1 到 99999 之间");
  });

  it("ignores inactive frequency threshold in rolling mode", () => {
    expect(validateAnalysisSettings({ ...DEFAULT_SETTINGS, frequencyThreshold: 0 })).toBeNull();
  });

  it("ignores inactive rolling thresholds in selected mode", () => {
    expect(
      validateAnalysisSettings({
        ...DEFAULT_SETTINGS,
        frequencyMode: "selected",
        frequencyStart: "2026-07-01T00:00",
        frequencyEnd: "2026-07-02T00:00",
        frequencyThreshold: 3,
        weekThreshold: 0,
        monthThreshold: 0,
        yearThreshold: 0,
      }),
    ).toBeNull();
  });

  it("does not append a trailing period to the selected-boundary message", () => {
    const message = validateAnalysisSettings({
      ...DEFAULT_SETTINGS,
      frequencyMode: "selected",
      frequencyStart: null,
      frequencyEnd: null,
    });
    expect(message).not.toMatch(/。$/);
  });

  it("does not append a trailing period to the start-after-end message", () => {
    const message = validateAnalysisSettings({
      ...DEFAULT_SETTINGS,
      frequencyMode: "selected",
      frequencyStart: "2026-07-02T00:00",
      frequencyEnd: "2026-07-01T00:00",
    });
    expect(message).not.toMatch(/。$/);
  });
});
