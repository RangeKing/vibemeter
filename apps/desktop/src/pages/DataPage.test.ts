import { describe, expect, it } from "vitest";
import type { ChartColors } from "../lib/chartTheme";
import { estimatedCostForPoints, trendOption } from "./DataPage";

const colors: ChartColors = {
  series: ["#111", "#222", "#333", "#444", "#555", "#666"],
  agent: ["#111", "#222", "#333", "#444", "#555", "#666"],
  model: ["#111", "#222", "#333", "#444", "#555", "#666"],
  text: "#111",
  textSecondary: "#222",
  textTertiary: "#333",
  hairline: "#444",
  paper: "#555",
  positive: "#666",
};

describe("Data trend chart", () => {
  it("plots the observed peak instead of a smoothed-down value", () => {
    const option = trendOption([{
      agent: "codex",
      buckets: ["2026-07-11"],
      values: [800_000_000],
      total: 800_000_000,
    }], colors, "zh-CN", false);
    const firstSeries = Array.isArray(option.series) ? option.series[0] : undefined;

    expect((firstSeries as { data?: number[] } | undefined)?.data).toEqual([800_000_000]);
  });
});

describe("Data cost ledger", () => {
  it("sums only the currently selected agents", () => {
    const usage = {
      inputTokens: 1,
      outputTokens: 0,
      cacheReadTokens: 0,
      cacheWriteTokens: 0,
      cacheWrite1hTokens: 0,
      reasoningTokens: 0,
    };
    expect(estimatedCostForPoints([
      { date: "2026-08-01", agent: "codex", model: "gpt-5.4", usage, activeSeconds: 1, sessionCount: 1, toolCalls: 0, errors: 0, estimatedCostUsd: 1.25 },
      { date: "2026-08-01", agent: "claude-code", model: "claude-sonnet-4-6", usage, activeSeconds: 1, sessionCount: 1, toolCalls: 0, errors: 0, estimatedCostUsd: 3.0 },
    ].filter((point) => point.agent === "claude-code"))).toBe(3.0);
  });

  it("preserves zero-dollar prices as a known value", () => {
    expect(estimatedCostForPoints([{
      date: "2026-08-01",
      agent: "hermes",
      model: "glm-4.7-flash",
      usage: { inputTokens: 1, outputTokens: 0, cacheReadTokens: 0, cacheWriteTokens: 0, cacheWrite1hTokens: 0, reasoningTokens: 0 },
      activeSeconds: 1,
      sessionCount: 1,
      toolCalls: 0,
      errors: 0,
      estimatedCostUsd: 0,
    }])).toBe(0);
  });
});
