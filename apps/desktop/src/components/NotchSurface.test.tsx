import { describe, expect, it } from "vitest";
import type { LiveSession } from "../types";
import {
  activeProviderCounts,
  expandedHeightForSessions,
  formatLiveElapsed,
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

  it("fits one session closely and grows with the visible instance count", () => {
    const one = expandedHeightForSessions([session("one", "running")], 32);
    const waiting = expandedHeightForSessions([session("waiting", "waiting")], 32);
    const two = expandedHeightForSessions(
      [session("one", "running"), session("two", "running")],
      32,
    );
    expect(one).toBe(168);
    expect(waiting).toBe(186);
    expect(two).toBe(255);
  });
});
