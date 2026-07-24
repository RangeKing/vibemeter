import { describe, expect, it } from "vitest";
import type { ProviderAccountUsage } from "../types";
import { compactProviderUsageBuckets, summarizeProviderAccountUsage } from "./providerUsage";

const usage: ProviderAccountUsage = {
  periodStart: "2026-07-01",
  periodEnd: "2026-07-24",
  fetchedAt: "2026-07-24T02:00:00Z",
  scope: "account",
  daily: [
    {
      date: "2026-07-17",
      model: "old-model",
      inputTokens: 900,
      outputTokens: 100,
      cacheReadTokens: 0,
      cacheWriteTokens: 0,
      apiCostUsd: 1,
      meteredCostUsd: 0.8,
      requestCount: 1,
      tokenRequestCount: 1,
    },
    {
      date: "2026-07-18",
      model: "composer-1",
      inputTokens: 100,
      outputTokens: 20,
      cacheReadTokens: 30,
      cacheWriteTokens: 10,
      apiCostUsd: 0.2,
      meteredCostUsd: 0.1,
      requestCount: 1,
      tokenRequestCount: 1,
    },
    {
      date: "2026-07-24",
      model: "composer-1",
      inputTokens: 200,
      outputTokens: 40,
      cacheReadTokens: 60,
      cacheWriteTokens: 20,
      apiCostUsd: 0.4,
      meteredCostUsd: 0.3,
      requestCount: 2,
      tokenRequestCount: 1,
    },
  ],
};

describe("Cursor dashboard range aggregation", () => {
  it("uses the selected inclusive local-day range instead of a lifetime total", () => {
    const result = summarizeProviderAccountUsage(usage, "7d", new Date(2026, 6, 24, 12));
    expect(result.periodStart).toBe("2026-07-18");
    expect(result.periodEnd).toBe("2026-07-24");
    expect(result.daily).toHaveLength(7);
    expect(result.totalTokens).toBe(480);
    expect(result.apiCostUsd).toBeCloseTo(0.6);
    expect(result.meteredCostUsd).toBeCloseTo(0.4);
    expect(result.requestCount).toBe(3);
    expect(result.models[0]).toEqual({ model: "composer-1", tokens: 480, requestCount: 3 });
  });

  it("does not publish partial cost totals", () => {
    const incomplete: ProviderAccountUsage = {
      ...usage,
      daily: usage.daily.map((row, index) => index === 1
        ? { ...row, apiCostUsd: null, meteredCostUsd: null }
        : row),
    };
    const result = summarizeProviderAccountUsage(incomplete, "7d", new Date(2026, 6, 24, 12));
    expect(result.totalTokens).toBe(480);
    expect(result.apiCostUsd).toBeUndefined();
    expect(result.meteredCostUsd).toBeUndefined();
  });

  it("compacts long histories without changing token totals", () => {
    const result = summarizeProviderAccountUsage(usage, "30d", new Date(2026, 6, 24, 12));
    const buckets = compactProviderUsageBuckets(result.daily, 6);
    expect(buckets.length).toBeLessThanOrEqual(6);
    expect(buckets.reduce((sum, bucket) => sum + bucket.tokens, 0)).toBe(result.totalTokens);
  });
});
