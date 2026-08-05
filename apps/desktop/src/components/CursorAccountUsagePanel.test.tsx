// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { I18nextProvider } from "react-i18next";
import i18n from "../i18n";
import { localDateKey } from "../lib/providerUsage";
import { CursorAccountUsagePanel } from "./CursorAccountUsagePanel";

const { providers, settings, showSettings } = vi.hoisted(() => ({
  providers: vi.fn(),
  settings: vi.fn(),
  showSettings: vi.fn(),
}));

vi.mock("../lib/api", () => ({
  api: { providers, settings, showSettings },
}));

function renderPanel() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });
  return render(
    <I18nextProvider i18n={i18n}>
      <QueryClientProvider client={queryClient}>
        <CursorAccountUsagePanel locale="zh-CN" range="today" />
      </QueryClientProvider>
    </I18nextProvider>,
  );
}

describe("CursorAccountUsagePanel", () => {
  beforeAll(async () => {
    await i18n.changeLanguage("zh-CN");
  });

  beforeEach(() => {
    providers.mockReset();
    settings.mockReset();
    showSettings.mockReset();
  });

  afterEach(cleanup);

  it("is off by default and does not fetch provider history", async () => {
    settings.mockResolvedValue({ cursorDashboardUsage: "false", credentialsAllowed: "false" });
    const { container } = renderPanel();
    await waitFor(() => expect(settings).toHaveBeenCalled());
    expect(container?.textContent).toBe("");
    expect(screen.queryByText("Cursor 账户 Token 与成本未开启")).toBeNull();
    expect(providers).not.toHaveBeenCalled();
  });

  it("shows selected-range tokens and both cost meanings when enabled", async () => {
    const today = localDateKey(new Date());
    settings.mockResolvedValue({ cursorDashboardUsage: "true", credentialsAllowed: "true" });
    providers.mockResolvedValue([
      {
        provider: "cursor",
        available: true,
        source: "cursor-desktop-session",
        windows: [],
        health: { state: "operational", description: "", statusUrl: "https://status.cursor.com" },
        stale: false,
        accountUsage: {
          periodStart: today,
          periodEnd: today,
          fetchedAt: new Date().toISOString(),
          scope: "account",
          daily: [{
            date: today,
            model: "composer-1",
            inputTokens: 100,
            outputTokens: 20,
            cacheReadTokens: 30,
            cacheWriteTokens: 10,
            apiCostUsd: 0.2,
            meteredCostUsd: 0.1,
            requestCount: 2,
            tokenRequestCount: 1,
          }],
        },
      },
    ]);
    renderPanel();
    await waitFor(() => expect(screen.getAllByText("160")).toHaveLength(2));
    expect(screen.getByText("账户 · 今天")).toBeTruthy();
    expect(screen.getByText("API 标价估算")).toBeTruthy();
    expect(screen.getByText("Cursor 实际扣费")).toBeTruthy();
    expect(screen.getByText("composer-1")).toBeTruthy();
  });

  it("shows an explicit loading state while account history is being read", async () => {
    settings.mockResolvedValue({ cursorDashboardUsage: "true", credentialsAllowed: "true" });
    providers.mockReturnValue(new Promise(() => undefined));

    renderPanel();

    expect(await screen.findByText("正在读取 Cursor Dashboard 用量…")).toBeTruthy();
    expect(document.querySelector(".cursor-account-panel.loading .spin")).not.toBeNull();
  });
});
