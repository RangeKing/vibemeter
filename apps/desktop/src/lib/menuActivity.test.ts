import { describe, expect, it } from "vitest";
import type { DailyUsagePoint, TokenUsage } from "../types";
import { buildMenuActivity, MENU_ACTIVITY_DAYS } from "./menuActivity";

const emptyUsage: TokenUsage = {
  inputTokens: 0,
  outputTokens: 0,
  cacheReadTokens: 0,
  cacheWriteTokens: 0,
  cacheWrite1hTokens: 0,
  reasoningTokens: 0,
};

function point(date: string, inputTokens: number, sessionCount: number): DailyUsagePoint {
  return {
    date,
    agent: "codex",
    model: "gpt-test",
    usage: { ...emptyUsage, inputTokens },
    activeSeconds: 0,
    sessionCount,
    toolCalls: 0,
    errors: 0,
  };
}

describe("menu activity", () => {
  it("fills the latest 15 local dates and combines same-day rows", () => {
    const days = buildMenuActivity(
      [point("2026-07-31", 10, 1), point("2026-07-31", 20, 2), point("2026-07-20", 4, 1)],
      new Date("2026-07-31T12:00:00"),
    );

    expect(days).toHaveLength(MENU_ACTIVITY_DAYS);
    expect(days[0]).toEqual({ date: "2026-07-17", value: 0, sessions: 0 });
    expect(days.find((day) => day.date === "2026-07-20")).toEqual({ date: "2026-07-20", value: 4, sessions: 1 });
    expect(days.at(-1)).toEqual({ date: "2026-07-31", value: 30, sessions: 3 });
  });
});
