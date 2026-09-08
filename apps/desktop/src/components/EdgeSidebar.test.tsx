// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import i18n from "../i18n";
import type { EdgeState, LiveSession } from "../types";
import { EdgeSidebar } from "./EdgeSidebar";
const mocks = vi.hoisted(() => ({
  state: { enabled: true, expanded: true, pinned: false, side: "right" } as EdgeState,
  listener: undefined as undefined | ((event: { payload: EdgeState }) => void),
  expand: vi.fn(), jump: vi.fn(), settings: vi.fn(), main: vi.fn(), refetch: vi.fn(),
  sessions: [] as LiveSession[], error: false,
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async (_name, cb) => { mocks.listener = cb; return () => {}; }) }));
vi.mock("../lib/api", () => ({ api: {
  edgeState: async () => mocks.state, setEdgeExpanded: mocks.expand, jumpToLiveSession: mocks.jump, showSettings: mocks.settings, showMain: mocks.main,
} }));
vi.mock("../lib/useLiveSnapshot", () => ({ useLiveSnapshot: () => ({ data: { sessions: mocks.sessions, attentionAvailable: true, attentionQueue: [] }, isError: mocks.error, isLoading: false, refetch: mocks.refetch }) }));
function session(agent: LiveSession["agent"], available = true): LiveSession {
  return { id: agent, sourceSessionId: agent, agent, projectLabel: `${agent}-project`, conversationTitle: `${agent} conversation`, status: "running", phase: "thinking", startedAt: "2026-09-08T00:00:00Z", updatedAt: "2026-09-08T00:01:00Z", actions: [], pulse: {
    lifecycle: { availability: available ? "available" : "not-recorded", value: available ? "running" : undefined, evidenceLevel: "observed", sourceCoverage: available ? "exact" : "unavailable" },
    workPhase: { availability: "available", value: "thinking", evidenceLevel: "derived", sourceCoverage: "exact" },
    attentionSignal: { availability: "available", value: "none", evidenceLevel: "derived", sourceCoverage: "exact" },
    freshness: { availability: "available", value: "fresh", evidenceLevel: "derived", sourceCoverage: "exact" },
  } };
}
async function mount() {
  await act(async () => { render(<I18nextProvider i18n={i18n}><EdgeSidebar locale="en-US" /></I18nextProvider>); });
}
beforeEach(async () => {
  vi.useFakeTimers(); vi.clearAllMocks(); await i18n.changeLanguage("en-US");
  mocks.state = { enabled: true, expanded: true, pinned: false, side: "right" };
  mocks.sessions = [session("codex"), session("claude-code", false)]; mocks.error = false;
  mocks.expand.mockImplementation(async (expanded: boolean, pinned?: boolean) => {
    mocks.state = { ...mocks.state, expanded, pinned: pinned ?? mocks.state.pinned };
    mocks.listener?.({ payload: mocks.state });
  });
  mocks.jump.mockResolvedValue(undefined);
});
afterEach(() => { cleanup(); vi.useRealTimers(); });
describe("Edge sidebar", () => {
  it("filters observed providers and keeps unavailable lifecycle explicit", async () => {
    await mount();
    expect(screen.getByText("Status not recorded")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Codex" }));
    expect(screen.getByText("codex conversation")).toBeTruthy();
    expect(screen.queryByText("claude-code conversation")).toBeNull();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Return to source" })); });
    expect(mocks.jump).toHaveBeenCalledWith("codex");
  });
  it("waits 450ms on leave, cancels on reentry and keeps a pinned panel open", async () => {
    await mount(); const panel = screen.getByRole("main");
    fireEvent.pointerLeave(panel);
    await act(async () => { vi.advanceTimersByTime(400); }); expect(mocks.expand).not.toHaveBeenCalled();
    fireEvent.pointerEnter(panel);
    await act(async () => { vi.advanceTimersByTime(100); }); expect(mocks.expand).not.toHaveBeenCalled();
    fireEvent.pointerLeave(panel);
    await act(async () => { vi.advanceTimersByTime(450); }); expect(mocks.expand).toHaveBeenLastCalledWith(false, undefined);
    await act(async () => { fireEvent.pointerEnter(panel); });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Keep open" })); });
    mocks.expand.mockClear(); fireEvent.pointerLeave(panel);
    await act(async () => { vi.advanceTimersByTime(1000); }); expect(mocks.expand).not.toHaveBeenCalled();
    await act(async () => { fireEvent.keyDown(panel, { key: "Escape" }); });
    expect(mocks.expand).toHaveBeenLastCalledWith(false, false);
    expect(document.querySelector(".edge-sheet")?.hasAttribute("inert")).toBe(true);
  });
  it("shows jump failure, preserves the session and exposes a retry", async () => {
    mocks.sessions = [session("codex")]; mocks.jump.mockRejectedValue(new Error("unavailable"));
    await mount(); await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Return to source" })); });
    expect(screen.getByRole("alert").textContent).toContain("Could not open");
    expect(screen.getByText("codex conversation")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Return to source" }).hasAttribute("disabled")).toBe(false);
  });
  it("distinguishes empty activity from failure", async () => {
    mocks.sessions = []; await mount(); expect(screen.getByText("No recent sessions")).toBeTruthy(); cleanup();
    mocks.error = true; await mount(); expect(screen.queryByText("No recent sessions")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Try again" })); expect(mocks.refetch).toHaveBeenCalledOnce();
  });
});
