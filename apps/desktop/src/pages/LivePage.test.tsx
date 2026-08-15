// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { I18nextProvider } from "react-i18next";
import i18n from "../i18n";
import type {
  AttentionEvent,
  AttentionQualityReport,
  LiveHistoryItem,
  LiveSession,
  LiveTimelinePoint,
} from "../types";
import {
  AttentionActions,
  AttentionHistory,
  AttentionQualityGate,
  AttentionQueue,
  HistoryList,
  LiveSessionCard,
  sortAttentionEvents,
  TimelineList,
  attentionHistoryNextOffset,
} from "./LivePage";

const historyItem: LiveHistoryItem = {
  id: "621",
  occurredAt: "2026-07-26T02:54:06Z",
  agent: "claude-code",
  projectLabel: "GlobalPhotovoltaic",
  status: "waiting",
  eventName: "PermissionRequest",
  sourceSessionId: "source-session",
  sessionId: "indexed-session",
};

function renderHistory(item: LiveHistoryItem, onOpenSession = vi.fn()) {
  render(
    <I18nextProvider i18n={i18n}>
      <HistoryList items={[item]} locale="zh-CN" onOpenSession={onOpenSession} />
    </I18nextProvider>,
  );
  return onOpenSession;
}

describe("Live history", () => {
  beforeAll(async () => {
    await i18n.changeLanguage("zh-CN");
  });

  afterEach(cleanup);

  it("shows a full local date and opens the matching indexed session", () => {
    const onOpenSession = renderHistory(historyItem);

    expect(screen.getByText(/2026年7月26日/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "返回源会话" }));

    expect(onOpenSession).toHaveBeenCalledWith("indexed-session");
  });

  it("keeps the jump action visible but disabled until indexing finishes", () => {
    renderHistory({ ...historyItem, sessionId: undefined });

    const button = screen.getByRole("button", { name: "返回源会话" }) as HTMLButtonElement;
    expect(button.disabled).toBe(true);
    expect(button.title).toBe("对应的本机会话尚未完成索引。");
  });
});

describe("Live timeline", () => {
  beforeAll(async () => {
    await i18n.changeLanguage("zh-CN");
  });

  afterEach(cleanup);

  it("keeps the timeline in a bounded scroll region", () => {
    const points: LiveTimelinePoint[] = Array.from({ length: 21 }, (_, index) => ({
      id: String(index),
      occurredAt: new Date(Date.UTC(2026, 6, 31, 12, 21 - index)).toISOString(),
      agent: "codex",
      projectLabel: `project-${index}`,
      status: "running",
      eventName: `event-${index}`,
      sourceSessionId: `session-${index}`,
    }));

    const { container } = render(
      <I18nextProvider i18n={i18n}>
        <TimelineList points={points} locale="zh-CN" />
      </I18nextProvider>,
    );

    expect(container.querySelector(".live-timeline-list")?.children).toHaveLength(21);
    expect(container.querySelector(".live-timeline-list")?.classList.contains("live-timeline-list")).toBe(true);
    expect(screen.getByText("project-0")).toBeTruthy();
  });
});

