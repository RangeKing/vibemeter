// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import i18n from "../i18n";
import type { AttentionEvent, LiveSession } from "../types";
import {
  activeProviderCounts,
  collapsedMorphInsets,
  conversationTitleForSession,
  expandedHeightForSessions,
  formatLiveElapsed,
  liveElapsedEnd,
  leftWingWidthForSession,
  NotchAttentionQueue,
  notchAttentionItemsForActiveSessions,
  notchPulseValue,
  notchVisibleActions,
  pickRightWingSession,
} from "./NotchSurface";

beforeAll(async () => {
  await i18n.changeLanguage("zh-CN");
});

afterEach(cleanup);

function session(
  id: string,
  status: LiveSession["status"],
  agent: LiveSession["agent"] = "codex",
  updatedAt = new Date().toISOString(),
): LiveSession {
  return {
    id,
    sourceSessionId: id,
    agent,
    projectLabel: "vibemeter",
    status,
    phase: status === "completed" ? "completed" : "thinking",
    startedAt: updatedAt,
    updatedAt,
    actions: [],
    pulse: {
      lifecycle: { availability: "available", value: status, evidenceLevel: "observed", sourceCoverage: "exact" },
      workPhase: { availability: "available", value: "thinking", evidenceLevel: "derived", sourceCoverage: "exact" },
      attentionSignal: {
        availability: "available",
        value: status === "waiting"
          ? "needs-you"
          : status === "error"
            ? "blocking-error"
            : status === "completed"
              ? "completion-review"
              : "none",
        evidenceLevel: "derived",
        sourceCoverage: "exact",
      },
      freshness: { availability: "available", value: "fresh", evidenceLevel: "derived", sourceCoverage: "exact", ageSeconds: 0 },
    },
  };
}

