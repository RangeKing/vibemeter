import { describe, expect, it } from "vitest";
import { formatResetRemaining, resetRemainingSeconds } from "./quota";

const now = new Date("2026-08-05T00:00:00.000Z");

describe("quota reset reminders", () => {
  it("returns a live countdown for a future reset", () => {
    const window = { id: "weekly", label: "quota.weekly", resetAt: "2026-08-06T02:30:00.000Z", provenance: "test" };
    const seconds = resetRemainingSeconds(window, now);

    expect(seconds).toBe(95_400);
    expect(formatResetRemaining(seconds ?? 0, "zh-CN")).toBe("1 天 2 小时");
    expect(formatResetRemaining(seconds ?? 0, "en-US")).toBe("1d 2h");
  });

  it("does not present a remaining time after the reset has passed", () => {
    const window = { id: "session", label: "quota.session", resetAt: "2026-08-04T23:59:00.000Z", provenance: "test" };

    expect(resetRemainingSeconds(window, now)).toBeUndefined();
  });

  it("rounds short countdowns up to a useful minute", () => {
    expect(formatResetRemaining(61, "zh-CN")).toBe("2 分钟");
    expect(formatResetRemaining(61, "en-US")).toBe("2m");
  });
});
