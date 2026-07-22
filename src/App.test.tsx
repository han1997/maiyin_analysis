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
    expect(await screen.findByRole("button", { name: /查看 周明远 详情/ })).toBeTruthy();
  });

  it("keeps secondary filters and export formats behind clear entry points", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "7月上旬旅馆数据" }, { timeout: 2_000 });

    const filterTrigger = screen.getByRole("button", { name: /更多筛选/ });
    expect(filterTrigger.getAttribute("aria-expanded")).toBe("false");
    fireEvent.click(filterTrigger);
    expect(filterTrigger.getAttribute("aria-expanded")).toBe("true");
    expect(screen.getByText("预警状态")).toBeTruthy();
    expect(screen.getByPlaceholderText("例如：旅馆 A，旅馆 B")).toBeTruthy();
    expect(screen.getByText("包含户籍地")).toBeTruthy();
    expect(screen.getByText("最小年龄")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "导出" }));
    expect(filterTrigger.getAttribute("aria-expanded")).toBe("false");
    expect(screen.getByRole("button", { name: /人员汇总 CSV/ })).toBeTruthy();
    expect(screen.getByRole("button", { name: /风险合并 Excel/ })).toBeTruthy();

    fireEvent.pointerDown(document.body);
    expect(screen.queryByRole("button", { name: /人员汇总 CSV/ })).toBeNull();

    fireEvent.click(filterTrigger);
    fireEvent.keyDown(document, { key: "Escape" });
    expect(filterTrigger.getAttribute("aria-expanded")).toBe("false");
  });

  it("loads imported records by page and exposes accessible view tabs", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "7月上旬旅馆数据" }, { timeout: 2_000 });

    const peopleTab = screen.getByRole("tab", { name: /人员研判/ });
    const recordsTab = screen.getByRole("tab", { name: /导入记录/ });
    expect(peopleTab.getAttribute("aria-selected")).toBe("true");
    expect(recordsTab.getAttribute("aria-selected")).toBe("false");

    fireEvent.click(recordsTab);
    expect(recordsTab.getAttribute("aria-selected")).toBe("true");
    expect(await screen.findByText("演示人员001")).toBeTruthy();
    expect((screen.getByLabelText("导入记录每页数量") as HTMLSelectElement).value).toBe("50");

    fireEvent.click(screen.getByRole("button", { name: "导入记录下一页" }));
    expect(await screen.findByText("演示人员051")).toBeTruthy();
  });

  it("applies multi-hotel result filters without reopening analysis settings", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "7月上旬旅馆数据" }, { timeout: 2_000 });

    fireEvent.click(screen.getByText("更多筛选"));
    fireEvent.change(screen.getByPlaceholderText("例如：旅馆 A，旅馆 B"), { target: { value: "阊江，牯牛降" } });
    fireEvent.click(screen.getByRole("button", { name: "应用筛选" }));

    expect(await screen.findByRole("button", { name: /查看 周明远 详情/ })).toBeTruthy();
    expect(screen.queryByRole("button", { name: /查看 林婉清 详情/ })).toBeNull();
  });

  it("keeps only time and frequency controls in analysis settings", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "7月上旬旅馆数据" }, { timeout: 2_000 });

    fireEvent.click(screen.getByRole("button", { name: /调整分析参数/ }));
    const settings = screen.getByRole("dialog", { name: "分析参数" });
    expect(within(settings).getByText("选定入住时间范围")).toBeTruthy();
    expect((within(settings).getByRole("radio", { name: /高频入住阈值/ }) as HTMLInputElement).checked).toBe(true);
    expect((within(settings).getByLabelText("开始时间").closest("fieldset") as HTMLFieldSetElement).disabled).toBe(true);
    expect(within(settings).queryByText("入住旅馆辖区")).toBeNull();
    expect(within(settings).queryByText("人员条件")).toBeNull();
  });

  it("requires both boundaries for selected-time analysis", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "7月上旬旅馆数据" }, { timeout: 2_000 });

    fireEvent.click(screen.getByRole("button", { name: /调整分析参数/ }));
    const settings = screen.getByRole("dialog", { name: "分析参数" });
    fireEvent.click(within(settings).getByRole("radio", { name: /选定入住时间范围/ }));
    expect((within(settings).getByLabelText("开始时间").closest("fieldset") as HTMLFieldSetElement).disabled).toBe(false);
    fireEvent.click(within(settings).getByRole("button", { name: "应用参数并重新分析" }));
    expect(screen.getByText("选定入住时间范围时，开始时间和结束时间均为必填。")).toBeTruthy();
  });

  it("changes page sizes and keeps frequency columns compact", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "7月上旬旅馆数据" }, { timeout: 2_000 });

    const peoplePageSize = screen.getByLabelText("人员每页数量");
    expect((peoplePageSize as HTMLSelectElement).value).toBe("50");
    fireEvent.change(peoplePageSize, { target: { value: "100" } });
    expect((peoplePageSize as HTMLSelectElement).value).toBe("100");
    expect(screen.getByRole("columnheader", { name: "365 天" }).classList.contains("people-col-frequency")).toBe(true);

    fireEvent.click(screen.getByRole("tab", { name: /导入记录/ }));
    await screen.findByText("演示人员001");
    const recordsPageSize = screen.getByLabelText("导入记录每页数量");
    fireEvent.change(recordsPageSize, { target: { value: "200" } });
    expect((recordsPageSize as HTMLSelectElement).value).toBe("200");
    expect(await screen.findByText("第 1 / 7 页")).toBeTruthy();
  });

  it("rejects an inverted result age range without applying it", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "7月上旬旅馆数据" }, { timeout: 2_000 });

    fireEvent.click(screen.getByText("更多筛选"));
    fireEvent.change(screen.getByLabelText("最小年龄"), { target: { value: "40" } });
    fireEvent.change(screen.getByLabelText("最大年龄"), { target: { value: "20" } });
    fireEvent.click(screen.getByRole("button", { name: "应用筛选" }));

    expect(screen.getByText("最小年龄不能大于最大年龄。")).toBeTruthy();
    expect(await screen.findByRole("button", { name: /查看 周明远 详情/ })).toBeTruthy();
  });
});
