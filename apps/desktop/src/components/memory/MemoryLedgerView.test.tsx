// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import i18n from "../../i18n";
import { api } from "../../lib/api";
import type { MemoryLedgerResponse } from "../../types";
import { MemoryLedgerView } from "./MemoryLedgerView";

vi.mock("../../lib/api", () => ({ api: { memoryLedger: vi.fn() } }));

beforeAll(async () => {
  await i18n.changeLanguage("zh-CN");
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

const partialLedger: MemoryLedgerResponse = {
  status: "partial",
  sessionId: "session-parent",
  algorithmVersion: "memory-ledger-1.0.0",
  accesses: [{
    id: "access-read",
    agent: "codex",
    sessionId: "session-parent",
    operation: "read",
    status: "recorded",
    occurredAt: "2026-08-31T10:01:00Z",
    confidence: 0.72,
    evidenceLevel: "derived",
    sourceCoverage: "partial-memory-read",
    algorithmVersion: "memory-ledger-1.0.0",
    evidenceIds: ["canonical-memory-read"],
  }],
  coverage: {
    capability: "partial",
    sourceCoverage: "partial-memory-read",
    accessCount: 1,
    readCount: 1,
    writeCount: 0,
    evidenceCount: 1,
    observedCount: 0,
    derivedCount: 1,
    inferredCount: 0,
    unavailableOperations: ["write"],
  },
  evidence: [{
    id: "memory-evidence-read",
    canonicalEventId: "canonical-memory-read",
    sessionId: "session-parent",
    role: "access",
    occurredAt: "2026-08-31T10:01:00Z",
    eventType: "memory.read",
    evidenceLevel: "derived",
    sourceCoverage: "partial-memory-read",
  }],
};

function renderLedger(ledger: MemoryLedgerResponse = partialLedger) {
  vi.mocked(api.memoryLedger).mockResolvedValue(ledger);
  const onOpenSession = vi.fn();
  const onOpenProcessEvidence = vi.fn();
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const view = render(
    <I18nextProvider i18n={i18n}>
      <QueryClientProvider client={client}>
        <MemoryLedgerView
          sessionId="session-parent"
          locale="zh-CN"
          onOpenSession={onOpenSession}
          onOpenProcessEvidence={onOpenProcessEvidence}
        />
      </QueryClientProvider>
    </I18nextProvider>,
  );
  return { ...view, onOpenProcessEvidence, onOpenSession };
}

describe("MemoryLedgerView", () => {
  it("shows loading, partial coverage, and a keyboard-selectable evidence drawer", async () => {
    let resolveLedger: ((ledger: MemoryLedgerResponse) => void) | undefined;
    vi.mocked(api.memoryLedger).mockReturnValue(new Promise((resolve) => {
      resolveLedger = resolve;
    }));
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const onOpenSession = vi.fn();
    const onOpenProcessEvidence = vi.fn();
    render(
      <I18nextProvider i18n={i18n}>
        <QueryClientProvider client={client}>
          <MemoryLedgerView sessionId="session-parent" locale="zh-CN" onOpenSession={onOpenSession} onOpenProcessEvidence={onOpenProcessEvidence} />
        </QueryClientProvider>
      </I18nextProvider>,
    );
    expect(await screen.findByText("正在加载记忆证据…")).toBeTruthy();
    resolveLedger?.(partialLedger);
    expect(await screen.findByRole("heading", { name: "记忆账本" })).toBeTruthy();
    expect(screen.getByText("记忆覆盖不完整。")).toBeTruthy();
    expect(screen.getAllByText("已派生").length).toBeGreaterThan(0);

    const row = screen.getByRole("button", { name: /读取记忆 · .* · 已派生/ });
    row.focus();
    expect(document.activeElement).toBe(row);
    fireEvent.click(row);
    expect(screen.getByRole("heading", { name: "读取记忆" })).toBeTruthy();
    expect(screen.getByText("置信度 72%")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: /回到过程/ }));
    expect(onOpenProcessEvidence).toHaveBeenCalledWith(partialLedger.evidence[0]);
    fireEvent.click(screen.getAllByRole("button", { name: /打开会话/ })[0]!);
    expect(onOpenSession).toHaveBeenCalledWith("session-parent");
  });

  it("filters read and write access without inventing unavailable operations", async () => {
    const ready: MemoryLedgerResponse = {
      ...partialLedger,
      status: "ready",
      accesses: [
        { ...partialLedger.accesses[0]!, evidenceLevel: "observed", confidence: 0.98, sourceCoverage: "exact-memory-read" },
        {
          ...partialLedger.accesses[0]!,
          id: "access-write",
          operation: "write",
          occurredAt: "2026-08-31T10:02:00Z",
          evidenceLevel: "observed",
          confidence: 0.98,
          sourceCoverage: "exact-memory-write",
          evidenceIds: ["canonical-memory-write"],
        },
      ],
      coverage: {
        ...partialLedger.coverage,
        capability: "exact",
        sourceCoverage: "exact-memory-read",
        accessCount: 2,
        readCount: 1,
        writeCount: 1,
        evidenceCount: 2,
        observedCount: 2,
        derivedCount: 0,
        unavailableOperations: [],
      },
      evidence: [
        { ...partialLedger.evidence[0]!, evidenceLevel: "observed", sourceCoverage: "exact-memory-read" },
        {
          ...partialLedger.evidence[0]!,
          id: "memory-evidence-write",
          canonicalEventId: "canonical-memory-write",
          eventType: "memory.write",
          occurredAt: "2026-08-31T10:02:00Z",
          evidenceLevel: "observed",
          sourceCoverage: "exact-memory-write",
        },
      ],
    };
    renderLedger(ready);
    await screen.findByRole("heading", { name: "记忆账本" });
    fireEvent.click(screen.getByRole("button", { name: /^写入记忆1$/ }));
    expect(screen.queryByRole("button", { name: /读取记忆 ·/ })).toBeNull();
    expect(screen.getByRole("button", { name: /写入记忆 ·/ })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /^全部访问2$/ }));
    expect(screen.getByRole("button", { name: /读取记忆 ·/ })).toBeTruthy();
  });

  it("shows truthful not-recorded coverage and never renders raw fields", async () => {
    const notRecorded: MemoryLedgerResponse = {
      ...partialLedger,
      status: "not-recorded",
      accesses: [],
      evidence: [],
      coverage: {
        ...partialLedger.coverage,
        capability: "unavailable",
        sourceCoverage: "not-recorded",
        accessCount: 0,
        readCount: 0,
        evidenceCount: 0,
        derivedCount: 0,
        unavailableOperations: ["read", "write"],
      },
      rawPrompt: "DO_NOT_RENDER_PROMPT",
      rawPath: "/Users/private/.codex/memories",
      toolInput: "cat secret",
    } as MemoryLedgerResponse;
    renderLedger(notRecorded);
    expect(await screen.findByRole("heading", { name: "未记录记忆访问" })).toBeTruthy();
    expect(document.body.textContent).toContain("不会根据标题、文字或文件路径猜测访问");
    expect(document.body.textContent).not.toContain("DO_NOT_RENDER_PROMPT");
    expect(document.body.textContent).not.toContain("/Users/private");
    expect(document.body.textContent).not.toContain("cat secret");
  });

  it("offers retry after a load error", async () => {
    vi.mocked(api.memoryLedger).mockRejectedValueOnce(new Error("offline")).mockResolvedValueOnce(partialLedger);
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <I18nextProvider i18n={i18n}>
        <QueryClientProvider client={client}>
          <MemoryLedgerView sessionId="session-parent" locale="zh-CN" onOpenSession={vi.fn()} onOpenProcessEvidence={vi.fn()} />
        </QueryClientProvider>
      </I18nextProvider>,
    );
    await waitFor(() => expect(screen.getByText("记忆证据加载失败。")).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: "重试" }));
    expect(await screen.findByRole("heading", { name: "记忆账本" })).toBeTruthy();
  });

  it("uses theme variables instead of fixed light-only colors", async () => {
    const { container } = renderLedger();
    await screen.findByRole("heading", { name: "记忆账本" });
    expect(container.querySelector(".memory-ledger-view")).toBeTruthy();
    expect(container.querySelector("[style*='#']")).toBeNull();
  });
});
