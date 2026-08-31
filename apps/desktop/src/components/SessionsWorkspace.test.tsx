// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { I18nextProvider } from "react-i18next";
import i18n from "../i18n";
import { api } from "../lib/api";
import { useUiStore } from "../store";
import type { CanonicalEvent, DelegationTraceResponse, MemoryLedgerResponse, SessionContext, SessionDetail } from "../types";
import { SessionReplay } from "./SessionsWorkspace";

vi.mock("../lib/api", () => ({
  api: {
    sessionContext: vi.fn(),
    delegationTrace: vi.fn(),
    memoryLedger: vi.fn(),
    splitSession: vi.fn(),
  },
}));
vi.mock("./EChart", () => ({
  EChart: ({ ariaLabel }: { ariaLabel: string }) => <div aria-label={ariaLabel} />,
}));

beforeAll(async () => {
  await i18n.changeLanguage("zh-CN");
  Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
    configurable: true,
    value: vi.fn(),
  });
});

afterEach(() => {
  cleanup();
  useUiStore.getState().selectSession(undefined);
});

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

const contextData: SessionContext = {
  status: "ready",
  summary: {
    totalTokens: 20,
    inputTokens: 10,
    outputTokens: 10,
    cacheTokens: 0,
    reasoningTokens: 0,
    compactions: 0,
    eventCount: 1,
    coverage: "observed",
  },
  composition: [
    { key: "input", tokens: 10, coverage: "observed" },
    { key: "output", tokens: 10, coverage: "observed" },
  ],
  events: [],
  hasMore: false,
  browser: {
    categories: [
      { key: "system", items: [], coverage: "not-recorded" },
      { key: "tools", items: [], coverage: "not-recorded" },
      { key: "user", items: [], coverage: "not-recorded" },
      { key: "injected", items: [], coverage: "not-recorded" },
      { key: "assistant", items: [], coverage: "not-recorded" },
      { key: "tool_use", items: [], coverage: "not-recorded" },
      { key: "tool_result", items: [], coverage: "not-recorded" },
    ],
  },
};

const notRecordedTrace: DelegationTraceResponse = {
  status: "not-recorded",
  algorithmVersion: "delegation-trace-1.0.0",
  nodes: [],
  edges: [],
  anomalies: [],
  coverage: {
    capability: "unavailable",
    sourceCoverage: "not-recorded",
    relationCount: 0,
    evidenceCount: 0,
    observedCount: 0,
    derivedCount: 0,
    inferredCount: 0,
    unavailableSignals: ["delegation"],
  },
  evidence: [],
};

const notRecordedMemory: MemoryLedgerResponse = {
  status: "not-recorded",
  algorithmVersion: "memory-ledger-1.0.0",
  accesses: [],
  coverage: {
    capability: "partial",
    sourceCoverage: "not-recorded",
    accessCount: 0,
    readCount: 0,
    writeCount: 0,
    evidenceCount: 0,
    observedCount: 0,
    derivedCount: 0,
    inferredCount: 0,
    unavailableOperations: ["write"],
  },
  evidence: [],
};

const crossSessionTrace: DelegationTraceResponse = {
  status: "ready",
  rootSessionId: "session-1",
  algorithmVersion: "delegation-trace-1.0.0",
  nodes: [
    {
      id: "node-root",
      kind: "root-agent",
      agent: "codex",
      safeLabel: "Codex · root",
      sessionId: "session-1",
      status: "running",
      startedAt: "2026-08-14T10:00:00Z",
      evidenceLevel: "observed",
      sourceCoverage: "exact-delegation",
      confidence: 0.98,
    },
    {
      id: "node-child",
      kind: "subagent",
      agent: "codex",
      safeLabel: "Codex · child",
      sessionId: "session-child",
      status: "waiting",
      startedAt: "2026-08-14T10:00:01Z",
      evidenceLevel: "observed",
      sourceCoverage: "exact-delegation",
      confidence: 0.98,
    },
  ],
  edges: [{
    id: "edge-cross-session",
    from: "node-root",
    to: "node-child",
    relationType: "spawn",
    status: "waiting",
    confidence: 0.98,
    evidenceLevel: "observed",
    sourceCoverage: "exact-delegation",
    algorithmVersion: "delegation-trace-1.0.0",
    evidenceIds: ["canonical-child-start"],
  }],
  anomalies: [{
    id: "anomaly-cross-session",
    kind: "blocked-branch",
    severity: "warning",
    nodeIds: ["node-root", "node-child"],
    edgeIds: ["edge-cross-session"],
    reasonKey: "delegation.anomaly.blocked-branch",
    evidenceIds: ["canonical-child-start"],
    ruleVersion: "delegation-anomaly-1.0.0",
    confidence: 0.95,
  }],
  coverage: {
    capability: "exact",
    sourceCoverage: "exact-delegation",
    relationCount: 1,
    evidenceCount: 1,
    observedCount: 1,
    derivedCount: 0,
    inferredCount: 0,
    unavailableSignals: ["handoff"],
  },
  evidence: [{
    id: "evidence-cross-session",
    canonicalEventId: "canonical-child-start",
    sessionId: "session-child",
    role: "start",
    occurredAt: "2026-08-14T10:00:01Z",
    eventType: "delegation.spawn.started",
    evidenceLevel: "observed",
    sourceCoverage: "exact-delegation",
  }],
};

