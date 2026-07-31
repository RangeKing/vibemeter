import { describe, expect, it } from "vitest";
import { cacheTokenTotal, sumTokenUsage, tokenTotal } from "./format";

describe("Token usage totals", () => {
  it("keeps input, output, and all cache classes independently visible", () => {
    const usage = sumTokenUsage([
      { inputTokens: 10, outputTokens: 2, cacheReadTokens: 20, cacheWriteTokens: 3, cacheWrite1hTokens: 4, reasoningTokens: 1 },
      { inputTokens: 5, outputTokens: 8, cacheReadTokens: 1, cacheWriteTokens: 2, cacheWrite1hTokens: 3, reasoningTokens: 6 },
    ]);

    expect(usage.inputTokens).toBe(15);
    expect(usage.outputTokens).toBe(10);
    expect(cacheTokenTotal(usage)).toBe(33);
    expect(tokenTotal(usage)).toBe(58);
    expect(usage.reasoningTokens).toBe(7);
  });
});
