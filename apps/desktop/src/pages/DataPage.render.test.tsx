// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { I18nextProvider } from "react-i18next";
import i18n from "../i18n";
import { useUiStore } from "../store";
import type { OverviewResponse, SourceStatus } from "../types";
import { DataPage } from "./DataPage";

const { comparison, overview, providers, settings, sources } = vi.hoisted(() => ({
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

vi.mock("../components/EChart", () => ({ EChart: () => null }));
vi.mock("../components/CursorAccountUsagePanel", () => ({ CursorAccountUsagePanel: () => null }));

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

  it("renders the ZCode filter before historical sessions are available", async () => {
    const zcode: SourceStatus = {
      agent: "zcode",
      available: false,
      selected: true,
      capabilityLevel: "partial",
      liveCapability: "experimental",
      parserVersion: "test-parser",
      sessionCount: 0,
      status: "not-found",
      warningCount: 0,
      pathLabel: "",
    };
    overview.mockResolvedValue(emptyOverview);
    sources.mockResolvedValue([zcode]);

    renderDataPage();

    const button = await screen.findByRole("button", { name: "ZCode" });
    expect(button.getAttribute("aria-pressed")).toBe("false");
  });
});
