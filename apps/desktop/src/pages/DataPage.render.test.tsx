// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { I18nextProvider } from "react-i18next";
import i18n from "../i18n";
import { useUiStore } from "../store";
import type { OverviewResponse, SourceStatus } from "../types";
import { DataPage } from "./DataPage";

const { chartProps, comparison, overview, providers, settings, sources } = vi.hoisted(() => ({
  chartProps: [] as Array<{ ariaLabel?: string; option?: { grid?: { right?: number } } }>,
  comparison: vi.fn(),
  overview: vi.fn(),
  providers: vi.fn(),
  settings: vi.fn(),
  sources: vi.fn(),
}));

vi.mock("../lib/api", () => ({
  api: {
    comparison,
    overview,
    providers,
    settings,
    sources,
  },
}));

vi.mock("../components/EChart", () => ({
  EChart: (props: { ariaLabel?: string; option?: { grid?: { right?: number } } }) => {
    chartProps.push(props);
    return null;
  },
}));

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((resolvePromise) => { resolve = resolvePromise; });
  return { promise, resolve };
}

const emptyOverview = {
  range: "90d",
  generatedAt: "2026-08-10T00:00:00Z",
  pricingVersion: "test",
  totals: {
    sessionCount: 0,
    activeSeconds: 0,
    activeDays: 0,
    usage: {
      inputTokens: 0,
      outputTokens: 0,
      cacheReadTokens: 0,
      cacheWriteTokens: 0,
      cacheWrite1hTokens: 0,
      reasoningTokens: 0,
    },
    costCoverage: 0,
    longestUninterruptedSeconds: 0,
    filesTouched: 0,
    linesAdded: 0,
    linesDeleted: 0,
    errors: 0,
    retries: 0,
  },
  daily: [],
  hourly: [],
  agents: [],
  models: [],
  tools: [],
  skills: {
    mostUsed: [],
    leastUsed: [],
    installedWithoutUsage: [],
    installedCount: 0,
    usedCount: 0,
  },
  behavior: {},
  recentSessions: [],
  coverage: [],
  indexStatus: { running: false },
} as unknown as OverviewResponse;

function renderDataPage() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });
  return render(
    <I18nextProvider i18n={i18n}>
      <QueryClientProvider client={queryClient}>
        <DataPage locale="zh-CN" />
      </QueryClientProvider>
    </I18nextProvider>,
  );
}

function source(agent: string): SourceStatus {
  return {
    agent,
    available: true,
    selected: true,
    capabilityLevel: "full",
    liveCapability: "exact",
    parserVersion: "test-parser",
    sessionCount: 1,
    status: "ready",
    warningCount: 0,
    pathLabel: "test",
  };
}

