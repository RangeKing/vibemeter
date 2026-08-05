import { describe, expect, it } from "vitest";
import type { ChartColors } from "../lib/chartTheme";
import { trendOption } from "./DataPage";

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
