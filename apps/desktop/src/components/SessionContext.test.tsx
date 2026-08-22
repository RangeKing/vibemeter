// @vitest-environment jsdom

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { I18nextProvider } from "react-i18next";
import i18n from "../i18n";
import type { SessionContext as SessionContextData } from "../types";
import { SessionContext } from "./SessionContext";

beforeAll(async () => {
  await i18n.changeLanguage("zh-CN");
});

afterEach(cleanup);

const context: SessionContextData = {
  status: "ready",
  summary: {
    totalTokens: 160,
    inputTokens: 100,
    outputTokens: 40,
    cacheTokens: 20,
    reasoningTokens: 0,
    compactions: 1,
    eventCount: 3,
    coverage: "observed",
  },
  composition: [
    { key: "input", tokens: 100, coverage: "observed" },
    { key: "cache", tokens: 20, coverage: "observed" },
    { key: "output", tokens: 40, coverage: "observed" },
  ],
  events: [
    {
      id: "event-3",
      sequence: 3,
      occurredAt: "2026-08-14T10:00:03Z",
      kind: "tool",
      name: "完成修改",
      phase: "edit",
      success: true,
      coverage: "observed",
    },
    {
      id: "event-2",
      sequence: 2,
      occurredAt: "2026-08-14T10:00:02Z",
      kind: "input",
      name: "用户消息",
      phase: "understand",
      success: true,
      coverage: "observed",
    },
  ],
  hasMore: true,
  nextOffset: 2,
  browser: {
    categories: [
      { key: "system", items: [], coverage: "not-recorded" },
      { key: "tools", items: [], coverage: "not-recorded" },
      {
        key: "user",
        coverage: "observed",
        items: [{ id: "prompt", label: "用户消息", text: "请完成这个改动", count: 1, coverage: "observed" }],
      },
      { key: "injected", items: [], coverage: "not-recorded" },
      { key: "assistant", items: [], coverage: "not-recorded" },
      { key: "tool_use", items: [{ id: "bash", label: "Bash", count: 3, coverage: "observed" }], coverage: "observed" },
      { key: "tool_result", items: [], coverage: "not-recorded" },
    ],
  },
};

function renderContext(overrides: Partial<Parameters<typeof SessionContext>[0]> = {}) {
  return render(
    <I18nextProvider i18n={i18n}>
      <SessionContext
        context={context}
        locale="zh-CN"
        isLoading={false}
        isError={false}
        onRetry={vi.fn()}
        {...overrides}
      />
    </I18nextProvider>,
  );
}

describe("SessionContext", () => {
  it("shows coverage, the full horizontal structure, and merged browser counts", () => {
    const view = renderContext();

    expect(screen.getByRole("heading", { name: "会话上下文" })).toBeTruthy();
    expect(screen.getAllByText("已观测").length).toBeGreaterThan(0);
    expect(screen.getByText("上下文结构")).toBeTruthy();
    expect(view.container.querySelector('[title="输入: 100"]')).toBeTruthy();
    expect(view.container.querySelector('[title="用户消息: 100"]')).toBeNull();
    expect(view.container.querySelector(".context-usage-grid")).toBeTruthy();
    expect(view.container.querySelector(".context-usage-item.usage-input b")?.textContent).toBe("100");
    expect(view.container.querySelector(".context-usage-item.usage-reasoning b")?.textContent).toBe("0");
    expect([...view.container.querySelectorAll(".context-usage-item")].map((item) => [...item.classList].find((name) => name.startsWith("usage-")))).toEqual([
      "usage-input",
      "usage-output",
      "usage-cache",
      "usage-reasoning",
    ]);
    expect(screen.getAllByText("系统提示词").length).toBeGreaterThan(0);
    expect(screen.getAllByText("工具定义").length).toBeGreaterThan(0);
    expect(screen.getAllByText("用户消息").length).toBeGreaterThan(0);
    expect(screen.getAllByText("注入内容").length).toBeGreaterThan(0);
    expect(screen.getAllByText("助手消息").length).toBeGreaterThan(0);
    expect(screen.getAllByText("工具调用").length).toBeGreaterThan(0);
    expect(screen.getAllByText("工具结果").length).toBeGreaterThan(0);
    expect(screen.getByText("请完成这个改动")).toBeTruthy();
    expect(screen.getByText("×3")).toBeTruthy();
    expect(screen.queryByText("上下文时间线")).toBeNull();
  });

  it("uses the same structure token values for the chart and the category cards", () => {
    const tokenContext: SessionContextData = {
      ...context,
      browser: {
        categories: context.browser.categories.map((category) => {
          if (category.key === "user") {
            return { ...category, items: category.items.map((item) => ({ ...item, tokens: 60 })) };
          }
          if (category.key === "tool_use") {
            return { ...category, items: category.items.map((item) => ({ ...item, tokens: 40 })) };
          }
          return category;
        }),
      },
    };
    const view = renderContext({ context: tokenContext });

    expect(view.container.querySelector('[title="用户消息: 60"]')).toBeTruthy();
    expect(view.container.querySelector('[title="工具调用: 40"]')).toBeTruthy();
    expect(view.container.querySelector(".context-structure-item.segment-user b")?.textContent).toBe("60");
    expect(view.container.querySelector(".context-structure-item.segment-tool_use b")?.textContent).toBe("40");
    expect(view.container.querySelector('[title="输入: 100"]')).toBeNull();
    expect(view.container.querySelector(".context-usage-grid")).toBeNull();
  });
});
