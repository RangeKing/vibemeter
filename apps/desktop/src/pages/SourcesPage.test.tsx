// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import i18n from "../i18n";
import { useUiStore } from "../store";
import { SourcesPage } from "./SourcesPage";

const { indexStatus, liveSnapshot, settings, sources } = vi.hoisted(() => ({
  indexStatus: vi.fn(),
  liveSnapshot: vi.fn(),
  settings: vi.fn(),
  sources: vi.fn(),
}));

vi.mock("../lib/api", () => ({
  api: {
    indexStatus,
    liveSnapshot,
    settings,
    sources,
    refreshIndex: vi.fn(),
    setSourceSelected: vi.fn(),
    showSettings: vi.fn(),
  },
}));

beforeAll(async () => {
  await i18n.changeLanguage("zh-CN");
});

beforeEach(() => {
  sources.mockResolvedValue([]);
  liveSnapshot.mockResolvedValue({ hookStatus: { providers: [] } });
  indexStatus.mockResolvedValue({ running: false, discoveredFiles: 0, processedFiles: 0, indexedSessions: 0, messageKey: "index.complete" });
  settings.mockResolvedValue({ cursorDashboardUsage: "false" });
  useUiStore.setState({ page: "sources", selectedSessionId: undefined });
});

afterEach(cleanup);

describe("SourcesPage navigation", () => {
  it("returns to Settings from the page header", () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <I18nextProvider i18n={i18n}>
        <QueryClientProvider client={queryClient}>
          <SourcesPage locale="zh-CN" />
        </QueryClientProvider>
      </I18nextProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "返回" }));

    expect(useUiStore.getState().page).toBe("settings");
  });

  it("shows truthful per-source Delegation and Memory Read capabilities", async () => {
    sources.mockResolvedValue([{
      agent: "codex",
      available: true,
      selected: true,
      capabilityLevel: "full",
      liveCapability: "exact",
      parserVersion: "codex-fixture",
      sessionCount: 3,
      status: "ready",
      warningCount: 0,
      pathLabel: "",
    }]);
    liveSnapshot.mockResolvedValue({
      hookStatus: {
        providers: [{ provider: "codex", installed: true }],
      },
    });
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <I18nextProvider i18n={i18n}>
        <QueryClientProvider client={queryClient}>
          <SourcesPage locale="zh-CN" />
        </QueryClientProvider>
      </I18nextProvider>,
    );

    expect(await screen.findByText("委派轨迹")).toBeTruthy();
    expect(screen.getByText("精确")).toBeTruthy();
    expect(screen.getByText("记忆读取")).toBeTruthy();
    expect(screen.getByText("部分")).toBeTruthy();
  });
});
