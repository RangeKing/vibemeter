// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
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
        items: [{ id: "prompt", label: "用户消息", text: "请完成这个改动", coverage: "observed" }],
      },
      { key: "inject", items: [], coverage: "not-recorded" },
      { key: "assistant", items: [], coverage: "not-recorded" },
      { key: "tool", items: [], coverage: "not-recorded" },
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
        loadingMore={false}
        onRetry={vi.fn()}
        onLoadMore={overrides.onLoadMore ?? vi.fn()}
        {...overrides}
      />
    </I18nextProvider>,
  );
}

describe("SessionContext", () => {
  it("shows coverage, all browser categories, and selects the newest event", () => {
    renderContext();

    expect(screen.getByRole("heading", { name: "会话上下文" })).toBeTruthy();
    expect(screen.getAllByText("已观测").length).toBeGreaterThan(0);
    expect(screen.getAllByText("完成修改").length).toBeGreaterThan(0);
    expect(screen.getByText("系统提示")).toBeTruthy();
    expect(screen.getByText("工具定义")).toBeTruthy();
    expect(screen.getAllByText("用户消息").length).toBeGreaterThan(0);
    expect(screen.getByText("注入上下文")).toBeTruthy();
    expect(screen.getByText("Agent 回复")).toBeTruthy();
    expect(screen.getByText("工具结果")).toBeTruthy();
    expect(screen.getByText("请完成这个改动")).toBeTruthy();
  });

  it("requests an older page from the timeline footer", () => {
    const onLoadMore = vi.fn();
    renderContext({ onLoadMore });

    fireEvent.click(screen.getByRole("button", { name: "加载更早的上下文事件" }));

    expect(onLoadMore).toHaveBeenCalledOnce();
  });
});
