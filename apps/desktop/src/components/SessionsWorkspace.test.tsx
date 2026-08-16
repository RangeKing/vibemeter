// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, it } from "vitest";
import { I18nextProvider } from "react-i18next";
import i18n from "../i18n";
import type { CanonicalEvent, SessionDetail } from "../types";
import { SessionReplay } from "./SessionsWorkspace";

beforeAll(async () => {
  await i18n.changeLanguage("zh-CN");
});

afterEach(cleanup);

function event(sequence: number, eventType: string, name: string): CanonicalEvent {
  return {
    sequence,
    eventType,
    category: "execute",
    name,
    occurredAt: `2026-08-14T10:00:${String(sequence).padStart(2, "0")}Z`,
    success: sequence === 5 ? false : true,
    durationMs: sequence * 100,
    provenance: "test",
  };
}

const detail = {
  id: "session-1",
  agent: "codex",
  model: "gpt-5",
  title: "实现会话轨迹",
  projectLabel: "vibemeter",
  startedAt: "2026-08-14T10:00:00Z",
  endedAt: "2026-08-14T10:01:00Z",
  activeSeconds: 60,
  usage: { inputTokens: 10, outputTokens: 10, cacheReadTokens: 0, cacheWriteTokens: 0, cacheWrite1hTokens: 0, reasoningTokens: 0 },
  costCoverage: 1,
  toolCalls: 4,
  filesTouched: 1,
  linesAdded: 10,
  linesDeleted: 2,
  errors: 1,
  retries: 0,
  verificationState: "verified",
  longestUninterruptedSeconds: 60,
  subagentCount: 0,
  hasCommit: false,
  provenance: "test",
  tools: [],
  daily: [],
  warnings: [],
  phases: [{
    id: "phase-1",
    phaseKey: "execute",
    startedAt: "2026-08-14T10:00:01Z",
    endedAt: "2026-08-14T10:00:06Z",
    eventCount: 6,
    provenance: "test",
    events: [
      event(1, "prompt.observed", "user"),
      event(2, "lifecycle.start", "started"),
      event(3, "tool.observed", "exec"),
      event(4, "verification.observed", "test"),
      event(5, "lifecycle.error", "error"),
      event(6, "lifecycle.complete", "completed"),
    ],
  }],
  contentPreview: {
    prompt: "Add a system proxy toggle",
    output: "The proxy toggle is ready",
  },
  fileChanges: [],
  gitEvidence: { available: false, state: "unavailable", commits: [] },
  capabilities: [],
  attention: [],
} as SessionDetail;

function renderReplay(session = detail) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  return render(
    <I18nextProvider i18n={i18n}>
      <QueryClientProvider client={queryClient}>
        <SessionReplay detail={session} locale="zh-CN" onClose={() => undefined} />
      </QueryClientProvider>
    </I18nextProvider>,
  );
}

describe("SessionReplay trajectory", () => {
  it("renders the dual overview and expands phase events independently", () => {
    renderReplay();

    expect(screen.getByLabelText("会话轨迹总览")).toBeTruthy();
    expect(screen.getByText("按时间")).toBeTruthy();
    expect(screen.getByText("输入")).toBeTruthy();
    expect(screen.getByText("Agent")).toBeTruthy();
    expect(screen.getByText("工具")).toBeTruthy();
    expect(screen.getByLabelText("会话时间轴")).toBeTruthy();
    expect(screen.getByText("0:00")).toBeTruthy();
    expect(screen.getByText("1:00")).toBeTruthy();
    expect(screen.getAllByText("Add a system proxy toggle")).toHaveLength(2);
    expect(screen.queryByText("observed")).toBeNull();
    expect(screen.queryByText("completed")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "展开其余 1 个事件" }));

    expect(screen.getByText("completed")).toBeTruthy();
    expect(screen.getAllByText("The proxy toggle is ready")).toHaveLength(2);
    expect(screen.getByRole("button", { name: "收起事件" }).getAttribute("aria-expanded")).toBe("true");
  });

  it("compresses dense phase rails inside their container and omits attention review", () => {
    const densePhases = Array.from({ length: 204 }, (_, index) => ({
      ...detail.phases[0],
      id: `phase-${index}`,
      eventCount: 1,
      events: [detail.phases[0]!.events[0]!],
    }));
    renderReplay({
      ...detail,
      phases: densePhases,
      attention: [{
        id: "attention-1",
        kind: "waiting",
        state: "open",
        reasonKey: "waiting",
        agent: "codex",
        sourceSessionId: "source-1",
        projectLabel: "vibemeter",
        openedAt: "2026-08-14T10:00:00Z",
        latestEvidenceAt: "2026-08-14T10:00:00Z",
        expiresAt: "2026-08-15T10:00:00Z",
        evidenceLevel: "observed",
        sourceCoverage: "exact-lifecycle",
        ruleVersion: "test",
        evidenceCount: 1,
        interventionCount: 0,
      }],
    });

    const rail = document.querySelector<HTMLElement>(".trajectory-phase-rail");
    const segments = document.querySelectorAll<HTMLElement>(".trajectory-phase-segment");
    expect(rail?.style.gap).toBe("0px");
    expect(segments).toHaveLength(204);
    expect([...segments].every((segment) => Number.parseFloat(segment.style.minWidth) === 0 && segment.style.flexShrink === "1")).toBe(true);
    expect(screen.queryByText("注意力复核")).toBeNull();
  });

  it("shows trajectory labels immediately instead of relying on the delayed native title", () => {
    renderReplay();

    const span = document.querySelector<HTMLElement>(".trajectory-span");
    expect(span).not.toBeNull();
    expect(span?.getAttribute("title")).toBeNull();

    fireEvent.mouseEnter(span!, { clientX: 240, clientY: 160 });

    expect(screen.getByRole("tooltip").textContent).toContain("user");
  });
});
