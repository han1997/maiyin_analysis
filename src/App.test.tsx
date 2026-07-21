// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import App from "./App";

afterEach(cleanup);

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
    expect(screen.getByPlaceholderText("例如：旅馆 A，旅馆 B")).toBeTruthy();
    expect(screen.getByText("包含户籍地")).toBeTruthy();
    expect(screen.getByText("最小年龄")).toBeTruthy();

    fireEvent.click(screen.getByText("导出"));
    expect(screen.getByRole("button", { name: /人员汇总 CSV/ })).toBeTruthy();
    expect(screen.getByRole("button", { name: /风险合并 Excel/ })).toBeTruthy();
  });

  it("applies multi-hotel result filters without reopening analysis settings", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "7月上旬旅馆数据" }, { timeout: 2_000 });

    fireEvent.click(screen.getByText("更多筛选"));
    fireEvent.change(screen.getByPlaceholderText("例如：旅馆 A，旅馆 B"), { target: { value: "阊江，牯牛降" } });
    fireEvent.click(screen.getByRole("button", { name: "应用筛选" }));

    expect(screen.getByRole("button", { name: /查看 周明远 详情/ })).toBeTruthy();
    expect(screen.queryByRole("button", { name: /查看 林婉清 详情/ })).toBeNull();
  });

  it("keeps only time and frequency controls in analysis settings", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "7月上旬旅馆数据" }, { timeout: 2_000 });

    fireEvent.click(screen.getByRole("button", { name: /调整分析参数/ }));
    const settings = screen.getByRole("dialog", { name: "分析参数" });
    expect(within(settings).getByText("选定入住时间范围")).toBeTruthy();
    expect(within(settings).queryByText("入住旅馆辖区")).toBeNull();
    expect(within(settings).queryByText("人员条件")).toBeNull();
  });

  it("rejects an inverted result age range without applying it", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "7月上旬旅馆数据" }, { timeout: 2_000 });

    fireEvent.click(screen.getByText("更多筛选"));
    fireEvent.change(screen.getByLabelText("最小年龄"), { target: { value: "40" } });
    fireEvent.change(screen.getByLabelText("最大年龄"), { target: { value: "20" } });
    fireEvent.click(screen.getByRole("button", { name: "应用筛选" }));

    expect(screen.getByText("最小年龄不能大于最大年龄。")).toBeTruthy();
    expect(screen.getByRole("button", { name: /查看 周明远 详情/ })).toBeTruthy();
  });
});
