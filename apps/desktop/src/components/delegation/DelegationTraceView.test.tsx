// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import i18n from "../../i18n";
import { api } from "../../lib/api";
import type { DelegationTraceResponse } from "../../types";
import { DelegationTraceView } from "./DelegationTraceView";

vi.mock("../../lib/api", () => ({ api: { delegationTrace: vi.fn() } }));
vi.mock("../EChart", () => ({
  EChart: ({ option, onClick, ariaLabel }: {
    option: { series?: Array<{ data?: Array<{ id: string; name: string }>; links?: Array<{ id: string }> }> };
    onClick?: (event: { dataType: "node" | "edge"; data: { id: string } }) => void;
    ariaLabel: string;
  }) => {
    const series = option.series?.[0];
    return <div aria-label={ariaLabel}>
      {series?.data?.map((node) => <button key={node.id} aria-label={`graph-node-${node.id}`} onClick={() => onClick?.({ dataType: "node", data: node })}>{node.name}</button>)}
      {series?.links?.map((edge) => <button key={edge.id} aria-label={`graph-edge-${edge.id}`} onClick={() => onClick?.({ dataType: "edge", data: edge })}>{edge.id}</button>)}
    </div>;
  },
}));

beforeAll(async () => {
  await i18n.changeLanguage("zh-CN");
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

const readyTrace: DelegationTraceResponse = {
  status: "ready",
  rootSessionId: "root-source",
  algorithmVersion: "delegation-trace-1.0.0",
  nodes: [
    {
      id: "root",
      kind: "root-agent",
      agent: "codex",
      safeLabel: "Codex · root1234",
      sessionId: "session-root",
      status: "completed",
      startedAt: "2026-08-30T10:00:00Z",
      endedAt: "2026-08-30T10:10:00Z",
      outcome: "completed",
      evidenceLevel: "observed",
      sourceCoverage: "exact-delegation",
      confidence: 0.99,
    },
    {
      id: "child",
      kind: "subagent",
      agent: "codex",
      safeLabel: "Codex · child123",
      sessionId: "session-child",
      status: "completed",
      startedAt: "2026-08-30T10:01:00Z",
      endedAt: "2026-08-30T10:08:00Z",
      outcome: "returned",
      evidenceLevel: "observed",
      sourceCoverage: "exact-delegation",
      confidence: 0.98,
    },
  ],
  edges: [{
    id: "spawn-edge",
    from: "root",
    to: "child",
    relationType: "spawn",
    status: "completed",
    confidence: 0.98,
    evidenceLevel: "observed",
    sourceCoverage: "exact-delegation",
    algorithmVersion: "delegation-trace-1.0.0",
    evidenceIds: ["canonical-start", "canonical-stop"],
  }],
  anomalies: [],
  coverage: {
    capability: "exact",
    sourceCoverage: "exact-delegation",
    relationCount: 1,
    evidenceCount: 2,
    observedCount: 1,
    derivedCount: 0,
    inferredCount: 0,
    unavailableSignals: ["handoff"],
  },
  evidence: [
    {
      id: "evidence-start",
      canonicalEventId: "canonical-start",
      sessionId: "session-child",
      role: "start",
      occurredAt: "2026-08-30T10:01:00Z",
      eventType: "delegation.spawn.started",
      evidenceLevel: "observed",
      sourceCoverage: "exact-delegation",
    },
    {
      id: "evidence-stop",
      canonicalEventId: "canonical-stop",
      sessionId: "session-child",
      role: "outcome",
      occurredAt: "2026-08-30T10:08:00Z",
      eventType: "delegation.spawn.completed",
      evidenceLevel: "observed",
      sourceCoverage: "exact-delegation",
    },
  ],
};

function renderTrace(trace: DelegationTraceResponse = readyTrace) {
  vi.mocked(api.delegationTrace).mockResolvedValue(trace);
  const onOpenSession = vi.fn();
  const onOpenProcessEvidence = vi.fn();
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const view = render(
    <I18nextProvider i18n={i18n}>
      <QueryClientProvider client={client}>
        <DelegationTraceView
          sessionId="session-root"
          locale="zh-CN"
          onOpenSession={onOpenSession}
          onOpenProcessEvidence={onOpenProcessEvidence}
        />
      </QueryClientProvider>
    </I18nextProvider>,
  );
  return { ...view, onOpenSession, onOpenProcessEvidence };
}

describe("DelegationTraceView", () => {
  it("shows a bounded loading state before the trace is ready", async () => {
    let resolveTrace: ((trace: DelegationTraceResponse) => void) | undefined;
    vi.mocked(api.delegationTrace).mockReturnValue(new Promise((resolve) => {
      resolveTrace = resolve;
    }));
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <I18nextProvider i18n={i18n}>
        <QueryClientProvider client={client}>
          <DelegationTraceView sessionId="session-root" locale="zh-CN" onOpenSession={vi.fn()} onOpenProcessEvidence={vi.fn()} />
        </QueryClientProvider>
      </I18nextProvider>,
    );

    expect(await screen.findByText("正在加载委派证据…")).toBeTruthy();
    resolveTrace?.(readyTrace);
    expect(await screen.findByRole("heading", { name: "委派轨迹" })).toBeTruthy();
  });

  it("renders ready data and shares graph, timeline, and evidence selection", async () => {
    const { onOpenProcessEvidence, onOpenSession } = renderTrace();
    await screen.findByRole("heading", { name: "委派轨迹" });

    fireEvent.click(screen.getByRole("button", { name: "graph-edge-spawn-edge" }));
    expect(screen.getByRole("heading", { name: "创建" })).toBeTruthy();
    expect(screen.getAllByText("已观测").length).toBeGreaterThan(0);
    expect(document.body.textContent).toContain("子 Agent 创建开始");

    fireEvent.click(screen.getAllByRole("button", { name: /回到过程/ })[0]!);
    expect(onOpenProcessEvidence).toHaveBeenCalledWith(readyTrace.evidence[0]);
    fireEvent.click(screen.getAllByRole("button", { name: /打开会话/ })[0]!);
    expect(onOpenSession).toHaveBeenCalledWith("session-child");

    const timelineNode = screen.getByRole("button", { name: /Codex · child123 · 已完成/ });
    timelineNode.focus();
    fireEvent.click(timelineNode);
    expect(screen.getByRole("heading", { name: "Codex · child123" })).toBeTruthy();
  });

  it("keeps the graph labeled and exposes a keyboard-focusable list fallback", async () => {
    renderTrace();
    await screen.findByRole("heading", { name: "委派轨迹" });

    expect(screen.getByLabelText("包含 2 个 Agent 与 1 条关系的委派图")).toBeTruthy();
    const details = screen.getByText("关系列表（3 项）").closest("details") as HTMLDetailsElement;
    details.open = true;
    const firstFallbackButton = details.querySelector("button") as HTMLButtonElement;
    firstFallbackButton.focus();
    expect(document.activeElement).toBe(firstFallbackButton);
    fireEvent.click(firstFallbackButton);
    expect(screen.getByRole("heading", { name: "Codex · root1234" })).toBeTruthy();
  });

  it("filters relationships and keeps inferred evidence visibly partial", async () => {
    const partial: DelegationTraceResponse = {
      ...readyTrace,
      status: "partial",
      nodes: [...readyTrace.nodes, {
        ...readyTrace.nodes[1]!,
        id: "orphan",
        safeLabel: "Codex · orphan12",
        status: "waiting",
        evidenceLevel: "inferred",
        confidence: 0.45,
      }],
      edges: [...readyTrace.edges, {
        ...readyTrace.edges[0]!,
        id: "handoff-edge",
        to: "orphan",
        relationType: "handoff",
        status: "waiting",
        evidenceLevel: "inferred",
        confidence: 0.45,
      }],
      coverage: { ...readyTrace.coverage, capability: "derived", derivedCount: 1, inferredCount: 1 },
    };
    renderTrace(partial);
    await screen.findByText("关系覆盖不完整。");

    fireEvent.change(screen.getByRole("combobox", { name: "关系" }), { target: { value: "handoff" } });
    expect(screen.queryByRole("button", { name: "graph-edge-spawn-edge" })).toBeNull();
    expect(screen.getByRole("button", { name: "graph-edge-handoff-edge" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "graph-edge-handoff-edge" }));
    expect(screen.getByText("置信度 45%")).toBeTruthy();
    expect(screen.getAllByText("已推断").length).toBeGreaterThan(0);
  });

  it("shows truthful not-recorded and error/retry states", async () => {
    const notRecorded: DelegationTraceResponse = {
      ...readyTrace,
      status: "not-recorded",
      nodes: [],
      edges: [],
      evidence: [],
      coverage: { ...readyTrace.coverage, capability: "unavailable", relationCount: 0, evidenceCount: 0, unavailableSignals: ["delegation", "handoff"] },
    };
    renderTrace(notRecorded);
    expect(await screen.findByRole("heading", { name: "未记录委派关系" })).toBeTruthy();
    expect(screen.getByText("未记录委派关系").closest("section")?.textContent).toContain("VibeMeter 不会猜测关系");
  });

  it("does not render unmodeled raw prompt or path fields", async () => {
    const rawEnvelope = {
      ...readyTrace,
      rawPrompt: "DO_NOT_RENDER_PROMPT",
      rawPath: "/Users/private/repository",
      toolInput: "rm -rf secret",
    } as DelegationTraceResponse;
    renderTrace(rawEnvelope);
    await screen.findByRole("heading", { name: "委派轨迹" });
    expect(document.body.textContent).not.toContain("DO_NOT_RENDER_PROMPT");
    expect(document.body.textContent).not.toContain("/Users/private/repository");
    expect(document.body.textContent).not.toContain("rm -rf secret");
  });

  it("offers retry after a load error", async () => {
    vi.mocked(api.delegationTrace).mockRejectedValueOnce(new Error("offline")).mockResolvedValueOnce(readyTrace);
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <I18nextProvider i18n={i18n}>
        <QueryClientProvider client={client}>
          <DelegationTraceView sessionId="session-root" locale="zh-CN" onOpenSession={vi.fn()} onOpenProcessEvidence={vi.fn()} />
        </QueryClientProvider>
      </I18nextProvider>,
    );
    await waitFor(() => expect(screen.getByText("委派证据加载失败。")).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: "重试" }));
    expect(await screen.findByRole("heading", { name: "委派轨迹" })).toBeTruthy();
  });
});