describe("Live work pulse", () => {
  beforeAll(async () => {
    await i18n.changeLanguage("zh-CN");
  });

  afterEach(cleanup);

  function liveSession(agent: LiveSession["agent"], status: LiveSession["status"]): LiveSession {
    return {
      id: `${agent}-${status}`,
      sourceSessionId: "source-session",
      agent,
      projectLabel: "VibeMeter",
      status,
      phase: status === "waiting" ? "needs-you" : "error",
      startedAt: "2026-08-10T08:00:00Z",
      updatedAt: "2026-08-10T08:00:20Z",
      actions: status === "error"
        ? [{ kind: "error", label: "Error", occurredAt: "2026-08-10T08:00:20Z" }]
        : [],
      pulse: {
        lifecycle: {
          availability: agent === "codex" ? "available" : "unknown",
          value: agent === "codex" ? status : undefined,
          evidenceLevel: agent === "codex" ? "observed" : "not-recorded",
          sourceCoverage: agent === "codex" ? "exact" : "experimental",
        },
        workPhase: {
          availability: "available",
          value: agent === "codex" ? "needs-you" : "recent-activity",
          evidenceLevel: agent === "codex" ? "derived" : "observed",
          sourceCoverage: agent === "codex" ? "exact" : "experimental",
        },
        attentionSignal: {
          availability: agent === "codex" ? "available" : "unknown",
          value: agent === "codex" ? "needs-you" : undefined,
          evidenceLevel: agent === "codex" ? "derived" : "not-recorded",
          sourceCoverage: agent === "codex" ? "exact" : "experimental",
        },
        freshness: {
          availability: "available",
          value: "fresh",
          evidenceLevel: "derived",
          sourceCoverage: agent === "codex" ? "exact" : "experimental",
          ageSeconds: 10,
        },
      },
    };
  }

  it("renders the four independent dimensions for exact live sources", () => {
    render(
      <I18nextProvider i18n={i18n}>
        <LiveSessionCard session={liveSession("codex", "waiting")} locale="zh-CN" />
      </I18nextProvider>,
    );

    expect(screen.getByText("生命周期")).toBeTruthy();
    expect(screen.getByText("工作阶段")).toBeTruthy();
    expect(screen.getByText("注意力信号")).toBeTruthy();
    expect(screen.getByText("新鲜度")).toBeTruthy();
    expect(screen.getAllByText("需要你").length).toBeGreaterThan(0);
  });

  it("shows only recent activity and freshness when lifecycle evidence is unavailable", () => {
    render(
      <I18nextProvider i18n={i18n}>
        <LiveSessionCard session={liveSession("kimi-code", "error")} locale="zh-CN" />
      </I18nextProvider>,
    );

    expect(screen.getAllByText("近期活动").length).toBeGreaterThan(0);
    expect(screen.getAllByText("未知")).toHaveLength(2);
    expect(screen.queryByText("错误")).toBeNull();
    expect(screen.getByLabelText("最近动作").children).toHaveLength(0);
  });
});

