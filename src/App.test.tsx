// @vitest-environment jsdom

import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import App from "./App";

describe("App", () => {
  it("loads the browser preview into the analysis workspace", async () => {
    render(<App />);
    expect(await screen.findByRole("heading", { name: "7月上旬旅馆数据" }, { timeout: 2_000 })).toBeTruthy();
    expect(screen.getByRole("table")).toBeTruthy();
    expect(screen.getByText("浏览器演示模式")).toBeTruthy();
    expect(screen.getByRole("button", { name: /查看 周明远 详情/ })).toBeTruthy();
  });

  it("keeps secondary filters and export formats behind clear entry points", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "7月上旬旅馆数据" }, { timeout: 2_000 });

    const filterTrigger = screen.getByText("更多筛选");
    expect(filterTrigger.closest("details")?.open).toBe(false);
    fireEvent.click(filterTrigger);
    expect(filterTrigger.closest("details")?.open).toBe(true);
    expect(screen.getByText("预警状态")).toBeTruthy();
    expect(screen.getByPlaceholderText("支持模糊搜索")).toBeTruthy();

    fireEvent.click(screen.getByText("导出"));
    expect(screen.getByRole("button", { name: /人员汇总 CSV/ })).toBeTruthy();
    expect(screen.getByRole("button", { name: /风险合并 Excel/ })).toBeTruthy();
  });
});