describe("DataPage query transition", () => {
  beforeAll(async () => {
    Object.defineProperty(window, "matchMedia", {
      configurable: true,
      value: vi.fn(() => ({
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
      })),
    });
    await i18n.changeLanguage("zh-CN");
  });

  beforeEach(() => {
    chartProps.length = 0;
    comparison.mockReset();
    overview.mockReset();
    providers.mockReset();
    settings.mockReset();
    sources.mockReset();
    comparison.mockResolvedValue([]);
    settings.mockResolvedValue({ credentialsAllowed: "false", cursorDashboardUsage: "false" });
    useUiStore.setState({ page: "data", selectedSessionId: undefined, range: "90d" });
  });

  afterEach(cleanup);

  it("keeps the React hook order stable when required queries finish loading", async () => {
    const overviewResult = deferred<OverviewResponse>();
    const sourceResult = deferred<[]>();
    overview.mockReturnValue(overviewResult.promise);
    sources.mockReturnValue(sourceResult.promise);

    renderDataPage();

    await act(async () => {
      overviewResult.resolve(emptyOverview);
      sourceResult.resolve([]);
    });

    await waitFor(() => expect(screen.getByRole("heading", { name: /你与 Agent/ })).toBeTruthy());
    expect(screen.queryByRole("heading", { name: "工作事件" })).toBeNull();
  });

  it("does not render an undetected ZCode filter", async () => {
    const zcode: SourceStatus = {
      agent: "zcode",
      available: false,
      selected: true,
      capabilityLevel: "partial",
      liveCapability: "exact",
      parserVersion: "test-parser",
      sessionCount: 0,
      status: "not-found",
      warningCount: 0,
      pathLabel: "",
    };
    overview.mockResolvedValue(emptyOverview);
    sources.mockResolvedValue([zcode]);

    renderDataPage();

    await screen.findByRole("heading", { name: /你与 Agent/ });
    expect(screen.queryByRole("button", { name: "ZCode" })).toBeNull();
  });

  it("does not render a standalone Cursor usage panel", async () => {
    overview.mockResolvedValue(emptyOverview);
    sources.mockResolvedValue([]);

    renderDataPage();

    await screen.findByRole("heading", { name: /你与 Agent/ });
    expect(screen.queryByLabelText("Cursor Dashboard 用量")).toBeNull();
  });

  it("reserves enough right-side space for workflow values", async () => {
    overview.mockResolvedValue({
      ...emptyOverview,
      tools: [{ id: "shell", label: "shell", value: 57_000 }],
    });
    sources.mockResolvedValue([]);

    renderDataPage();

    await waitFor(() => {
      const chart = chartProps.find((props) => props.ariaLabel === "工作流足迹");
      expect(chart?.option?.grid?.right).toBeGreaterThanOrEqual(48);
    });
  });

  it("updates the API-equivalent cost when an agent is filtered out", async () => {
    const usage = {
      inputTokens: 1,
      outputTokens: 0,
      cacheReadTokens: 0,
      cacheWriteTokens: 0,
      cacheWrite1hTokens: 0,
      reasoningTokens: 0,
    };
    overview.mockResolvedValue({
      ...emptyOverview,
      totals: { ...emptyOverview.totals, sessionCount: 2, estimatedCostUsd: 99 },
      agents: [
        { id: "codex", label: "codex", value: 1, provenance: "observed" },
        { id: "claude-code", label: "claude-code", value: 1, provenance: "observed" },
      ],
      daily: [
        { date: "2026-08-10", agent: "codex", model: "gpt-5.4", usage, activeSeconds: 1, sessionCount: 1, toolCalls: 0, errors: 0, estimatedCostUsd: 10 },
        { date: "2026-08-10", agent: "claude-code", model: "claude-sonnet-4-6", usage, activeSeconds: 1, sessionCount: 1, toolCalls: 0, errors: 0, estimatedCostUsd: 3 },
      ],
    });
    sources.mockResolvedValue([source("codex"), source("claude-code")]);

    renderDataPage();

    const costLabel = await screen.findByText("API 等价成本估算");
    const costMetric = costLabel.parentElement;
    expect(costMetric?.textContent).toContain("US$13.00");

    fireEvent.click(screen.getByRole("button", { name: "Codex" }));

    await waitFor(() => expect(costMetric?.textContent).toContain("US$3.00"));
    expect(costMetric?.textContent).not.toContain("US$13.00");
  });

  it("hides undetected Agents from the top-right filter in automatic mode", async () => {
    overview.mockResolvedValue({
      ...emptyOverview,
      agents: [{ id: "codex", label: "codex", value: 1, provenance: "observed" }],
      daily: [],
    });
    sources.mockResolvedValue([
      source("codex"),
      { ...source("zcode"), available: false, sessionCount: 0, status: "not-found" },
    ]);
    settings.mockResolvedValue({ credentialsAllowed: "false", cursorDashboardUsage: "false", dataPageAgents: "auto" });

    renderDataPage();

    expect(await screen.findByRole("button", { name: "Codex" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "ZCode" })).toBeNull();
  });

  it("respects a custom Data page Agent display list", async () => {
    overview.mockResolvedValue({
      ...emptyOverview,
      agents: [{ id: "grok-build", label: "grok-build", value: 1, provenance: "observed" }],
      daily: [],
    });
    sources.mockResolvedValue([source("codex"), source("grok-build")]);
    settings.mockResolvedValue({ credentialsAllowed: "false", cursorDashboardUsage: "false", dataPageAgents: '["grok-build"]' });

    renderDataPage();

    expect(await screen.findByRole("button", { name: "Grok Build" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Codex" })).toBeNull();
  });
});
