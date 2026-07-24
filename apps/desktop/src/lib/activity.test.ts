import { describe, expect, it } from "vitest";
import type { DailyUsagePoint, HourlyUsagePoint } from "../types";
import { buildActivityDays, buildHourlyActivity, buildTrendBuckets, heatmapLayout } from "./activity";

function point(date: string, agent: "codex" | "claude-code", tokens: number): DailyUsagePoint {
  return {
    date,
    agent,
    model: "test",
    usage: { inputTokens: tokens, outputTokens: 0, cacheReadTokens: 0, cacheWriteTokens: 0, cacheWrite1hTokens: 0, reasoningTokens: 0 },
    activeSeconds: 0,
    sessionCount: 1,
    toolCalls: 0,
    errors: 0,
  };
}

function hourPoint(hour: string, agent: "codex" | "claude-code", tokens: number): HourlyUsagePoint {
  return {
    hour,
    agent,
    model: "test",
    usage: { inputTokens: tokens, outputTokens: 0, cacheReadTokens: 0, cacheWriteTokens: 0, cacheWrite1hTokens: 0, reasoningTokens: 0 },
  };
}

describe("range-aware activity series", () => {
  const data = [
    point("2026-03-04", "codex", 4),
    point("2026-07-19", "claude-code", 19),
    point("2026-07-20", "codex", 20),
  ];

  it("fills every date in short ranges instead of compressing inactive gaps", () => {
    const days = buildActivityDays(data, "7d", "2026-07-20");
    expect(days).toHaveLength(7);
    expect(days[0].date).toBe("2026-07-14");
    expect(days[0].total).toBe(0);
    expect(days.at(-1)?.total).toBe(20);
  });

  it("uses weekly buckets for 90 days and monthly buckets for a year", () => {
    expect(buildTrendBuckets(data, "90d", "2026-07-20")).toHaveLength(13);
    const yearly = buildTrendBuckets(data, "year", "2026-07-20");
    expect(yearly.every((bucket) => bucket.granularity === "month")).toBe(true);
    expect(yearly.length).toBeGreaterThanOrEqual(12);
  });

  it("uses the actual observed span for all-time charts", () => {
    const days = buildActivityDays(data, "all", "2026-07-20");
    expect(days[0].date).toBe("2026-03-04");
    expect(days.at(-1)?.date).toBe("2026-07-20");
    expect(days).toHaveLength(139);
    expect(buildTrendBuckets(data, "all", "2026-07-20").every((bucket) => bucket.granularity === "week")).toBe(true);
  });

  it("switches long heatmaps to seven-row week layouts", () => {
    expect(heatmapLayout(30)).toEqual({ columns: 30, rows: 1, weekLayout: false, dense: false });
    expect(heatmapLayout(90)).toEqual({ columns: 13, rows: 7, weekLayout: true, dense: false });
    expect(heatmapLayout(365)).toEqual({ columns: 53, rows: 7, weekLayout: true, dense: true });
    expect(heatmapLayout(42, true)).toEqual({ columns: 42, rows: 1, weekLayout: false, dense: true });
  });

  it("uses one-hour cells today and four-hour cells across seven days", () => {
    const reference = new Date(2026, 6, 20, 14, 37);
    const today = buildHourlyActivity([
      hourPoint("2026-07-20T09:00", "claude-code", 90),
      hourPoint("2026-07-20T09:00", "codex", 10),
    ], "today", reference);
    expect(today).toHaveLength(15);
    expect(today[9]).toMatchObject({ startAt: "2026-07-20T09:00", claude: 90, codex: 10, total: 100 });

    const week = buildHourlyActivity([
      hourPoint("2026-07-20T12:00", "claude-code", 40),
      hourPoint("2026-07-20T15:00", "claude-code", 60),
    ], "7d", reference);
    expect(week).toHaveLength(40);
    expect(week.at(-1)).toMatchObject({ startAt: "2026-07-20T12:00", total: 100, granularity: "four-hours" });
  });
});
