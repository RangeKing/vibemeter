// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import i18n from "../i18n";
import type {
  AppSettings,
  EdgeState,
  ProviderAccount,
  ProviderUsage,
  RateWindow,
  SourceStatus,
} from "../types";
import { EdgeSidebar, notchHeightFor } from "./EdgeSidebar";

const mocks = vi.hoisted(() => ({
  state: {
    enabled: true,
    expanded: true,
    pinned: false,
    side: "right",
    offset: 0.5,
    notchTop: 0,
  } as EdgeState,
  listeners: new Map<string, (event: { payload: unknown }) => void>(),
  expand: vi.fn(),
  settings: vi.fn(),
  main: vi.fn(),
  providers: vi.fn(),
  sources: vi.fn(),
  placement: vi.fn(),
  startDrag: vi.fn(),
  endDrag: vi.fn(async () => ({ moved: false, offset: 0.5 })),
  setSetting: vi.fn(),
  refreshProviders: vi.fn(),
  credentialsAllowed: "true",
  edgeSidebarAgents: "auto",
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, cb: (event: { payload: unknown }) => void) => {
    mocks.listeners.set(name, cb);
    return () => mocks.listeners.delete(name);
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
    setEdgePlacement: mocks.placement,
    startEdgeDrag: mocks.startDrag,
    endEdgeDrag: mocks.endDrag,
    setSetting: mocks.setSetting,
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
function account(provider: string, windows: RateWindow[], available = true): ProviderAccount {
  return {
    id: `subscription:${provider}`,
    provider,
    kind: "subscription",
    label: `${provider}-oauth`,
    available,
    windows,
    refreshedAt: "2026-09-17T02:00:00Z",
  };
}
function provider(name: string, windows: RateWindow[], available = true): ProviderUsage {
  return {
    provider: name,
    available,
    source: `${name}-oauth`,
    windows,
    accounts: [account(name, windows, available)],
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
  mocks.listeners.clear();
  mocks.state = {
    enabled: true,
    expanded: true,
    pinned: false,
    side: "right",
    offset: 0.5,
    notchTop: 0,
  };
  mocks.credentialsAllowed = "true";
  mocks.edgeSidebarAgents = "auto";
  mocks.refreshProviders.mockResolvedValue([]);
  mocks.placement.mockResolvedValue(undefined);
  mocks.startDrag.mockResolvedValue(undefined);
  mocks.endDrag.mockResolvedValue({ moved: false, offset: 0.5 });
  mocks.setSetting.mockResolvedValue(undefined);
  mocks.sources.mockResolvedValue([
    source("claude-code"),
    source("codex"),
    source("cursor"),
    source("deepseek-harness"),
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
    mocks.listeners.get("edge-state")?.({ payload: mocks.state });
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
    // Not in the automatic list — it has no subscription to read — so it takes
    // an explicit tick to appear.
    mocks.edgeSidebarAgents = JSON.stringify(["claude-code", "zcode"]);
    await mount();
    await act(async () => {
      fireEvent.pointerEnter(screen.getByRole("button", { name: /^ZCode/ }));
    });
    expect(
      screen.getByText("This Agent has no subscription VibeMeter can read. Nothing is estimated in its place."),
    ).toBeTruthy();
    expect(screen.queryByRole("button", { name: /^Hermes/ })).toBeNull();
  });

  it("reports an API balance as an amount, with no bar and no reset", async () => {
    mocks.edgeSidebarAgents = JSON.stringify(["deepseek-harness"]);
    mocks.providers.mockResolvedValue([
      {
        provider: "deepseek",
        available: true,
        source: "api-key",
        windows: [],
        accounts: [
          {
            id: "deepseek:a",
            provider: "deepseek",
            kind: "api" as const,
            label: "work key",
            available: true,
            windows: [],
            balance: {
              currency: "CNY",
              total: 110,
              granted: 10,
              toppedUp: 100,
              spendable: true,
              provenance: "observed",
            },
            refreshedAt: "2026-09-18T02:00:00Z",
          },
        ],
        health: { state: "operational", description: "", statusUrl: "" },
        refreshedAt: "2026-09-18T02:00:00Z",
        stale: false,
      },
    ]);
    await mount();
    expect(screen.getByText("API key")).toBeTruthy();
    expect(screen.getByText("work key")).toBeTruthy();
    expect(screen.getByText("CN¥110.00")).toBeTruthy();
    expect(screen.getByText("CN¥10.00 granted · CN¥100.00 topped up")).toBeTruthy();
    // A balance has no window, so nothing draws a track or a countdown.
    expect(document.querySelector(".edge-quota-track")).toBeNull();
    // The ring carries the amount, since there is no percentage to carry.
    expect(screen.getByRole("button", { name: /DeepSeek Harness · ¥110/ })).toBeTruthy();
  });

  it("totals several keys on one provider and lists each of them", async () => {
    mocks.edgeSidebarAgents = JSON.stringify(["deepseek-harness"]);
    const key = (id: string, label: string, total: number) => ({
      id,
      provider: "deepseek",
      kind: "api" as const,
      label,
      available: true,
      windows: [],
      balance: {
        currency: "CNY" as const,
        total,
        spendable: true,
        provenance: "observed",
      },
      refreshedAt: "2026-09-18T02:00:00Z",
    });
    mocks.providers.mockResolvedValue([
      {
        provider: "deepseek",
        available: true,
        source: "api-key",
        windows: [],
        accounts: [key("deepseek:a", "work key", 110), key("deepseek:b", "side key", 40)],
        health: { state: "operational", description: "", statusUrl: "" },
        refreshedAt: "2026-09-18T02:00:00Z",
        stale: false,
      },
    ]);
    await mount();
    expect(screen.getByText("Across 2 accounts")).toBeTruthy();
    expect(screen.getByText("CN¥150.00")).toBeTruthy();
    // And each key stays visible on its own, since they are separate limits.
    expect(screen.getByText("work key")).toBeTruthy();
    expect(screen.getByText("side key")).toBeTruthy();
    expect(screen.getByText("CN¥110.00")).toBeTruthy();
    expect(screen.getByText("CN¥40.00")).toBeTruthy();
  });

  it("shows only Agents with a subscription to read unless told otherwise", async () => {
    await mount();
    for (const name of [/^Claude Code/, /^Codex/, /^Cursor/, /^DeepSeek Harness/]) {
      expect(screen.getByRole("button", { name })).toBeTruthy();
    }
    // Detected, but there is no subscription behind it: a ring that could only
    // ever say so is not worth a slot by default.
    expect(screen.queryByRole("button", { name: /^ZCode/ })).toBeNull();
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

  it("does not re-ask an open panel to open on every ring it sweeps", async () => {
    await mount();
    mocks.expand.mockClear();
    // The panel is already open; crossing four rings is four selections and no
    // native resize at all.
    for (const name of [/^Codex/, /^Cursor/, /^DeepSeek Harness/, /^Claude Code/]) {
      await act(async () => {
        fireEvent.pointerEnter(screen.getByRole("button", { name }));
      });
    }
    expect(mocks.expand).not.toHaveBeenCalled();
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("Claude Code");
  });

  it("drags along the edge without selecting the ring it started on", async () => {
    await mount();
    const strip = document.querySelector(".edge-notch") as HTMLElement;
    strip.setPointerCapture = vi.fn();
    strip.releasePointerCapture = vi.fn();
    // The page reports how long the notch and the card are, so the panel is
    // never taller than what it shows — a taller one costs travel.
    expect(mocks.placement).toHaveBeenLastCalledWith(276, undefined);
    mocks.placement.mockClear();

    await act(async () => {
      fireEvent.pointerDown(strip, { button: 0, pointerId: 1, screenY: 500 });
    });
    // The press says only that a drag may have begun. No coordinate from the
    // page goes anywhere: every one of them is measured against the window
    // this drag is about to move, so feeding one back in would make the strip
    // chase its own movement.
    expect(mocks.startDrag).toHaveBeenCalledOnce();
    expect(mocks.placement).not.toHaveBeenCalled();
    await act(async () => {
      fireEvent.pointerMove(strip, { pointerId: 1, screenY: 560 });
    });
    expect(mocks.placement).not.toHaveBeenCalled();

    // Until it has travelled far enough, the press is still a click, so the
    // strip has not moved and the rings still answer the pointer.
    const codex = screen.getByRole("button", { name: /^Codex/ });
    await act(async () => {
      fireEvent.pointerEnter(codex);
    });
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("Codex");

    // The native tracker owns the threshold and says when it is crossed.
    await act(async () => {
      mocks.listeners.get("edge-drag")?.({ payload: true });
    });
    expect(strip.className).toContain("is-dragging");

    // A drag that passes over a ring must not switch the card to it.
    const claude = screen.getByRole("button", { name: /^Claude Code/ });
    await act(async () => {
      fireEvent.pointerEnter(claude);
      fireEvent.click(claude);
    });
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("Codex");

    // Dragging pulls the window out from under the pointer, so leaving the
    // panel mid-drag must not start folding it away.
    mocks.expand.mockClear();
    fireEvent.pointerLeave(screen.getByRole("main"));
    await act(async () => {
      vi.advanceTimersByTime(1000);
    });
    expect(mocks.expand).not.toHaveBeenCalled();

    // The offset is written back once, at the end, and it is the one the
    // tracker ended on rather than anything the page worked out.
    mocks.endDrag.mockResolvedValue({ moved: true, offset: 0.82 });
    await act(async () => {
      fireEvent.pointerUp(strip, { pointerId: 1, screenY: 560 });
    });
    expect(mocks.setSetting).toHaveBeenCalledExactlyOnceWith("edgeSidebarOffset", "0.82");
  });

  it("leaves a press that never moved alone", async () => {
    await mount();
    const strip = document.querySelector(".edge-notch") as HTMLElement;
    strip.setPointerCapture = vi.fn();
    strip.releasePointerCapture = vi.fn();
    mocks.placement.mockClear();

    await act(async () => {
      fireEvent.pointerDown(strip, { button: 0, pointerId: 1, screenY: 500 });
      fireEvent.pointerUp(strip, { pointerId: 1, screenY: 500 });
    });
    // Nothing was placed, and nothing was saved: a click on the strip is a
    // click, not a move to wherever the pointer happens to be.
    expect(mocks.placement).not.toHaveBeenCalled();
    expect(mocks.setSetting).not.toHaveBeenCalled();
    expect(strip.className).not.toContain("is-dragging");
  });

  it("puts the strip where the panel says, so an open card costs it no travel", async () => {
    mocks.state = { ...mocks.state, notchTop: 97 };
    await mount();
    const strip = document.querySelector(".edge-notch") as HTMLElement;
    expect(strip.style.marginTop).toBe("97px");
  });

  it("keeps the notch inside the folded panel however many providers report", async () => {
    expect(notchHeightFor(1)).toBe(117);
    expect(notchHeightFor(3)).toBe(223);
    // Every supported Agent still fits inside the folded panel.
    expect(notchHeightFor(9)).toBe(541);
    expect(notchHeightFor(40)).toBe(720);
  });
});
