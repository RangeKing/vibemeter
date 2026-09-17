// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import i18n from "../i18n";
import type { AppSettings, EdgeState, ProviderUsage, RateWindow, SourceStatus } from "../types";
import { EdgeSidebar, notchHeightFor } from "./EdgeSidebar";

const mocks = vi.hoisted(() => ({
  state: { enabled: true, expanded: true, pinned: false, side: "right" } as EdgeState,
  listener: undefined as undefined | ((event: { payload: EdgeState }) => void),
  expand: vi.fn(),
  settings: vi.fn(),
  main: vi.fn(),
  providers: vi.fn(),
  sources: vi.fn(),
  refreshProviders: vi.fn(),
  credentialsAllowed: "true",
  edgeSidebarAgents: "auto",
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (_name, cb) => {
    mocks.listener = cb;
    return () => {};
  }),
}));
vi.mock("../lib/api", () => ({
  api: {
    edgeState: async () => mocks.state,
    setEdgeExpanded: mocks.expand,
    showSettings: mocks.settings,
    showMain: mocks.main,
    providers: mocks.providers,
    sources: mocks.sources,
    refreshProviders: mocks.refreshProviders,
    settings: async (): Promise<Partial<AppSettings>> => ({
      credentialsAllowed: mocks.credentialsAllowed,
      edgeSidebarAgents: mocks.edgeSidebarAgents,
      cursorDashboardUsage: "false",
      useSystemProxy: "false",
    }),
  },
}));

function window_(id: string, label: string, usedPercent?: number, resetAt?: string): RateWindow {
  return { id, label, usedPercent, resetAt, provenance: "observed" };
}
function provider(name: string, windows: RateWindow[], available = true): ProviderUsage {
  return {
    provider: name,
    available,
    source: `${name}-oauth`,
    windows,
    health: { state: "operational", description: "", statusUrl: "https://example.test" },
    refreshedAt: "2026-09-17T02:00:00Z",
    stale: false,
  };
}

function source(agent: string, available = true): SourceStatus {
  return {
    agent,
    available,
    selected: true,
    capabilityLevel: "full",
    liveCapability: "exact",
    parserVersion: "1",
    sessionCount: 1,
    status: available ? "ready" : "not-found",
    warningCount: 0,
    pathLabel: agent,
  };
}

async function mount() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  await act(async () => {
    render(
      <I18nextProvider i18n={i18n}>
        <QueryClientProvider client={client}>
          <EdgeSidebar locale="en-US" />
        </QueryClientProvider>
      </I18nextProvider>,
    );
  });
}