describe("Notch session selection", () => {
  it("does not render an error attention item twice for the same active session", () => {
    const attention: AttentionEvent = {
      id: "attention-error-duplicate",
      kind: "error",
      state: "open",
      reasonKey: "blocking-error",
      agent: "zcode",
      sourceSessionId: "same-session",
      projectLabel: "vibemeter",
      openedAt: "2026-08-18T08:00:00Z",
      latestEvidenceAt: "2026-08-18T08:00:00Z",
      expiresAt: "9999-12-31T23:59:59Z",
      evidenceLevel: "observed",
      sourceCoverage: "exact-lifecycle",
      ruleVersion: "test",
      evidenceCount: 1,
      interventionCount: 0,
    };

    expect(
      notchAttentionItemsForActiveSessions([attention], [session("same-session", "error", "zcode")]),
    ).toEqual([]);
    expect(
      notchAttentionItemsForActiveSessions([attention], [session("same-session", "running", "zcode")]),
    ).toEqual([]);
    expect(
      notchAttentionItemsForActiveSessions([attention], [session("other-session", "error", "zcode")]),
    ).toHaveLength(1);
  });

  it("shows an honest unavailable state instead of an empty attention queue", () => {
    render(
      <I18nextProvider i18n={i18n}>
        <NotchAttentionQueue
          items={[]}
          available={false}
          onFeedback={vi.fn()}
          onJump={vi.fn()}
          onRemove={vi.fn()}
        />
      </I18nextProvider>,
    );

    expect(screen.getByRole("alert").textContent).toBe(
      "注意力状态暂时不可用，现有证据未被清空。",
    );
    expect(screen.queryByText("注意力雷达")).toBeNull();
  });

  it("shows the retained attention item when its jump fails", () => {
    const attention: AttentionEvent = {
      id: "attention-jump-failed",
      kind: "waiting",
      state: "open",
      reasonKey: "permission-required",
      agent: "codex",
      sourceSessionId: "source-session",
      projectLabel: "VibeMeter",
      openedAt: "2026-08-10T08:00:00Z",
      latestEvidenceAt: "2026-08-10T08:00:00Z",
      expiresAt: "9999-12-31T23:59:59Z",
      evidenceLevel: "observed",
      sourceCoverage: "exact-lifecycle",
      ruleVersion: "test",
      evidenceCount: 1,
      interventionCount: 0,
    };
    render(
      <I18nextProvider i18n={i18n}>
        <NotchAttentionQueue
          items={[attention]}
          jumpErrorId={attention.id}
          onFeedback={vi.fn()}
          onJump={vi.fn()}
          onRemove={vi.fn()}
        />
      </I18nextProvider>,
    );

    expect(screen.getByRole("alert").textContent).toBe("无法返回源会话，关注事件已保留，可稍后重试。");
    expect(screen.getByText("VibeMeter")).toBeTruthy();
    expect(screen.queryByText("注意力雷达")).toBeNull();
    expect(screen.getByRole("button", { name: "已处理" }).className).toContain(
      "notch-attention-action-text",
    );
  });

  it("activates attention actions on pointerdown", () => {
    const attention: AttentionEvent = {
      id: "attention-actions",
      kind: "error",
      state: "open",
      reasonKey: "blocking-error",
      agent: "codex",
      sourceSessionId: "source-session",
      projectLabel: "VibeMeter",
      openedAt: "2026-08-10T08:00:00Z",
      latestEvidenceAt: "2026-08-10T08:00:00Z",
      expiresAt: "9999-12-31T23:59:59Z",
      evidenceLevel: "observed",
      sourceCoverage: "exact-lifecycle",
      ruleVersion: "test",
      evidenceCount: 1,
      interventionCount: 0,
    };
    const onFeedback = vi.fn();
    const onJump = vi.fn();
    render(
      <I18nextProvider i18n={i18n}>
        <NotchAttentionQueue
          items={[attention]}
          onFeedback={onFeedback}
          onJump={onJump}
          onRemove={vi.fn()}
        />
      </I18nextProvider>,
    );

    fireEvent.pointerDown(screen.getByRole("button", { name: "已处理" }), { button: 0 });
    fireEvent.pointerDown(screen.getByRole("button", { name: "返回源会话" }), { button: 0 });

    expect(onFeedback).toHaveBeenCalledWith(attention.id, "handled");
    expect(onJump).toHaveBeenCalledWith(attention.id);
  });

  it("offers a removable attention action", () => {
    const attention: AttentionEvent = {
      id: "attention-remove",
      kind: "error",
      state: "open",
      reasonKey: "blocking-error",
      agent: "codex",
      sourceSessionId: "source-session",
      projectLabel: "VibeMeter",
      openedAt: "2026-08-10T08:00:00Z",
      latestEvidenceAt: "2026-08-10T08:00:00Z",
      expiresAt: "9999-12-31T23:59:59Z",
      evidenceLevel: "observed",
      sourceCoverage: "exact-lifecycle",
      ruleVersion: "test",
      evidenceCount: 1,
      interventionCount: 0,
    };
    const onRemove = vi.fn();
    render(
      <I18nextProvider i18n={i18n}>
        <NotchAttentionQueue
          items={[attention]}
          onFeedback={vi.fn()}
          onJump={vi.fn()}
          onRemove={onRemove}
        />
      </I18nextProvider>,
    );

    fireEvent.pointerDown(screen.getByRole("button", { name: "移除关注事件" }), { button: 0 });
    expect(onRemove).toHaveBeenCalledWith(attention.id);
  });

  it("does not render completion-review items in the Notch", () => {
    const attention: AttentionEvent = {
      id: "attention-completion-review",
      kind: "completion-review",
      state: "open",
      reasonKey: "completion-needs-review",
      agent: "codex",
      sourceSessionId: "matching-source-session",
      projectLabel: "vibemeter",
      openedAt: "2026-08-13T08:00:00Z",
      latestEvidenceAt: "2026-08-13T08:00:00Z",
      expiresAt: "9999-12-31T23:59:59Z",
      evidenceLevel: "observed",
      sourceCoverage: "exact-lifecycle",
      ruleVersion: "test",
      evidenceCount: 1,
      interventionCount: 0,
    };
    render(
      <I18nextProvider i18n={i18n}>
        <NotchAttentionQueue
          items={[attention]}
          onFeedback={vi.fn()}
          onJump={vi.fn()}
          onRemove={vi.fn()}
        />
      </I18nextProvider>,
    );

    expect(screen.queryByText("待确认完成")).toBeNull();
    expect(document.querySelector(".notch-attention-queue")).toBeNull();
  });

  it("formats a live conversation duration instead of a wall-clock timestamp", () => {
    const startedAt = "2026-07-26T00:00:00Z";
    expect(formatLiveElapsed(startedAt, Date.parse("2026-07-26T00:03:07Z"))).toBe("3:07");
    expect(formatLiveElapsed(startedAt, Date.parse("2026-07-26T01:02:09Z"))).toBe("1:02:09");
    expect(formatLiveElapsed("not-a-date", Date.now())).toBe("—");
  });

  it("freezes the current activity duration while a session needs attention", () => {
    const waiting = session("waiting", "waiting", "codex", "2026-07-26T00:03:00Z");
    waiting.startedAt = "2026-07-26T00:00:00Z";
    waiting.activityEndedAt = "2026-07-26T00:03:00Z";
    waiting.updatedAt = "2026-07-26T00:08:00Z";
    const now = Date.parse("2026-07-26T00:10:00Z");
    expect(formatLiveElapsed(waiting.startedAt, liveElapsedEnd(waiting, now))).toBe("3:00");

    const running = { ...waiting, status: "running" as const };
    expect(formatLiveElapsed(running.startedAt, liveElapsedEnd(running, now))).toBe("10:00");
  });

  it("keeps waiting and error ahead of running or completion cues", () => {
    const now = Date.now();
    const sessions = [
      session("running", "running"),
      session("completed", "completed", "codex", new Date(now - 500).toISOString()),
      session("error", "error"),
      session("waiting", "waiting"),
    ];
    expect(pickRightWingSession(sessions, now)?.id).toBe("waiting");
    expect(pickRightWingSession(sessions.filter((item) => item.status !== "waiting"), now)?.id).toBe(
      "error",
    );
  });

  it("shows a recent completion briefly, then returns to the running session", () => {
    const now = Date.now();
    const running = session("running", "running");
    const completed = session("completed", "completed", "codex", new Date(now - 500).toISOString());
    expect(pickRightWingSession([running, completed], now)?.id).toBe("completed");
    expect(pickRightWingSession([running, completed], now + 6_000)?.id).toBe("running");
  });

  it("keeps blocking errors ahead of stuck and stuck ahead of completion review", () => {
    const now = Date.now();
    const error = session("error", "error");
    const stuck = session("stuck", "running");
    stuck.pulse.attentionSignal.value = "stuck";
    const completed = session("completed", "completed", "codex", new Date(now - 500).toISOString());

    expect(pickRightWingSession([completed, stuck, error], now)?.id).toBe("error");
    expect(pickRightWingSession([completed, stuck], now)?.id).toBe("stuck");
  });

  it("does not keep a paused session in the active Notch wing", () => {
    const paused = session("paused", "paused");
    expect(pickRightWingSession([paused], Date.now())).toBeUndefined();
    expect(activeProviderCounts([paused]).active).toHaveLength(0);
  });

  it("keeps unavailable lifecycle evidence from posing as an exact error", () => {
    const experimental = session("experimental-error", "error", "kimi-code");
    experimental.pulse.lifecycle = {
      availability: "unknown",
      evidenceLevel: "not-recorded",
      sourceCoverage: "experimental",
    };
    experimental.pulse.workPhase = {
      availability: "available",
      value: "recent-activity",
      evidenceLevel: "observed",
      sourceCoverage: "experimental",
    };
    experimental.pulse.attentionSignal = {
      availability: "unknown",
      evidenceLevel: "not-recorded",
      sourceCoverage: "experimental",
    };
    const running = session("exact-running", "running", "codex");

    expect(notchPulseValue(experimental)).toBe("recent-activity");
    expect(notchVisibleActions(experimental)).toEqual([]);
    expect(pickRightWingSession([experimental, running], Date.now())?.id).toBe("exact-running");
  });

  it("counts only running, waiting, and error sessions by provider", () => {
    const counts = activeProviderCounts([
      session("codex-running", "running"),
      session("codex-idle", "idle"),
      session("claude-waiting", "waiting", "claude-code"),
      session("claude-completed", "completed", "claude-code"),
      session("claude-error", "error", "claude-code"),
      session("deepseek-running", "running", "deepseek-harness"),
    ]);
    expect(counts.active).toHaveLength(4);
    expect(counts.codex).toBe(1);
    expect(counts.claudeCode).toBe(2);
    expect(counts.deepSeekHarness).toBe(1);
  });

  it("uses project-aware width only for a single visible provider", () => {
    expect(leftWingWidthForSession()).toBe(88);
    expect(leftWingWidthForSession(session("one", "running"))).toBeGreaterThanOrEqual(88);
    expect(
      leftWingWidthForSession({
        projectLabel: "AnExtremelyLongProjectNameThatMustBeClamped",
      }),
    ).toBe(154);
  });

  it("morphs to the real asymmetric compact wings instead of the hardware cutout", () => {
    expect(collapsedMorphInsets(180, 88, 98, true)).toEqual({ left: 42, right: 32 });
    expect(collapsedMorphInsets(180, 88, 98, false)).toEqual({ left: 130, right: 130 });
  });

  it("keeps the available conversation title for CSS to truncate", () => {
    expect(
      conversationTitleForSession({
        projectLabel: "vibemeter",
        conversationTitle: "修复 Notch 排序",
      }),
    ).toBe("修复 Notch 排序");
    expect(
      conversationTitleForSession({
        projectLabel: "AnExtremelyLongProjectNameThatConsumesTheWholeRow",
        conversationTitle: "Conversation title",
      }),
    ).toBe("Conversation title");
    expect(
      conversationTitleForSession({
        projectLabel: "vibemeter",
        conversationTitle: "VibeMeter",
      }),
    ).toBeUndefined();
  });

  it("fits one session closely and grows with the visible instance count", () => {
    const one = expandedHeightForSessions([session("one", "running")], 32);
    const waiting = expandedHeightForSessions([session("waiting", "waiting")], 32);
    const two = expandedHeightForSessions(
      [session("one", "running"), session("two", "running")],
      32,
    );
    expect(one).toBe(170);
    expect(waiting).toBe(188);
    expect(two).toBe(257);
  });

  it("keeps growing past the former four-session height limit", () => {
    const four = expandedHeightForSessions(
      [
        session("one", "running"),
        session("two", "running"),
        session("three", "running"),
        session("four", "running"),
      ],
      32,
    );
    expect(four).toBe(431);
  });

  it("accounts for the completed group without compressing active cards", () => {
    const collapsed = expandedHeightForSessions([session("active", "running")], 32, {
      completedCount: 2,
      completedExpanded: false,
    });
    const expanded = expandedHeightForSessions([session("active", "running")], 32, {
      completedCount: 2,
      completedExpanded: true,
    });
    const withJumpError = expandedHeightForSessions([], 32, {
      completedCount: 1,
      completedExpanded: true,
      completedErrorCount: 1,
    });
    const withActiveJumpError = expandedHeightForSessions([session("active", "running")], 32, {
      activeErrorCount: 1,
    });
    expect(collapsed).toBe(205);
    expect(expanded).toBe(379);
    expect(withJumpError).toBe(223);
    expect(withActiveJumpError).toBe(188);
  });
});