describe("Attention actions", () => {
  beforeAll(async () => {
    await i18n.changeLanguage("zh-CN");
  });

  afterEach(cleanup);

  it("offers only fixed feedback choices and no free-text control", () => {
    const onFeedback = vi.fn();
    render(
      <I18nextProvider i18n={i18n}>
        <AttentionActions kind="stuck" onFeedback={onFeedback} />
      </I18nextProvider>,
    );

    expect(screen.getAllByRole("button")).toHaveLength(4);
    expect(screen.queryByRole("textbox")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "不是卡住" }));
    expect(onFeedback).toHaveBeenCalledWith("not-stuck");
  });

  function attention(kind: AttentionEvent["kind"], id: string, openedAt: string): AttentionEvent {
    return {
      id,
      kind,
      state: "open",
      reasonKey: kind,
      agent: "codex",
      sourceSessionId: `${id}-source`,
      projectLabel: id,
      openedAt,
      latestEvidenceAt: openedAt,
      expiresAt: "2026-08-11T08:00:00Z",
      evidenceLevel: kind === "stuck" ? "derived" : "observed",
      sourceCoverage: "exact-lifecycle",
      ruleVersion: "test",
      evidenceCount: 3,
      interventionCount: 0,
    };
  }

  it("sorts the multi-session queue by fixed priority with stable ties", () => {
    const openedAt = "2026-08-10T08:00:00Z";
    const sorted = sortAttentionEvents([
      attention("completion-review", "completion", openedAt),
      attention("stuck", "stuck-b", openedAt),
      attention("error", "error", openedAt),
      attention("waiting", "waiting", openedAt),
      attention("stuck", "stuck-a", openedAt),
    ]);
    expect(sorted.map((event) => event.id)).toEqual([
      "waiting",
      "error",
      "stuck-a",
      "stuck-b",
      "completion",
    ]);
  });

  it("renders an honest empty queue", () => {
    render(
      <I18nextProvider i18n={i18n}>
        <AttentionQueue items={[]} onFeedback={vi.fn()} onJump={vi.fn()} />
      </I18nextProvider>,
    );
    expect(screen.getByText("暂无需要关注的事件")).toBeTruthy();
  });

  it("keeps the attention event visible and shows a jump failure", () => {
    const event = attention("waiting", "jump-failed", "2026-08-10T08:00:00Z");
    render(
      <I18nextProvider i18n={i18n}>
        <AttentionQueue
          items={[event]}
          jumpErrors={new Set([event.id])}
          onFeedback={vi.fn()}
          onJump={vi.fn()}
        />
      </I18nextProvider>,
    );

    expect(screen.getByRole("alert").textContent).toBe("无法返回源会话，关注事件已保留，可稍后重试。");
    expect(screen.getByText("需要你")).toBeTruthy();
  });

  it("shows the sanitized conversation title in the attention queue", () => {
    const event = attention("completion-review", "review", "2026-08-10T08:00:00Z");
    event.projectLabel = "vibemeter";
    event.conversationTitle = "VibeMeter 可视化功能";
    render(
      <I18nextProvider i18n={i18n}>
        <AttentionQueue items={[event]} onFeedback={vi.fn()} onJump={vi.fn()} />
      </I18nextProvider>,
    );

    expect(screen.getByText(/VibeMeter 可视化功能/)).toBeTruthy();
  });

  it("shows a private session reference when no trusted title exists", () => {
    const event = attention("completion-review", "private-reference", "2026-08-10T08:00:00Z");
    event.projectLabel = "vibemeter";
    event.sourceSessionId = "raw-private-session-id";
    render(
      <I18nextProvider i18n={i18n}>
        <AttentionQueue items={[event]} onFeedback={vi.fn()} onJump={vi.fn()} />
      </I18nextProvider>,
    );

    expect(screen.getByText(/会话 [0-9A-F]{6}/)).toBeTruthy();
    expect(screen.queryByText(/raw-private-session-id/)).toBeNull();
  });

  it("loads attention history in bounded pages", () => {
    const onLoadMore = vi.fn();
    render(
      <I18nextProvider i18n={i18n}>
        <AttentionHistory
          items={[attention("error", "resolved-error", "2026-08-10T08:00:00Z")]}
          hasMore
          onLoadMore={onLoadMore}
        />
      </I18nextProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "加载更多注意力历史" }));
    expect(onLoadMore).toHaveBeenCalledOnce();
  });

  it("advances history offsets beyond one hundred rows", () => {
    const page = Array.from({ length: 51 }, (_, index) =>
      attention("error", `history-${index}`, `2026-08-10T08:00:${String(index).padStart(2, "0")}Z`));
    expect(attentionHistoryNextOffset(page, [page])).toBe(50);
    expect(attentionHistoryNextOffset(page, [page, page])).toBe(100);
    expect(attentionHistoryNextOffset(page.slice(0, 50), [page, page, page])).toBeUndefined();
  });

  it("keeps the hard quality gate incomplete when local evidence is missing", () => {
    const report: AttentionQualityReport = {
      reviewedSamples: 0,
      stuckPrecision: null,
      feedbackSamples: 0,
      falsePositiveRate: null,
      notificationSamples: 0,
      notificationP95Seconds: null,
      jumpAttempts: 0,
      jumpSuccessRate: null,
      realAppVerified: false,
      requiredSamples: 100,
      requiredPrecision: 0.9,
      maximumFalsePositiveRate: 0.1,
      maximumNotificationP95Seconds: 2,
      requiredJumpSuccessRate: 0.95,
      passed: false,
    };
    render(
      <I18nextProvider i18n={i18n}>
        <AttentionQualityGate report={report} />
      </I18nextProvider>,
    );

    expect(screen.getByText("验收未完成")).toBeTruthy();
    expect(screen.getByText("0 / 100")).toBeTruthy();
    expect(screen.getAllByText("未记录").length).toBeGreaterThanOrEqual(3);
  });
});