function renderReplay(
  session = detail,
  replayProps: Partial<Parameters<typeof SessionReplay>[0]> = {},
) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  return render(
    <I18nextProvider i18n={i18n}>
      <QueryClientProvider client={queryClient}>
        <SessionReplay detail={session} locale="zh-CN" onClose={() => undefined} {...replayProps} />
      </QueryClientProvider>
    </I18nextProvider>,
  );
}

describe("SessionReplay trajectory", () => {
  it("loads context only when the context tab is selected", async () => {
    vi.mocked(api.sessionContext).mockResolvedValue(contextData);
    renderReplay();

    expect(api.sessionContext).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "上下文" }));

    await waitFor(() => expect(screen.getByRole("heading", { name: "会话上下文" })).toBeTruthy());
    expect(api.sessionContext).toHaveBeenCalledWith("session-1", 0, 50);
  });

  it("loads Delegation only after its tab is selected", async () => {
    vi.mocked(api.delegationTrace).mockResolvedValue(notRecordedTrace);
    renderReplay();

    expect(api.delegationTrace).not.toHaveBeenCalled();
    expect(screen.getAllByRole("button", { name: /过程|上下文|委派|记忆/ }).map((button) => button.textContent)).toEqual(["过程", "上下文", "委派", "记忆"]);
    fireEvent.click(screen.getByRole("button", { name: "委派" }));

    await waitFor(() => expect(screen.getByRole("heading", { name: "未记录委派关系" })).toBeTruthy());
    expect(api.delegationTrace).toHaveBeenCalledWith("session-1");
  });

  it("opens the evidence session before returning cross-session Delegation evidence to Process", async () => {
    vi.mocked(api.delegationTrace).mockResolvedValue(crossSessionTrace);
    renderReplay();

    fireEvent.click(screen.getByRole("button", { name: "委派" }));
    await waitFor(() => expect(screen.getByRole("heading", { name: "委派异常" })).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: /分支正在等待或被阻塞/ }));
    fireEvent.click(screen.getByRole("button", { name: "回到过程" }));

    expect(useUiStore.getState().selectedSessionId).toBe("session-child");
  });

  it("continues cross-session Delegation evidence into its localized Process phase", async () => {
    const onProcessEvidenceFocused = vi.fn();
    const evidence = crossSessionTrace.evidence[0]!;
    renderReplay({
      ...detail,
      id: "session-child",
      title: "子分支",
      phases: [{
        ...detail.phases[0]!,
        eventCount: 1,
        events: [{
          ...event(1, evidence.eventType, "Delegation"),
          occurredAt: evidence.occurredAt,
        }],
      }],
    }, {
      processEvidence: evidence,
      onProcessEvidenceFocused,
    });

    await waitFor(() => expect(onProcessEvidenceFocused).toHaveBeenCalledTimes(1));
    expect(screen.getByRole("button", { name: "过程" }).getAttribute("aria-selected")).toBe("true");
    expect(screen.getByText("子 Agent 创建开始")).toBeTruthy();
    expect(screen.getAllByText("委派")).toHaveLength(2);
    expect(screen.queryByText("Delegation")).toBeNull();
    expect(screen.queryByText("delegation.spawn.started")).toBeNull();
  });

  it("loads Memory only after its tab is selected", async () => {
    vi.mocked(api.memoryLedger).mockResolvedValue(notRecordedMemory);
    renderReplay();

    expect(api.memoryLedger).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "记忆" }));

    await waitFor(() => expect(screen.getByRole("heading", { name: "未记录记忆访问" })).toBeTruthy());
    expect(api.memoryLedger).toHaveBeenCalledWith("session-1");
  });

  it("localizes Memory evidence in Process without exposing canonical identifiers", () => {
    renderReplay({
      ...detail,
      phases: [{
        ...detail.phases[0]!,
        eventCount: 1,
        events: [event(1, "memory.read", "MemoryAccess")],
      }],
    });

    expect(screen.getByText("读取记忆")).toBeTruthy();
    expect(screen.getByText("访问信号")).toBeTruthy();
    expect(screen.queryByText("MemoryAccess")).toBeNull();
    expect(screen.queryByText("memory.read")).toBeNull();
  });

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

  it("uses trajectory terminology and explains an authorized session with no commits", () => {
    renderReplay({
      ...detail,
      gitEvidence: { available: true, state: "available", branch: "main", commits: [] },
    });

    expect(screen.getByRole("heading", { name: "过程" })).toBeTruthy();
    expect(screen.getByRole("heading", { name: "文件轨迹" })).toBeTruthy();
    expect(screen.getByRole("heading", { name: "Git 轨迹" })).toBeTruthy();
    expect(screen.getByText("本次会话期间没有记录到提交。")).toBeTruthy();
  });
});
