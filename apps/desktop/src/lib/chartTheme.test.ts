import { describe, expect, it } from "vitest";
import { AGENT_CHART_FALLBACKS } from "./chartTheme";

function rgb(hex: string) {
  const value = hex.slice(1);
  return [0, 2, 4].map((offset) => Number.parseInt(value.slice(offset, offset + 2), 16));
}

function distance(left: string, right: string) {
  const a = rgb(left);
  const b = rgb(right);
  return Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);
}

describe("agent chart palette", () => {
  it("keeps every Agent fallback color visibly separated", () => {
    for (let left = 0; left < AGENT_CHART_FALLBACKS.length; left += 1) {
      for (let right = left + 1; right < AGENT_CHART_FALLBACKS.length; right += 1) {
        expect(distance(AGENT_CHART_FALLBACKS[left], AGENT_CHART_FALLBACKS[right])).toBeGreaterThanOrEqual(60);
      }
    }
  });
});