beforeEach(async () => {
  vi.useFakeTimers();
  vi.clearAllMocks();
  await i18n.changeLanguage("en-US");
  mocks.state = { enabled: true, expanded: true, pinned: false, side: "right" };
  mocks.credentialsAllowed = "true";
  mocks.edgeSidebarAgents = "auto";
  mocks.refreshProviders.mockResolvedValue([]);
  mocks.sources.mockResolvedValue([
    source("claude-code"),
    source("codex"),
    source("cursor"),
    source("zcode"),
    source("hermes", false),
  ]);
  mocks.providers.mockResolvedValue([
    provider("claude", [
      window_("session", "quota.session", 73, "2026-09-17T03:00:00Z"),
      window_("weekly", "quota.weekly", 7),
    ]),
    provider("codex", [window_("codex-primary", "quota.window", 21)]),
    provider("cursor", [], false),
  ]);
  mocks.expand.mockImplementation(async (expanded: boolean, pinned?: boolean) => {
    mocks.state = { ...mocks.state, expanded, pinned: pinned ?? mocks.state.pinned };
    mocks.listener?.({ payload: mocks.state });
  });
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("Edge sidebar", () => {
  it("reports remaining subscription quota and its reset, not live sessions", async () => {
    await mount();
    // The first provider's windows, as remaining rather than consumed.
    expect(screen.getByText("Current session")).toBeTruthy();
    expect(screen.getByText("27% left")).toBeTruthy();
    expect(screen.getByText("93% left")).toBeTruthy();
    expect(screen.getByText(/Resets /)).toBeTruthy();
    // The ring carries the tightest window for that provider.
    expect(screen.getByRole("button", { name: "Claude Code · 27% left" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Codex · 79% left" })).toBeTruthy();
  });

  it("switches provider on hover and keeps an unreported quota explicit", async () => {
    await mount();
    await act(async () => {
      fireEvent.pointerEnter(screen.getByRole("button", { name: /^Cursor/ }));
    });
    expect(screen.getByRole("button", { name: "Cursor · Quota not reported" })).toBeTruthy();
    expect(screen.getByText("Quota not reported")).toBeTruthy();
    expect(screen.queryByText("27% left")).toBeNull();
    // An unreported window draws no arc rather than a full one.
    expect(document.querySelectorAll(".edge-ring-arc")).toHaveLength(2);
  });

  it("gives an Agent with no readable subscription a ring and says why", async () => {
    await mount();
    await act(async () => {
      fireEvent.pointerEnter(screen.getByRole("button", { name: /^ZCode/ }));
    });
    expect(
      screen.getByText("This Agent has no subscription VibeMeter can read. Nothing is estimated in its place."),
    ).toBeTruthy();
    // Undetected Agents stay out of the automatic list.
    expect(screen.queryByRole("button", { name: /^Hermes/ })).toBeNull();
  });

  it("offers the settings route instead of a quota when credentials are off", async () => {
    mocks.credentialsAllowed = "false";
    await mount();
    expect(screen.getByText("Quota access is off")).toBeTruthy();
    expect(mocks.refreshProviders).not.toHaveBeenCalled();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /Quota access is off/ }));
    });
    expect(mocks.settings).toHaveBeenCalledOnce();
  });

  it("refreshes the reading on its own clock while it stays open", async () => {
    await mount();
    expect(mocks.refreshProviders).toHaveBeenCalledWith(true, false, false);
    mocks.refreshProviders.mockClear();
    await act(async () => {
      vi.advanceTimersByTime(5 * 60_000);
    });
    expect(mocks.refreshProviders).toHaveBeenCalledOnce();
  });

  it("waits 450ms on leave, cancels on reentry and keeps a pinned panel open", async () => {
    await mount();
    const panel = screen.getByRole("main");
    fireEvent.pointerLeave(panel);
    await act(async () => {
      vi.advanceTimersByTime(400);
    });
    expect(mocks.expand).not.toHaveBeenCalled();
    fireEvent.pointerEnter(panel);
    await act(async () => {
      vi.advanceTimersByTime(100);
    });
    expect(mocks.expand).not.toHaveBeenCalled();
    fireEvent.pointerLeave(panel);
    await act(async () => {
      vi.advanceTimersByTime(450);
    });
    expect(mocks.expand).toHaveBeenLastCalledWith(false, undefined);
    await act(async () => {
      fireEvent.pointerEnter(panel);
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Keep open" }));
    });
    mocks.expand.mockClear();
    fireEvent.pointerLeave(panel);
    await act(async () => {
      vi.advanceTimersByTime(1000);
    });
    expect(mocks.expand).not.toHaveBeenCalled();
    await act(async () => {
      fireEvent.keyDown(panel, { key: "Escape" });
    });
    expect(mocks.expand).toHaveBeenLastCalledWith(false, false);
    expect(document.querySelector(".edge-sheet")?.hasAttribute("inert")).toBe(true);
  });

  it("distinguishes an empty provider list from a failed read", async () => {
    mocks.sources.mockResolvedValue([]);
    await mount();
    expect(screen.getByText("No Agent detected")).toBeTruthy();
    cleanup();
    mocks.sources.mockResolvedValue([source("claude-code")]);
    mocks.providers.mockRejectedValue(new Error("unavailable"));
    await mount();
    expect(screen.queryByText("No Agent detected")).toBeNull();
    expect(screen.getByRole("alert").textContent).toContain("Quota is unavailable");
  });

  it("shows only the subscriptions the settings keep, and says so when none", async () => {
    mocks.edgeSidebarAgents = JSON.stringify(["codex"]);
    await mount();
    expect(screen.getByRole("button", { name: /^Codex/ })).toBeTruthy();
    expect(screen.queryByRole("button", { name: /^Claude/ })).toBeNull();
    expect(screen.queryByRole("button", { name: /^Cursor/ })).toBeNull();
    cleanup();
    // An empty list is the user asking for none, which must not read as a
    // provider outage.
    mocks.edgeSidebarAgents = "[]";
    await mount();
    expect(screen.getByText("Every Agent is hidden")).toBeTruthy();
    expect(screen.queryByText("No Agent detected")).toBeNull();
  });

  it("keeps the notch inside the folded panel however many providers report", async () => {
    expect(notchHeightFor(1)).toBe(147);
    expect(notchHeightFor(3)).toBe(279);
    // Every supported Agent still fits inside the folded panel.
    expect(notchHeightFor(9)).toBe(675);
    expect(notchHeightFor(40)).toBe(720);
  });
});
