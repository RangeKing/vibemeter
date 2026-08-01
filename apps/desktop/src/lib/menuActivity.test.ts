import { describe, expect, it } from "vitest";
import type { DailyUsagePoint, HourlyUsagePoint, TokenUsage } from "../types";
import { buildMenuActivity, MENU_ACTIVITY_MAX_BARS } from "./menuActivity";

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

function hourlyPoint(hour: string, inputTokens: number): HourlyUsagePoint {
  return {
    hour,
    agent: "codex",
    model: "gpt-test",
    usage: { ...emptyUsage, inputTokens },
  };
}

describe("menu activity", () => {
  it("splits today into twelve fixed two-hour buckets", () => {
    const buckets = buildMenuActivity(
      [point("2026-07-31", 99, 2)],
      new Date("2026-07-31T12:00:00"),
      "today",
      [
        hourlyPoint("2026-07-31T00:00", 10),
        hourlyPoint("2026-07-31T01:00", 20),
        hourlyPoint("2026-07-31T02:00", 30),
        hourlyPoint("2026-07-30T23:00", 40),
      ],
    );

    expect(buckets).toHaveLength(12);
    expect(buckets[0]).toMatchObject({ startHour: 0, endHour: 2, value: 30 });
    expect(buckets[1]).toMatchObject({ startHour: 2, endHour: 4, value: 30 });
    expect(buckets.at(-1)).toMatchObject({ startHour: 22, endHour: 24, value: 0 });
  });

  it("fills a seven-day range and combines same-day rows", () => {
    const days = buildMenuActivity(
      [point("2026-07-31", 10, 1), point("2026-07-31", 20, 2), point("2026-07-20", 4, 1)],
      new Date("2026-07-31T12:00:00"),
      "7d",
    );

    expect(days).toHaveLength(7);
    expect(days[0]).toEqual({ key: "2026-07-25:2026-07-25", startDate: "2026-07-25", endDate: "2026-07-25", value: 0, sessions: 0 });
    expect(days.at(-1)).toEqual({ key: "2026-07-31:2026-07-31", startDate: "2026-07-31", endDate: "2026-07-31", value: 30, sessions: 3 });
  });

  it("condenses long ranges into at most 15 contiguous buckets", () => {
    const buckets = buildMenuActivity(
      [point("2026-07-30", 10, 1), point("2026-07-31", 20, 2)],
      new Date("2026-07-31T12:00:00"),
      "30d",
    );

    expect(buckets).toHaveLength(MENU_ACTIVITY_MAX_BARS);
    expect(buckets[0].startDate).toBe("2026-07-02");
    expect(buckets[0].endDate).toBe("2026-07-03");
    expect(buckets.at(-1)).toMatchObject({ startDate: "2026-07-30", endDate: "2026-07-31", value: 30, sessions: 3 });
  });
});
