// @vitest-environment jsdom

import { render, screen } from "@testing-library/react";
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
});

