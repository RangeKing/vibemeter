import { describe, expect, it } from "vitest";
import type { LiveSession } from "../types";
import {
  activeProviderCounts,
  collapsedMorphInsets,
  conversationTitleForSession,
  expandedHeightForSessions,
  formatLiveElapsed,
  liveElapsedEnd,
  leftWingWidthForSession,
  pickRightWingSession,
} from "./NotchSurface";

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
  };
}

describe("Notch session selection", () => {
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

  it("does not keep a paused session in the active Notch wing", () => {
    const paused = session("paused", "paused");
    expect(pickRightWingSession([paused], Date.now())).toBeUndefined();
    expect(activeProviderCounts([paused]).active).toHaveLength(0);
  });

  it("counts only running, waiting, and error sessions by provider", () => {
    const counts = activeProviderCounts([
      session("codex-running", "running"),
      session("codex-idle", "idle"),
      session("claude-waiting", "waiting", "claude-code"),
      session("claude-completed", "completed", "claude-code"),
      session("claude-error", "error", "claude-code"),
    ]);
    expect(counts.active).toHaveLength(3);
    expect(counts.codex).toBe(1);
    expect(counts.claudeCode).toBe(2);
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
